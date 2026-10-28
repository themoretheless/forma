#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use half::{bf16, f16};
use tensor_core::{
    HasShape, LowDtype, ScatterOp, Shape, TensorLowBackend, TensorLowScatterBackend,
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
fn encode(dtype: LowDtype, x: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(x).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(x).to_bits(),
    }
}

#[test]
fn common_low_scatter_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_low_scatter_backend(&b).unwrap();
}

#[test]
fn lists_handle_contention_strides_broadcast_and_fresh_repeated_calls() {
    let Some(b) = backend() else { return };
    let n = 65539;
    let values: Vec<u32> = (0..n)
        .map(|i| {
            if i % 11 == 0 {
                u32::MAX
            } else {
                (i % 17) as u32
            }
        })
        .collect();
    let mut counts = [0usize; 17];
    let mut invalid = 0;
    for &i in &values {
        if i < 17 {
            counts[i as usize] += 1;
        } else {
            invalid += 1;
        }
    }
    let indices = b.upload_u32(shape(&[n]), &values).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let physical = b
            .upload_low(
                dtype,
                shape(&[17, 2, 3]),
                &vec![encode(dtype, 0.5); 17 * 2 * 3],
            )
            .unwrap();
        let base = b.permute_low(&physical, &[2, 0, 1]).unwrap();
        let updates = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 0.125)])
            .unwrap();
        let expected: Vec<f32> = (0..3)
            .flat_map(|_| counts.iter().flat_map(|&n| [0.5 + n as f32 * 0.125; 2]))
            .collect();
        for _ in 0..3 {
            let out = b
                .scatter_low_f32(ScatterOp::Add, &base, &indices, &updates, 1)
                .unwrap();
            assert_eq!(out.values.shape(), base.shape());
            assert_eq!(b.read_f32(&out.values).unwrap(), expected);
            assert_eq!(b.read_u32(&out.invalid_count).unwrap(), [invalid]);
            let out = b
                .scatter_low(ScatterOp::Add, &base, &indices, &updates, 1)
                .unwrap();
            assert_eq!(
                b.read_low_bits(&out.values).unwrap(),
                expected
                    .iter()
                    .map(|&x| encode(dtype, x))
                    .collect::<Vec<_>>()
            );
        }
        // New graphs must not see head/next values from a cached kernel's
        // previous invocation. Every current index is invalid, so values stay.
        let bad = b.upload_u32(shape(&[3]), &[17, u32::MAX, 100]).unwrap();
        let out = b
            .scatter_low_f32(ScatterOp::Add, &base, &bad, &updates, 1)
            .unwrap();
        assert_eq!(b.read_f32(&out.values).unwrap(), vec![0.5; 17 * 2 * 3]);
        assert_eq!(b.read_u32(&out.invalid_count).unwrap(), [3]);
        assert_eq!(
            b.read_low_bits(&base).unwrap(),
            vec![encode(dtype, 0.5); 17 * 2 * 3]
        );
        // Deferred graphs retain both strided source arrays and linked-list
        // storage until evaluation even after user-facing input handles drop.
        let result = b
            .scatter_low_f32(ScatterOp::Add, &base, &indices, &updates, 1)
            .unwrap();
        drop(physical);
        drop(base);
        drop(updates);
        assert_eq!(b.read_f32(&result.values).unwrap(), expected);
    }
}

#[test]
fn low_scatter_validates_before_empty_execution_and_keeps_counts_resident() {
    let Some(b) = backend() else { return };
    let base = b.upload_low(LowDtype::Bf16, shape(&[0, 2]), &[]).unwrap();
    let updates = b.upload_low(LowDtype::Bf16, shape(&[]), &[1]).unwrap();
    let indices = b.upload_u32(shape(&[3]), &[0, 2, u32::MAX]).unwrap();
    for op in [
        ScatterOp::Replace,
        ScatterOp::Add,
        ScatterOp::Multiply,
        ScatterOp::Min,
        ScatterOp::Max,
    ] {
        let out = b.scatter_low(op, &base, &indices, &updates, 1).unwrap();
        assert!(b.read_low_bits(&out.values).unwrap().is_empty());
        assert_eq!(b.read_u32(&out.invalid_count).unwrap(), [2]);
        let out = b.scatter_low_f32(op, &base, &indices, &updates, 0).unwrap();
        assert!(b.read_f32(&out.values).unwrap().is_empty());
        assert_eq!(b.read_u32(&out.invalid_count).unwrap(), [3]);
    }
    assert!(
        b.scatter_low(ScatterOp::Add, &base, &indices, &updates, 2)
            .is_err()
    );
    let mixed = b.upload_low(LowDtype::F16, shape(&[]), &[1]).unwrap();
    assert!(
        b.scatter_low(ScatterOp::Replace, &base, &indices, &mixed, 1)
            .is_err()
    );
    let wrong = b.upload_f32(shape(&[1]), &[0.]).unwrap();
    assert!(matches!(
        b.scatter_low(ScatterOp::Add, &base, &wrong, &updates, 1),
        Err(MlxError::Dtype)
    ));
    let foreign = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        foreign.scatter_low(ScatterOp::Add, &base, &indices, &updates, 1),
        Err(MlxError::ForeignContext)
    ));
}
