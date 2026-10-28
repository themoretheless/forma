#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use half::{bf16, f16};
use tensor_core::{HasLowDtype, HasShape, LowDtype, LowStorage, Shape, TensorLowBackend};

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
fn encode(dtype: LowDtype, value: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(value).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(value).to_bits(),
    }
}
fn decode(dtype: LowDtype, bits: u16) -> f32 {
    match dtype {
        LowDtype::F16 => f16::from_bits(bits).to_f32(),
        LowDtype::Bf16 => bf16::from_bits(bits).to_f32(),
    }
}

#[test]
fn native_storage_and_strided_views_preserve_all_bit_patterns() {
    let Some(b) = backend() else { return };
    let bits: Vec<u16> = (0..=u16::MAX).collect();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let support = b.low_precision_support(dtype);
        assert_eq!(support.storage, LowStorage::Native16);
        assert!(support.matmul);
        assert!(support.matmul_f32);
        let input = b.upload_low(dtype, shape(&[256, 256]), &bits).unwrap();
        assert_eq!(input.low_dtype(), dtype);
        assert_eq!(b.read_low_bits(&input).unwrap(), bits);
        let transposed = b.permute_low(&input, &[1, 0]).unwrap();
        let expected: Vec<u16> = (0..256)
            .flat_map(|column| (0..256).map(move |row| (row * 256 + column) as u16))
            .collect();
        let materialized = b.materialize_low(&transposed).unwrap();
        assert_eq!(b.read_low_bits(&materialized).unwrap(), expected);
        let flat = b.reshape_low(&transposed, shape(&[65536])).unwrap();
        assert_eq!(b.read_low_bits(&flat).unwrap(), expected);
        let payload = b.upload_low(dtype, shape(&[]), &[0xff81]).unwrap();
        let broadcast = b.broadcast_low(&payload, shape(&[3, 5])).unwrap();
        assert_eq!(b.read_low_bits(&broadcast).unwrap(), vec![0xff81; 15]);
    }
}

#[test]
fn common_low_backend_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_low_backend(&b).unwrap();
}

#[test]
fn device_casts_match_rne_including_subnormals_and_special_values() {
    let Some(b) = backend() else { return };
    let mut values = vec![
        0.,
        -0.,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MIN_POSITIVE,
        -f32::MIN_POSITIVE,
        f32::from_bits(1),
        1. + 2.0_f32.powi(-11),
        1. + 3. * 2.0_f32.powi(-11),
        1. + 2.0_f32.powi(-8),
        1. + 3. * 2.0_f32.powi(-8),
        65504.,
        65520.,
        -65520.,
        2.0_f32.powi(-24),
        2.0_f32.powi(-25),
        f32::from_bits(0x0001_0000),
        f32::from_bits(0x0000_8000),
        f32::from_bits(0x0001_8000),
        f32::from_bits(0x8001_0000),
    ];
    let mut state = 17u32;
    for _ in 0..65536 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        values.push(f32::from_bits(state));
    }
    let input = b.upload_f32(shape(&[values.len()]), &values).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let low = b.cast_to_low(&input, dtype).unwrap();
        let actual = b.read_low_bits(&low).unwrap();
        for (i, (&value, &bits)) in values.iter().zip(&actual).enumerate() {
            if value.is_nan() {
                assert!(decode(dtype, bits).is_nan(), "{dtype:?} NaN at {i}");
            } else {
                assert_eq!(
                    bits,
                    encode(dtype, value),
                    "{dtype:?} cast at {i}, f32 bits={:08x}",
                    value.to_bits()
                );
            }
        }
        let all: Vec<u16> = (0..=u16::MAX).collect();
        let low = b.upload_low(dtype, shape(&[256, 256]), &all).unwrap();
        let low = b.permute_low(&low, &[1, 0]).unwrap();
        let widened = b.read_f32(&b.cast_to_f32(&low).unwrap()).unwrap();
        for (i, value) in widened.into_iter().enumerate() {
            let bits = all[(i % 256) * 256 + i / 256];
            let expected = decode(dtype, bits);
            if expected.is_nan() {
                assert!(value.is_nan(), "{dtype:?} widen {bits:04x}");
            } else {
                assert_eq!(
                    value.to_bits(),
                    expected.to_bits(),
                    "{dtype:?} widen {bits:04x}"
                );
            }
        }
    }
}

#[test]
fn low_matmul_accumulates_before_rounding_and_retains_native_output() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let tie = match dtype {
            LowDtype::F16 => 2.0_f32.powi(-11),
            LowDtype::Bf16 => 2.0_f32.powi(-8),
        };
        let left = b
            .upload_low(
                dtype,
                shape(&[3]),
                &[encode(dtype, 1.), encode(dtype, tie), encode(dtype, tie)],
            )
            .unwrap();
        let right = b
            .upload_low(dtype, shape(&[3]), &[encode(dtype, 1.); 3])
            .unwrap();
        let out = b.matmul_low(&left, &right).unwrap();
        assert_eq!(out.shape(), &shape(&[]));
        assert_eq!(out.low_dtype(), dtype);
        assert_eq!(
            b.read_low_bits(&out).unwrap(),
            [encode(dtype, 1. + 2. * tie)]
        );
        assert_eq!(
            b.read_f32(&b.matmul_low_f32(&left, &right).unwrap())
                .unwrap(),
            [1. + 2. * tie]
        );

        // Odd M/N/K reaches the tiled native matrix path with nonaligned tails.
        let (m, n, k) = (33, 35, 129);
        let a: Vec<f32> = (0..m * k).map(|i| (i % 7) as f32 * 0.125 - 0.25).collect();
        let weights: Vec<f32> = (0..n * k).map(|i| (i % 5) as f32 * 0.125 - 0.25).collect();
        let a = b
            .upload_low(
                dtype,
                shape(&[m, k]),
                &a.iter().map(|&v| encode(dtype, v)).collect::<Vec<_>>(),
            )
            .unwrap();
        let physical: Vec<u16> = weights.iter().map(|&v| encode(dtype, v)).collect();
        let w = b.upload_low(dtype, shape(&[n, k]), &physical).unwrap();
        let w = b.permute_low(&w, &[1, 0]).unwrap();
        let result = b.matmul_low(&a, &w).unwrap();
        let actual = b.read_low_bits(&result).unwrap();
        for row in 0..m {
            for col in 0..n {
                let sum: f32 = (0..k)
                    .map(|j| ((row * k + j) % 7) as f32 * 0.125 - 0.25)
                    .zip(&weights[col * k..(col + 1) * k])
                    .map(|(a, &w)| a * w)
                    .sum();
                assert_eq!(
                    actual[row * n + col],
                    encode(dtype, sum),
                    "{dtype:?} [{row},{col}]"
                );
            }
        }
    }
}

#[test]
fn low_empty_scalar_batch_and_validation_paths_are_explicit() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let empty = b.upload_low(dtype, shape(&[0]), &[]).unwrap();
        let zero = b.matmul_low(&empty, &empty).unwrap();
        assert_eq!(zero.shape(), &shape(&[]));
        assert_eq!(b.read_low_bits(&zero).unwrap(), [0]);
        let batch = b.upload_low(dtype, shape(&[0, 2, 3]), &[]).unwrap();
        let v = b
            .upload_low(dtype, shape(&[3]), &[encode(dtype, 1.); 3])
            .unwrap();
        let out = b.matmul_low(&batch, &v).unwrap();
        assert_eq!(out.shape(), &shape(&[0, 2]));
        assert!(b.read_low_bits(&out).unwrap().is_empty());
        let scalar = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 1.)])
            .unwrap();
        assert!(b.matmul_low(&scalar, &scalar).is_err());
        assert!(b.reshape_low(&scalar, shape(&[2])).is_err());
        assert!(b.permute_low(&v, &[1]).is_err());
        assert!(b.broadcast_low(&v, shape(&[2])).is_err());
        assert!(b.upload_low(dtype, shape(&[2]), &[0]).is_err());
        let u = b.upload_u32(shape(&[]), &[1]).unwrap();
        assert!(matches!(b.cast_to_low(&u, dtype), Err(MlxError::Dtype)));
    }
    let a = b.upload_low(LowDtype::F16, shape(&[1]), &[0x3c00]).unwrap();
    let other_type = b
        .upload_low(LowDtype::Bf16, shape(&[1]), &[0x3f80])
        .unwrap();
    assert!(matches!(
        b.matmul_low(&a, &other_type),
        Err(MlxError::Dtype)
    ));
    let foreign = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        foreign.read_low_bits(&a),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        foreign.cast_to_f32(&a),
        Err(MlxError::ForeignContext)
    ));
    assert!(matches!(
        foreign.materialize_low(&a),
        Err(MlxError::ForeignContext)
    ));
}
