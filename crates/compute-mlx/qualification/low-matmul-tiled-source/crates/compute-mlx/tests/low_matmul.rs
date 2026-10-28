#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use half::{bf16, f16};
use tensor_core::{HasShape, LowDtype, Shape, TensorLowBackend};

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

#[test]
fn f32_output_retains_extra_bits_and_accumulation_cancellation() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        assert!(b.low_precision_support(dtype).matmul_f32);
        let tie = if dtype == LowDtype::F16 {
            2f32.powi(-11)
        } else {
            2f32.powi(-8)
        };
        let a = b
            .upload_low(dtype, shape(&[2]), &[encode(dtype, 1.), encode(dtype, tie)])
            .unwrap();
        let ones = b
            .upload_low(dtype, shape(&[2]), &[encode(dtype, 1.); 2])
            .unwrap();
        let result = b.matmul_low_f32(&a, &ones).unwrap();
        assert_eq!(result.shape(), &shape(&[]));
        assert_eq!(b.read_f32(&result).unwrap(), [1. + tie]);
        assert_ne!(
            1. + tie,
            if dtype == LowDtype::F16 {
                f16::from_f32(1. + tie).to_f32()
            } else {
                bf16::from_f32(1. + tie).to_f32()
            }
        );
        let big = if dtype == LowDtype::F16 { 2048. } else { 256. };
        let left: Vec<_> = [big, 1., -big]
            .into_iter()
            .map(|x| encode(dtype, x))
            .collect();
        let left = b.upload_low(dtype, shape(&[1, 3]), &left).unwrap();
        let right = b
            .upload_low(dtype, shape(&[3, 1]), &[encode(dtype, 1.); 3])
            .unwrap();
        assert_eq!(
            b.read_f32(&b.matmul_low_f32(&left, &right).unwrap())
                .unwrap(),
            [1.]
        );
    }
}

#[test]
fn broadcast_transposed_partial_tiles_preserve_values_and_lazy_lifetime() {
    let Some(b) = backend() else { return };
    let (m, n, k) = (17, 19, 35);
    let left_values: Vec<_> = (0..2 * k * m)
        .map(|i| (i % 17) as f32 * 0.125 - 1.)
        .collect();
    let right_values: Vec<_> = (0..3 * n * k)
        .map(|i| (i % 13) as f32 * 0.25 - 1.)
        .collect();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let left_bits: Vec<_> = left_values.iter().map(|&x| encode(dtype, x)).collect();
        let right_bits: Vec<_> = right_values.iter().map(|&x| encode(dtype, x)).collect();
        let left_source = b
            .upload_low(dtype, shape(&[2, 1, k, m]), &left_bits)
            .unwrap();
        let right_source = b.upload_low(dtype, shape(&[3, n, k]), &right_bits).unwrap();
        let left = b.permute_low(&left_source, &[0, 1, 3, 2]).unwrap();
        let right = b.permute_low(&right_source, &[0, 2, 1]).unwrap();
        let result = b.matmul_low_f32(&left, &right).unwrap();
        let repeated = b.matmul_low_f32(&left, &right).unwrap();
        assert_eq!(b.read_low_bits(&left_source).unwrap(), left_bits);
        assert_eq!(b.read_low_bits(&right_source).unwrap(), right_bits);
        drop((left, right, left_source, right_source));
        assert_eq!(result.shape(), &shape(&[2, 3, m, n]));
        let actual = b.read_f32(&result).unwrap();
        assert_eq!(actual, b.read_f32(&repeated).unwrap());
        for ba in 0..2 {
            for bb in 0..3 {
                for row in 0..m {
                    for col in 0..n {
                        let expected = (0..k)
                            .map(|inner| {
                                f64::from(left_values[(ba * k + inner) * m + row])
                                    * f64::from(right_values[(bb * n + col) * k + inner])
                            })
                            .sum::<f64>() as f32;
                        let index = ((ba * 3 + bb) * m + row) * n + col;
                        assert_eq!(
                            actual[index], expected,
                            "{dtype:?} batch[{ba},{bb}] [{row},{col}]"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn tiny_bf16_operands_form_normal_products_in_both_orders() {
    let Some(b) = backend() else { return };
    let small = [1u16, 0x007f, 0x0080, 0x8001, 0x807f, 0x8080];
    let a = b
        .upload_low(LowDtype::Bf16, shape(&[small.len(), 1]), &small)
        .unwrap();
    let big = b
        .upload_low(LowDtype::Bf16, shape(&[1, 1]), &[0x7f7f])
        .unwrap();
    for reverse in [false, true] {
        let result = if reverse {
            let row = b.permute_low(&a, &[1, 0]).unwrap();
            b.matmul_low_f32(&big, &row).unwrap()
        } else {
            b.matmul_low_f32(&a, &big).unwrap()
        };
        let actual = b.read_f32(&result).unwrap();
        for (&value, bits) in actual.iter().zip(small) {
            let expected = (f64::from(bf16::from_bits(bits).to_f32())
                * f64::from(bf16::from_bits(0x7f7f).to_f32())) as f32;
            assert!(expected.is_normal());
            assert_eq!(value, expected, "reverse={reverse}, bits={bits:04x}");
        }
    }
}

#[test]
fn bounded_tile_grid_reuses_groups_and_zero_strides() {
    let Some(b) = backend() else { return };
    let batches = 65537;
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let values: Vec<_> = (0..batches).map(|i| (i % 31) as f32 * 0.25).collect();
        let bits: Vec<_> = values.iter().map(|&x| encode(dtype, x)).collect();
        let left = b.upload_low(dtype, shape(&[batches, 1, 1]), &bits).unwrap();
        let left = b.broadcast_low(&left, shape(&[batches, 1, 3])).unwrap();
        let scalar = b
            .upload_low(dtype, shape(&[1]), &[encode(dtype, 1.)])
            .unwrap();
        let right = b.broadcast_low(&scalar, shape(&[3])).unwrap();
        let result = b.matmul_low_f32(&left, &right).unwrap();
        assert_eq!(result.shape(), &shape(&[batches, 1]));
        assert_eq!(
            b.read_f32(&result).unwrap(),
            values.into_iter().map(|x| 3. * x).collect::<Vec<_>>()
        );
    }
}

#[test]
fn empty_products_and_preallocation_validation_follow_common_contract() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let empty = b.upload_low(dtype, shape(&[0]), &[]).unwrap();
        let result = b.matmul_low_f32(&empty, &empty).unwrap();
        assert_eq!(result.shape(), &shape(&[]));
        assert_eq!(b.read_f32(&result).unwrap(), [0.]);
        let left = b.upload_low(dtype, shape(&[2, 3, 0]), &[]).unwrap();
        let right = b.upload_low(dtype, shape(&[0, 5]), &[]).unwrap();
        let result = b.matmul_low_f32(&left, &right).unwrap();
        assert_eq!(result.shape(), &shape(&[2, 3, 5]));
        assert_eq!(b.read_f32(&result).unwrap(), vec![0.; 30]);
        let left = b.upload_low(dtype, shape(&[0, 2, 3]), &[]).unwrap();
        let right = b
            .upload_low(dtype, shape(&[3]), &[encode(dtype, 1.); 3])
            .unwrap();
        let result = b.matmul_low_f32(&left, &right).unwrap();
        assert_eq!(result.shape(), &shape(&[0, 2]));
        assert!(b.read_f32(&result).unwrap().is_empty());
        let scalar = b.upload_low(dtype, shape(&[]), &[0]).unwrap();
        assert!(b.matmul_low_f32(&scalar, &scalar).is_err());
        assert!(b.matmul_low_f32(&left, &empty).is_err());
    }
    let left = b.upload_low(LowDtype::F16, shape(&[0, 1]), &[]).unwrap();
    let right = b.upload_low(LowDtype::Bf16, shape(&[1, 0]), &[]).unwrap();
    assert!(matches!(
        b.matmul_low_f32(&left, &right),
        Err(MlxError::Dtype)
    ));
    let other = MlxBackend::new_gpu().unwrap();
    assert!(matches!(
        other.matmul_low_f32(&left, &right),
        Err(MlxError::ForeignContext)
    ));
}
