#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{
    AttentionMask, AttentionOptions, HasShape, LowDtype, Shape, TensorError,
    TensorLowAttentionBackend, TensorLowBackend,
};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
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

#[test]
fn common_low_attention_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_low_attention_backend(&b).unwrap();
}

#[test]
fn strided_wide_values_and_skipped_tiles_keep_lazy_inputs_alive() {
    let Some(b) = backend() else { return };
    let (nq, nk, dv) = (3, 67, 263);
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let q = b
            .upload_low(dtype, shape(&[1, 4, nq, 1]), &[0; 12])
            .unwrap();
        let k = b
            .upload_low(dtype, shape(&[2, 2, nk, 1]), &vec![0; 4 * nk])
            .unwrap();
        let source: Vec<_> = (0..2)
            .flat_map(|batch| {
                (0..2).flat_map(move |head| {
                    (0..dv).flat_map(move |channel| {
                        (0..nk).map(move |key| {
                            (batch * 32 + head * 16 + channel % 11 + key % 7) as f32
                        })
                    })
                })
            })
            .collect();
        let v = b.upload_f32(shape(&[2, 2, dv, nk]), &source).unwrap();
        let v = b.cast_to_low(&v, dtype).unwrap();
        let v = b.permute_low(&v, &[0, 1, 3, 2]).unwrap();
        let flags: Vec<_> = (0..nk)
            .flat_map(|key| (0..nq).map(move |row| u32::from(key >= 32 && (key + row) % 3 != 0)))
            .collect();
        let mask = b.upload_u32(shape(&[nk, nq]), &flags).unwrap();
        let mask = b.permute(&mask, &[1, 0]).unwrap();
        let options = AttentionOptions::default();
        let result = b
            .attention_low_f32(&q, &k, &v, AttentionMask::Keep(&mask), options)
            .unwrap();
        let repeated = b
            .attention_low_f32(&q, &k, &v, AttentionMask::Keep(&mask), options)
            .unwrap();
        drop((q, k, v, mask));
        assert_eq!(result.shape(), &shape(&[2, 4, nq, dv]));
        let actual = b.read_f32(&result).unwrap();
        assert_eq!(actual, b.read_f32(&repeated).unwrap());
        for batch in 0..2 {
            for head in 0..4 {
                for row in 0..nq {
                    let keys: Vec<_> = (32..nk).filter(|&key| flags[key * nq + row] != 0).collect();
                    for channel in 0..dv {
                        let expected = keys
                            .iter()
                            .map(|&key| {
                                source[((batch * 2 + head / 2) * dv + channel) * nk + key] as f64
                            })
                            .sum::<f64>()
                            / keys.len() as f64;
                        let index = ((batch * 4 + head) * nq + row) * dv + channel;
                        assert!(
                            (f64::from(actual[index]) - expected).abs()
                                <= 2e-5 * expected.abs().max(1.)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn bounded_grid_reuses_groups_without_skipping_rows() {
    let Some(b) = backend() else { return };
    let rows = 65537;
    for (dtype, one) in [(LowDtype::F16, 0x3c00), (LowDtype::Bf16, 0x3f80)] {
        let scalar = b.upload_low(dtype, shape(&[1, 1]), &[one]).unwrap();
        let q = b.broadcast_low(&scalar, shape(&[rows, 1])).unwrap();
        let result = b
            .attention_low_f32(
                &q,
                &scalar,
                &scalar,
                AttentionMask::None,
                AttentionOptions::default(),
            )
            .unwrap();
        assert_eq!(result.shape(), &shape(&[rows, 1]));
        assert_eq!(b.read_f32(&result).unwrap(), vec![1.; rows]);
    }
}

#[test]
fn owner_and_mask_validation_precede_empty_shortcuts() {
    let Some(b) = backend() else { return };
    let q = b.upload_low(LowDtype::F16, shape(&[0, 1]), &[]).unwrap();
    let k = b
        .upload_low(LowDtype::F16, shape(&[2, 1]), &[0; 2])
        .unwrap();
    let v = b
        .upload_low(LowDtype::F16, shape(&[2, 1]), &[0; 2])
        .unwrap();
    let options = AttentionOptions::default();
    let other = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        other.attention_low_f32(&q, &k, &v, AttentionMask::None, options),
        Err(MlxError::ForeignContext)
    ));
    let foreign_mask = other.upload_u32(shape(&[]), &[1]).unwrap();
    assert!(matches!(
        b.attention_low_f32(&q, &k, &v, AttentionMask::Keep(&foreign_mask), options),
        Err(MlxError::ForeignContext)
    ));
    let float_mask = b.upload_f32(shape(&[]), &[1.]).unwrap();
    assert!(matches!(
        b.attention_low_f32(&q, &k, &v, AttentionMask::Keep(&float_mask), options),
        Err(MlxError::Dtype)
    ));
    let wrong = b
        .upload_low(LowDtype::Bf16, shape(&[2, 1]), &[0; 2])
        .unwrap();
    assert!(matches!(
        b.attention_low_f32(&q, &wrong, &v, AttentionMask::None, options),
        Err(MlxError::Contract(TensorError::LowDtypeMismatch { .. }))
    ));
}
