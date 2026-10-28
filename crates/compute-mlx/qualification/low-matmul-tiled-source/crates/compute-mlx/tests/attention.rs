#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use tensor_core::{AttentionMask, AttentionOptions, HasShape, Shape, TensorAttentionBackend};

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
fn close(actual: f32, expected: f64) {
    assert!(
        actual.is_finite() && (f64::from(actual) - expected).abs() <= 6e-5 * expected.abs().max(1.),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn common_attention_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_attention_backend(&b).unwrap();
}

#[test]
fn constant_extreme_values_keep_finite_convex_outputs() {
    let Some(b) = backend() else { return };
    for nq in [1, 17] {
        let q = b.upload_f32(shape(&[nq, 64]), &vec![0.; nq * 64]).unwrap();
        let k = b.upload_f32(shape(&[4, 64]), &vec![0.; 4 * 64]).unwrap();
        let v = b
            .upload_f32(shape(&[4, 64]), &vec![f32::MAX; 4 * 64])
            .unwrap();
        let out = b
            .attention(&q, &k, &v, AttentionMask::None, AttentionOptions::default())
            .unwrap();
        for x in b.read_f32(&out).unwrap() {
            close(x, f64::from(f32::MAX));
        }
    }
}

#[test]
fn masked_rows_are_zero_across_native_fallback_vector_full_and_two_pass_shapes() {
    let Some(b) = backend() else { return };
    // MLX0.32.1 uses its generic graph for D3, vector for Lq<=8, full for
    // Lq>8, and a two-pass vector path for long keys on M4 Max.
    for (d, dv, nq, nk, hq, hk) in [
        (3, 5, 4, 3, 1, 1),
        (64, 64, 4, 4, 1, 1),
        (64, 64, 16, 8, 1, 1),
        (192, 128, 2, 4, 4, 2),
        (128, 128, 2, 1031, 4, 2),
    ] {
        let q = b
            .upload_f32(shape(&[1, hq, nq, d]), &vec![0.; hq * nq * d])
            .unwrap();
        let k = b
            .upload_f32(shape(&[1, hk, nk, d]), &vec![0.; hk * nk * d])
            .unwrap();
        let values: Vec<f32> = (0..hk)
            .flat_map(|h| {
                (0..nk)
                    .flat_map(move |j| (0..dv).map(move |_| 10. * h as f32 + (j % 7) as f32 + 1.))
            })
            .collect();
        let v = b.upload_f32(shape(&[1, hk, nk, dv]), &values).unwrap();
        let flags: Vec<u32> = (0..nq)
            .flat_map(|i| {
                (0..nk).map(move |j| {
                    if i % 2 == 1 && j % 3 != 1 {
                        u32::MAX
                    } else {
                        0
                    }
                })
            })
            .collect();
        let biases: Vec<f32> = flags
            .iter()
            .enumerate()
            .map(|(i, &f)| {
                if f == 0 {
                    f32::NEG_INFINITY
                } else {
                    (i % nk % 5) as f32 * -0.5
                }
            })
            .collect();
        let keep = b.upload_u32(shape(&[nq, nk]), &flags).unwrap();
        let additive = b.upload_f32(shape(&[nq, nk]), &biases).unwrap();
        for (mask, biased) in [
            (AttentionMask::Keep(&keep), false),
            (AttentionMask::Additive(&additive), true),
        ] {
            let out = b
                .attention(&q, &k, &v, mask, AttentionOptions::default())
                .unwrap();
            let out = b.read_f32(&out).unwrap();
            for head in 0..hq {
                for row in 0..nq {
                    let mut sum = 0.;
                    let mut numerator = 0.;
                    for key in 0..nk {
                        if flags[row * nk + key] != 0 {
                            let w = if biased {
                                f64::from(biases[row * nk + key]).exp()
                            } else {
                                1.
                            };
                            sum += w;
                            numerator +=
                                w * f64::from(values[((head / (hq / hk)) * nk + key) * dv]);
                        }
                    }
                    let expected = if sum == 0. { 0. } else { numerator / sum };
                    for col in 0..dv {
                        close(out[(head * nq + row) * dv + col], expected);
                    }
                }
            }
        }
    }
}

#[test]
fn causal_offsets_cover_native_alignment_explicit_masks_and_signed_extremes() {
    let Some(b) = backend() else { return };
    for (nq, nk, depth) in [(4, 2, 3), (2, 4, 64), (16, 8, 64)] {
        let q = b
            .upload_f32(shape(&[nq, depth]), &vec![0.; nq * depth])
            .unwrap();
        let k = b
            .upload_f32(shape(&[nk, depth]), &vec![0.; nk * depth])
            .unwrap();
        let values: Vec<f32> = (0..nk)
            .flat_map(|j| std::iter::repeat_n(j as f32 + 1., depth))
            .collect();
        let v = b.upload_f32(shape(&[nk, depth]), &values).unwrap();
        for offset in [0, -1, nk as i32 - nq as i32, i32::MIN, i32::MAX] {
            let out = b
                .attention(
                    &q,
                    &k,
                    &v,
                    AttentionMask::None,
                    AttentionOptions {
                        scale: None,
                        causal: Some(offset),
                    },
                )
                .unwrap();
            assert_eq!(out.shape(), &shape(&[nq, depth]));
            let out = b.read_f32(&out).unwrap();
            for row in 0..nq {
                let count = (row as i64 + i64::from(offset) + 1).clamp(0, nk as i64);
                let expected = if count == 0 {
                    0.
                } else {
                    (count as f64 + 1.) / 2.
                };
                for col in 0..depth {
                    close(out[row * depth + col], expected);
                }
            }
        }
    }
}

#[test]
fn large_scales_apply_after_dot_without_overflowing_finite_query_values() {
    let Some(b) = backend() else { return };
    let q = b.upload_f32(shape(&[2, 64]), &vec![f32::MAX; 128]).unwrap();
    let k = b.upload_f32(shape(&[3, 64]), &vec![0.; 192]).unwrap();
    let v = b
        .upload_f32(shape(&[3, 2]), &[1., 2., 3., 4., 5., 6.])
        .unwrap();
    for scale in [0., 2., -2., f32::MAX, -f32::MAX] {
        let out = b
            .attention(
                &q,
                &k,
                &v,
                AttentionMask::None,
                AttentionOptions {
                    scale: Some(scale),
                    causal: None,
                },
            )
            .unwrap();
        assert_eq!(b.read_f32(&out).unwrap(), [3., 4., 3., 4.]);
    }
    let additive = b
        .upload_f32(
            shape(&[2, 3]),
            &[
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                0.,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
        )
        .unwrap();
    let out = b
        .attention(
            &q,
            &k,
            &v,
            AttentionMask::Additive(&additive),
            AttentionOptions {
                scale: Some(2.),
                causal: Some(0),
            },
        )
        .unwrap();
    assert_eq!(b.read_f32(&out).unwrap(), [0., 0., 1., 2.]);
}

#[test]
fn strided_batch_broadcast_and_grouped_heads_keep_lazy_graph_inputs_alive() {
    let Some(b) = backend() else { return };
    let output = {
        let q = b
            .upload_f32(shape(&[2, 1, 4, 64, 3]), &vec![0.; 2 * 4 * 64 * 3])
            .unwrap();
        let q = b.permute(&q, &[0, 1, 2, 4, 3]).unwrap();
        let k = b
            .upload_f32(shape(&[1, 3, 2, 64, 4]), &vec![0.; 3 * 2 * 64 * 4])
            .unwrap();
        let k = b.permute(&k, &[0, 1, 2, 4, 3]).unwrap();
        let values: Vec<f32> = (0..3)
            .flat_map(|batch| {
                (0..2).flat_map(move |h| {
                    (0..4).flat_map(move |j| {
                        std::iter::repeat_n((100 * batch + 10 * h + j + 1) as f32, 64)
                    })
                })
            })
            .collect();
        let v = b.upload_f32(shape(&[1, 3, 2, 4, 64]), &values).unwrap();
        // Logical 3x4 mask comes from a transpose: row1 is fully excluded.
        let mask = b
            .upload_u32(shape(&[4, 3]), &[1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 1])
            .unwrap();
        let mask = b.permute(&mask, &[1, 0]).unwrap();
        b.attention(
            &q,
            &k,
            &v,
            AttentionMask::Keep(&mask),
            AttentionOptions::default(),
        )
        .unwrap()
    };
    assert_eq!(output.shape(), &shape(&[2, 3, 4, 3, 64]));
    let values = b.read_f32(&output).unwrap();
    for outer in 0..2 {
        for batch in 0..3 {
            for h in 0..4 {
                for row in 0..3 {
                    let expected = if row == 1 {
                        0.
                    } else {
                        (100 * batch + 10 * (h / 2)) as f64 + if row == 0 { 2. } else { 3. }
                    };
                    for col in 0..64 {
                        close(
                            values[((((outer * 3 + batch) * 4 + h) * 3 + row) * 64) + col],
                            expected,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn empty_shapes_dtype_and_owner_errors_are_checked_before_native_attention() {
    let Some(b) = backend() else { return };
    let q = b.upload_f32(shape(&[2, 3]), &[0.; 6]).unwrap();
    let k = b.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let v = b.upload_f32(shape(&[0, 5]), &[]).unwrap();
    let out = b
        .attention(&q, &k, &v, AttentionMask::None, AttentionOptions::default())
        .unwrap();
    assert_eq!(b.read_f32(&out).unwrap(), [0.; 10]);
    let empty_q = b.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let empty = b
        .attention(
            &empty_q,
            &k,
            &v,
            AttentionMask::None,
            AttentionOptions::default(),
        )
        .unwrap();
    assert_eq!(empty.shape(), &shape(&[0, 5]));
    let wrong = b.upload_u32(shape(&[2, 3]), &[0; 6]).unwrap();
    assert!(matches!(
        b.attention(
            &wrong,
            &k,
            &v,
            AttentionMask::None,
            AttentionOptions::default()
        ),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        b.attention(
            &q,
            &k,
            &v,
            AttentionMask::Keep(&q),
            AttentionOptions::default()
        ),
        Err(MlxError::Dtype)
    ));
    let foreign = MlxBackend::new_gpu().unwrap();
    let foreign_mask = foreign.upload_u32(shape(&[2, 0]), &[]).unwrap();
    assert!(matches!(
        b.attention(
            &q,
            &k,
            &v,
            AttentionMask::Keep(&foreign_mask),
            AttentionOptions::default()
        ),
        Err(MlxError::ForeignContext)
    ));
    for scale in [f32::NAN, f32::INFINITY] {
        assert!(
            b.attention(
                &empty_q,
                &k,
                &v,
                AttentionMask::None,
                AttentionOptions {
                    scale: Some(scale),
                    causal: None
                }
            )
            .is_err()
        );
    }
}
