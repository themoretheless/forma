use super::*;
use tensor_core::CompareOp;

#[test]
fn unsigned_compare_select_and_reduction_compose_without_signed_or_f32_promotion() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    let a = graph.input_u32(shape(&[2, 5])).unwrap();
    let c = graph.input_u32(shape(&[5])).unwrap();
    let mask = graph.input_u32(shape(&[2, 5])).unwrap();
    let mut outputs = Vec::new();
    let ops = [
        CompareOp::Equal,
        CompareOp::NotEqual,
        CompareOp::Less,
        CompareOp::LessEqual,
        CompareOp::Greater,
        CompareOp::GreaterEqual,
    ];
    for op in ops {
        outputs.push(graph.compare_u32(a, c, op).unwrap());
    }
    let selected = graph.select_u32(mask, a, c).unwrap();
    let sum = graph
        .reduce_u32(selected, ReduceOp::Sum, &[0, 1], false)
        .unwrap();
    outputs.extend([selected, sum]);
    let program = graph.compile(&outputs).unwrap();
    let av = [
        0,
        u32::MAX,
        1 << 31,
        (1 << 24) + 1,
        7,
        u32::MAX,
        0,
        (1 << 31) - 1,
        1 << 24,
        8,
    ];
    let cv = [u32::MAX, 0, 1 << 31, 1 << 24, 7];
    let masks = [0, 1, u32::MAX, 1 << 31, 0, 17, 0, 2, 0, 0];
    let a = b.upload_u32(shape(&[2, 5]), &av).unwrap();
    let c = b.upload_u32(shape(&[5]), &cv).unwrap();
    let mask = b.upload_u32(shape(&[2, 5]), &masks).unwrap();
    let out = program
        .run_typed(&[(&a).into(), (&c).into(), (&mask).into()])
        .unwrap();
    for (out, op) in out.iter().zip(ops) {
        let expected: Vec<_> = av
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                u32::from(match op {
                    CompareOp::Equal => x == cv[i % 5],
                    CompareOp::NotEqual => x != cv[i % 5],
                    CompareOp::Less => x < cv[i % 5],
                    CompareOp::LessEqual => x <= cv[i % 5],
                    CompareOp::Greater => x > cv[i % 5],
                    CompareOp::GreaterEqual => x >= cv[i % 5],
                })
            })
            .collect();
        assert_eq!(out.dtype(), MlxDtype::U32);
        assert_eq!(b.read_u32(out.as_tensor().unwrap()).unwrap(), expected);
    }
    let selected: Vec<_> = av
        .iter()
        .enumerate()
        .map(|(i, &x)| if masks[i] != 0 { x } else { cv[i % 5] })
        .collect();
    assert_eq!(b.read_u32(out[6].as_tensor().unwrap()).unwrap(), selected);
    assert_eq!(
        b.read_u32(out[7].as_tensor().unwrap()).unwrap(),
        [selected.iter().fold(0u32, |a, &x| a.wrapping_add(x))]
    );
}

#[test]
fn legacy_transport_rejects_low_and_unsigned_outputs_from_f32_inputs_before_trace() {
    let Some(b) = backend() else { return };
    for dtype in [Some(LowDtype::F16), Some(LowDtype::Bf16), None] {
        let mut graph = b.program();
        let x = graph.input(shape(&[3])).unwrap();
        let output = if let Some(dtype) = dtype {
            graph.cast_to_low(x, dtype).unwrap()
        } else {
            graph.compare(x, x, CompareOp::Equal).unwrap()
        };
        let program = graph.compile(&[output]).unwrap();
        let x = b.upload_f32(shape(&[3]), &[1., -0., 0.5]).unwrap();
        assert!(matches!(program.run(&[&x]), Err(MlxError::Dtype)));
        traces(&program, 0);
        let out = program.run_typed(&[(&x).into()]).unwrap();
        if let Some(dtype) = dtype {
            low_values(
                &b,
                &out[0],
                &[3],
                dtype,
                &[encode(dtype, 1.), 0x8000, encode(dtype, 0.5)],
            );
        } else {
            assert_eq!(out[0].dtype(), MlxDtype::U32);
            assert_eq!(b.read_u32(out[0].as_tensor().unwrap()).unwrap(), [1; 3]);
        }
        traces(&program, 1);
        assert!(matches!(program.run(&[&x]), Err(MlxError::Dtype)));
        traces(&program, 1);
    }
}
