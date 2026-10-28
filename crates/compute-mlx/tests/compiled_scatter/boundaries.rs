use super::*;
use tensor_core::TensorError;

#[test]
fn low_replace_preserves_every_payload_and_extrema_keep_tiny_signed_zero_bits() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let n = 65537;
        let mut g = b.program();
        let x = g.input_low(dtype, shape(&[n])).unwrap();
        let i = g.input_u32(shape(&[65536])).unwrap();
        let u = g.input_low(dtype, shape(&[65536])).unwrap();
        let r = g.scatter_low(x, i, u, ScatterOp::Replace, 0).unwrap();
        let p = g.compile(&[r.values, r.invalid_count]).unwrap();
        let x = b.upload_low(dtype, shape(&[n]), &vec![0x8000; n]).unwrap();
        let raw: Vec<u16> = (0..=u16::MAX).collect();
        let u = b.upload_low(dtype, shape(&[65536]), &raw).unwrap();
        let i = b
            .upload_u32(shape(&[65536]), &(0..65536).collect::<Vec<u32>>())
            .unwrap();
        let result = p
            .run_typed(&[(&x).into(), (&i).into(), (&u).into()])
            .unwrap();
        let mut expected = raw;
        expected.push(0x8000);
        assert_eq!(
            b.read_low_bits(result[0].as_low().unwrap()).unwrap(),
            expected
        );
        assert_eq!(b.read_u32(result[1].as_tensor().unwrap()).unwrap(), [0]);
        traces(&p, 1);
        let max = match dtype {
            LowDtype::F16 => 0x7bff,
            LowDtype::Bf16 => 0x7f7f,
        };
        let mut g = b.program();
        let x = g.input_low(dtype, shape(&[5])).unwrap();
        let i = g.input_u32(shape(&[5])).unwrap();
        let u = g.input_low(dtype, shape(&[5])).unwrap();
        let mut outputs = vec![];
        for op in [ScatterOp::Min, ScatterOp::Max] {
            outputs.push(g.scatter_low(x, i, u, op, 0).unwrap().values);
            outputs.push(g.scatter_low_f32(x, i, u, op, 0).unwrap().values);
        }
        let p = g.compile(&outputs).unwrap();
        let x = b
            .upload_low(dtype, shape(&[5]), &[0, 0x8000, 1, 0x8001, max])
            .unwrap();
        let u = b
            .upload_low(dtype, shape(&[5]), &[0x8000, 0, 2, 0x8002, max])
            .unwrap();
        let i = b.upload_u32(shape(&[5]), &[0, 1, 2, 3, 4]).unwrap();
        let result = p
            .run_typed(&[(&x).into(), (&i).into(), (&u).into()])
            .unwrap();
        for (j, raw) in [[0x8000, 0x8000, 1, 0x8002, max], [0, 0, 2, 0x8001, max]]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                b.read_low_bits(result[j * 2].as_low().unwrap()).unwrap(),
                raw
            );
            let actual = b.read_f32(result[j * 2 + 1].as_tensor().unwrap()).unwrap();
            assert_eq!(
                actual.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                raw.iter()
                    .map(|&x| decode(dtype, x).to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn empty_base_counts_compose_on_device_and_scalar_indices_work() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut g = b.program();
        let x = g.input_low(dtype, shape(&[0, 3])).unwrap();
        let i = g.input_u32(shape(&[3])).unwrap();
        let u = g.input_low(dtype, shape(&[])).unwrap();
        let counts = g.input_u32(shape(&[4])).unwrap();
        let one = g.input_u32(shape(&[])).unwrap();
        let mut outputs = vec![];
        for op in OPS {
            for axis in [0, 1] {
                let r = g.scatter_low(x, i, u, op, axis).unwrap();
                outputs.push(r.values);
                outputs.push(r.invalid_count);
                let updated = g
                    .scatter_u32(counts, r.invalid_count, one, ScatterOp::Add, 0)
                    .unwrap();
                outputs.push(updated.values);
            }
        }
        let p = g.compile(&outputs).unwrap();
        let x = b.upload_low(dtype, shape(&[0, 3]), &[]).unwrap();
        let i = b.upload_u32(shape(&[3]), &[0, 3, u32::MAX]).unwrap();
        let u = b.upload_low(dtype, shape(&[]), &[1]).unwrap();
        let count = b.upload_u32(shape(&[4]), &[0; 4]).unwrap();
        let one = b.upload_u32(shape(&[]), &[1]).unwrap();
        let result = p
            .run_typed(&[
                (&x).into(),
                (&i).into(),
                (&u).into(),
                (&count).into(),
                (&one).into(),
            ])
            .unwrap();
        for j in 0..10 {
            assert!(
                b.read_low_bits(result[j * 3].as_low().unwrap())
                    .unwrap()
                    .is_empty()
            );
            let n = if j % 2 == 0 { 3 } else { 2 };
            assert_eq!(
                b.read_u32(result[j * 3 + 1].as_tensor().unwrap()).unwrap(),
                [n]
            );
            let mut expected = [0; 4];
            expected[n as usize] = 1;
            assert_eq!(
                b.read_u32(result[j * 3 + 2].as_tensor().unwrap()).unwrap(),
                expected
            );
        }
        let mut g = b.program();
        let x = g.input_low(dtype, shape(&[3])).unwrap();
        let i = g.input_u32(shape(&[])).unwrap();
        let u = g.input_low(dtype, shape(&[])).unwrap();
        let r = g.scatter_low_f32(x, i, u, ScatterOp::Add, 0).unwrap();
        let p = g.compile(&[r.values, r.invalid_count]).unwrap();
        let x = b
            .upload_low(dtype, shape(&[3]), &[encode(dtype, 1.); 3])
            .unwrap();
        let u = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 0.5)])
            .unwrap();
        for (run, index) in [1, u32::MAX, 2].into_iter().enumerate() {
            let i = b.upload_u32(shape(&[]), &[index]).unwrap();
            let result = p
                .run_typed(&[(&x).into(), (&i).into(), (&u).into()])
                .unwrap();
            let mut expected = [1.; 3];
            if index < 3 {
                expected[index as usize] = 1.5
            };
            assert_eq!(
                b.read_f32(result[0].as_tensor().unwrap()).unwrap(),
                expected
            );
            assert_eq!(
                b.read_u32(result[1].as_tensor().unwrap()).unwrap(),
                [u32::from(index >= 3)]
            );
            traces(&p, run + 1);
        }
    }
}

#[test]
fn invalid_scatter_builds_roll_back_before_empty_and_empty_indices_preserve_base() {
    let Some(b) = backend() else { return };
    let mut g = b.program();
    let x = g.input_low(LowDtype::Bf16, shape(&[5])).unwrap();
    let i = g.input_u32(shape(&[0])).unwrap();
    let u = g.input_low(LowDtype::Bf16, shape(&[])).unwrap();
    let wrong = g.input_low(LowDtype::F16, shape(&[])).unwrap();
    let f = g.input(shape(&[])).unwrap();
    for op in OPS {
        assert!(matches!(
            g.scatter_low(x, i, wrong, op, 0),
            Err(MlxError::Contract(TensorError::LowDtypeMismatch { .. }))
        ));
        assert!(g.scatter_low(x, f, u, op, 0).is_err());
        assert!(g.scatter_low(x, i, u, op, 1).is_err());
    }
    let mut foreign = b.program();
    let bad = foreign.input_u32(shape(&[0])).unwrap();
    assert!(g.scatter_low(x, bad, u, ScatterOp::Add, 0).is_err());
    let mut outputs = vec![];
    for op in OPS {
        let r = g.scatter_low(x, i, u, op, 0).unwrap();
        outputs.extend([r.values, r.invalid_count]);
    }
    let p = g.compile(&outputs).unwrap();
    let raw = [0, 0x8000, 1, 0x7fc1, 0xffff];
    let x = b.upload_low(LowDtype::Bf16, shape(&[5]), &raw).unwrap();
    let i = b.upload_u32(shape(&[0]), &[]).unwrap();
    let u = b.upload_low(LowDtype::Bf16, shape(&[]), &[0]).unwrap();
    let wrong = b.upload_low(LowDtype::F16, shape(&[]), &[0]).unwrap();
    let f = b.upload_f32(shape(&[]), &[0.]).unwrap();
    let result = p
        .run_typed(&[
            (&x).into(),
            (&i).into(),
            (&u).into(),
            (&wrong).into(),
            (&f).into(),
        ])
        .unwrap();
    for j in 0..5 {
        assert_eq!(
            b.read_low_bits(result[j * 2].as_low().unwrap()).unwrap(),
            raw
        );
        assert_eq!(
            b.read_u32(result[j * 2 + 1].as_tensor().unwrap()).unwrap(),
            [0]
        );
    }
    traces(&p, 1);
}
