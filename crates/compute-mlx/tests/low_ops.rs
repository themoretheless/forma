#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use half::{bf16, f16};
use tensor_core::{
    BinaryOp, HasShape, LowDtype, ReduceOp, Shape, TensorLowBackend, TensorLowOpsBackend, UnaryOp,
};

fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu() {
        Ok(b) => Some(b),
        Err(e) => {
            eprintln!("MLX unavailable: {e}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{e}");
            None
        }
    }
}
fn encode(d: LowDtype, x: f32) -> u16 {
    match d {
        LowDtype::F16 => f16::from_f32(x).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(x).to_bits(),
    }
}
fn decode(d: LowDtype, b: u16) -> f32 {
    match d {
        LowDtype::F16 => f16::from_bits(b).to_f32(),
        LowDtype::Bf16 => bf16::from_bits(b).to_f32(),
    }
}
fn ordered(b: u16) -> u16 {
    if b & 0x8000 != 0 { !b } else { b ^ 0x8000 }
}

#[test]
fn custom_unary_preserves_every_finite_sign_bit_and_strided_storage() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let bits: Vec<u16> = (0..=u16::MAX).collect();
        let input = b.upload_low(dtype, shape(&[256, 256]), &bits).unwrap();
        let input = b.permute_low(&input, &[1, 0]).unwrap();
        for op in [UnaryOp::Negate, UnaryOp::Abs] {
            let out = b.unary_low(op, &input).unwrap();
            let actual = b.read_low_bits(&out).unwrap();
            for (i, &got) in actual.iter().enumerate() {
                let original = bits[(i % 256) * 256 + i / 256];
                if !decode(dtype, original).is_finite() {
                    continue;
                }
                let expected = if op == UnaryOp::Negate {
                    original ^ 0x8000
                } else {
                    original & 0x7fff
                };
                assert_eq!(got, expected, "{dtype:?} {op:?} {original:04x}");
            }
        }
    }
}

#[test]
fn custom_extrema_select_exact_subnormals_and_zero_signs() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let x = [0, 0x8000, 1, 0x8001, 2, 0x8002, 0x007f, 0x807f];
        let y = [0x8000, 0, 2, 0x8002, 1, 0x8001, 1, 0x8001];
        let a = b.upload_low(dtype, shape(&[2, 4]), &x).unwrap();
        let c = b.upload_low(dtype, shape(&[2, 4]), &y).unwrap();
        for (binary, reduce) in [
            (BinaryOp::Min, ReduceOp::Min),
            (BinaryOp::Max, ReduceOp::Max),
        ] {
            let got = b
                .read_low_bits(&b.binary_low(binary, &a, &c).unwrap())
                .unwrap();
            let expected: Vec<_> = x
                .iter()
                .zip(y)
                .map(|(&x, y)| {
                    if (ordered(x) < ordered(y)) == (binary == BinaryOp::Min) {
                        x
                    } else {
                        y
                    }
                })
                .collect();
            assert_eq!(got, expected, "{dtype:?} {binary:?}");
            let values = b.upload_low(dtype, shape(&[2]), &[1, 2]).unwrap();
            let f = b.reduce_low_f32(reduce, &values, &[0], false).unwrap();
            let expected = decode(dtype, if reduce == ReduceOp::Min { 1 } else { 2 });
            assert_eq!(b.read_f32(&f).unwrap()[0].to_bits(), expected.to_bits());
            let zeros = b.upload_low(dtype, shape(&[2]), &[0, 0x8000]).unwrap();
            let out = b.reduce_low(reduce, &zeros, &[0], false).unwrap();
            assert_eq!(
                b.read_low_bits(&out).unwrap(),
                [if reduce == ReduceOp::Min { 0x8000 } else { 0 }]
            );
        }
    }
}

#[test]
fn custom_arithmetic_evaluates_once_in_f32_with_broadcast_and_cache_reuse() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let values = [0.25, 0.5, 1.0, 1.5, 2.0, 3.0];
        let a = b
            .upload_low(dtype, shape(&[2, 3]), &values.map(|x| encode(dtype, x)))
            .unwrap();
        let a = b.permute_low(&a, &[1, 0]).unwrap();
        for op in [
            UnaryOp::Square,
            UnaryOp::Sqrt,
            UnaryOp::Reciprocal,
            UnaryOp::Exp,
            UnaryOp::Log,
            UnaryOp::Sin,
            UnaryOp::Cos,
        ] {
            let output = b.unary_low(op, &a).unwrap();
            for (i, got) in b.read_low_bits(&output).unwrap().into_iter().enumerate() {
                let x = values[(i % 2) * 3 + i / 2];
                let expected = match op {
                    UnaryOp::Square => x * x,
                    UnaryOp::Sqrt => x.sqrt(),
                    UnaryOp::Reciprocal => x.recip(),
                    UnaryOp::Exp => x.exp(),
                    UnaryOp::Log => x.ln(),
                    UnaryOp::Sin => x.sin(),
                    _ => x.cos(),
                };
                assert_eq!(got, encode(dtype, expected), "{dtype:?} {op:?} {x}");
            }
        }
        let scalar = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 0.5)])
            .unwrap();
        for _ in 0..2 {
            for op in [
                BinaryOp::Add,
                BinaryOp::Subtract,
                BinaryOp::Multiply,
                BinaryOp::Divide,
            ] {
                let output = b.binary_low(op, &a, &scalar).unwrap();
                let got = b.read_low_bits(&output).unwrap();
                for (i, got) in got.into_iter().enumerate() {
                    let x = values[(i % 2) * 3 + i / 2];
                    let expected = match op {
                        BinaryOp::Add => x + 0.5,
                        BinaryOp::Subtract => x - 0.5,
                        BinaryOp::Multiply => x * 0.5,
                        _ => x / 0.5,
                    };
                    assert_eq!(got, encode(dtype, expected), "{dtype:?} {op:?}");
                }
            }
        }
    }
}

#[test]
fn custom_hierarchy_has_f32_partials_and_keeps_lazy_inputs_alive() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let small = if dtype == LowDtype::F16 {
            2f32.powi(-11)
        } else {
            2f32.powi(-8)
        };
        let a = b
            .upload_low(
                dtype,
                shape(&[2]),
                &[encode(dtype, 1.), encode(dtype, small)],
            )
            .unwrap();
        let sum = b.reduce_low_f32(ReduceOp::Sum, &a, &[0], false).unwrap();
        assert_eq!(b.read_f32(&sum).unwrap(), [1. + small]);
        let mean = b.mean_low_f32(&a, &[0], false).unwrap();
        assert_eq!(b.read_f32(&mean).unwrap(), [(1. + small) / 2.]);
        let n = 131077;
        let bits: Vec<_> = (0..n * 3)
            .map(|i| encode(dtype, if i % 7 == 0 { 0.5 } else { 0.125 }))
            .collect();
        let physical = b.upload_low(dtype, shape(&[n, 3]), &bits).unwrap();
        let transposed = b.permute_low(&physical, &[1, 0]).unwrap();
        let result = b
            .reduce_low_f32(ReduceOp::Sum, &transposed, &[1], true)
            .unwrap();
        let means = b.mean_low_f32(&transposed, &[1], false).unwrap();
        drop(physical);
        drop(transposed);
        assert_eq!(result.shape(), &shape(&[3, 1]));
        let got = b.read_f32(&result).unwrap();
        let gotmean = b.read_f32(&means).unwrap();
        for row in 0..3 {
            let expected: f32 = (0..n).map(|i| decode(dtype, bits[i * 3 + row])).sum();
            assert_eq!(got[row], expected);
            assert_eq!(gotmean[row], expected / n as f32);
        }
        // Reduction grid is capped: more than 65535 rows reuse each workgroup.
        let scalar = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 0.5)])
            .unwrap();
        let view = b.broadcast_low(&scalar, shape(&[65537, 2])).unwrap();
        let sum = b.reduce_low_f32(ReduceOp::Sum, &view, &[1], false).unwrap();
        assert_eq!(b.read_f32(&sum).unwrap(), vec![1.; 65537]);
    }
}

#[test]
fn custom_low_validation_empty_axes_and_empty_contractions() {
    let Some(b) = backend() else { return };
    let a = b.upload_low(LowDtype::F16, shape(&[2, 0, 3]), &[]).unwrap();
    for op in [ReduceOp::Sum, ReduceOp::Product] {
        let out = b.reduce_low_f32(op, &a, &[1], true).unwrap();
        assert_eq!(out.shape(), &shape(&[2, 1, 3]));
        assert_eq!(
            b.read_f32(&out).unwrap(),
            vec![if op == ReduceOp::Sum { 0. } else { 1. }; 6]
        );
    }
    assert!(b.reduce_low_f32(ReduceOp::Min, &a, &[1], false).is_err());
    assert!(b.mean_low_f32(&a, &[1], false).is_err());
    assert!(b.mean_low_f32(&a, &[3], false).is_err());
    assert!(b.reduce_low_f32(ReduceOp::Sum, &a, &[0, 0], false).is_err());
    assert!(
        b.read_f32(&b.mean_low_f32(&a, &[0], false).unwrap())
            .unwrap()
            .is_empty()
    );
    let scalar = b.upload_low(LowDtype::Bf16, shape(&[]), &[0x8001]).unwrap();
    let preserved = b
        .reduce_low_f32(ReduceOp::Sum, &scalar, &[], false)
        .unwrap();
    assert_eq!(b.read_f32(&preserved).unwrap()[0].to_bits(), 0x80010000);
    let half = b.upload_low(LowDtype::F16, shape(&[]), &[0]).unwrap();
    assert!(b.binary_low(BinaryOp::Add, &half, &scalar).is_err());
    let foreign = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        foreign.unary_low(UnaryOp::Negate, &scalar),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        foreign.reduce_low_f32(ReduceOp::Min, &scalar, &[], false),
        Err(MlxError::ForeignContext)
    ));
}

#[test]
fn common_low_ops_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_low_ops_backend(&b).unwrap();
}
