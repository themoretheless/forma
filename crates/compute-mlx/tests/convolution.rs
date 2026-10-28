#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{MlxBackend, MlxError};
use half::{bf16, f16};
use tensor_core::{
    ConvOptions, HasShape, LowDtype, Shape, TensorConvBackend, TensorError, TensorLowBackend,
    TensorLowConvBackend,
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
fn encode(dtype: LowDtype, value: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(value).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(value).to_bits(),
    }
}

#[test]
fn common_convolution_conformance() {
    let Some(b) = backend() else { return };
    tensor_core::conformance::check_conv_backend(&b).unwrap();
    tensor_core::conformance::check_low_conv_backend(&b).unwrap();
}

#[test]
fn direct_low_contraction_keeps_f32_bits_and_tiny_products() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let tie = if dtype == LowDtype::F16 {
            2f32.powi(-11)
        } else {
            2f32.powi(-8)
        };
        let k = 257;
        let mut values = vec![encode(dtype, 0.); k];
        values[0] = encode(dtype, 1.);
        values[k - 1] = encode(dtype, tie);
        let input = b.upload_low(dtype, shape(&[1, 1, k]), &values).unwrap();
        let weight = b
            .upload_low(dtype, shape(&[1, 1, k]), &vec![encode(dtype, 1.); k])
            .unwrap();
        let out = b
            .conv_low_f32(&input, &weight, &ConvOptions::new(1))
            .unwrap();
        let low = b.conv_low(&input, &weight, &ConvOptions::new(1)).unwrap();
        drop((input, weight));
        assert_eq!(out.shape(), &shape(&[1, 1, 1]));
        assert_eq!(b.read_f32(&out).unwrap(), [1. + tie]);
        assert_eq!(b.read_low_bits(&low).unwrap(), [encode(dtype, 1.)]);
    }
    for (tiny, normal) in [(1, 0x7f7f), (0x8001, 0x7f7f), (0x007f, 0xff7f)] {
        for reverse in [false, true] {
            let (x, w) = if reverse {
                (normal, tiny)
            } else {
                (tiny, normal)
            };
            let input = b
                .upload_low(LowDtype::Bf16, shape(&[1, 1, 1]), &[x])
                .unwrap();
            let weight = b
                .upload_low(LowDtype::Bf16, shape(&[1, 1, 1]), &[w])
                .unwrap();
            let expected = (f64::from(bf16::from_bits(x).to_f32())
                * f64::from(bf16::from_bits(w).to_f32())) as f32;
            assert!(expected.is_normal());
            let out = b
                .conv_low_f32(&input, &weight, &ConvOptions::new(1))
                .unwrap();
            assert_eq!(b.read_f32(&out).unwrap(), [expected]);
        }
    }
}

#[test]
fn bounded_grid_reuses_shared_reduction_and_zero_stride_views() {
    let Some(b) = backend() else { return };
    let count = 65537;
    let values: Vec<_> = (0..count).map(|i| (i % 31) as f32 * 0.25).collect();
    let input_base = b.upload_f32(shape(&[1, 1, count]), &values).unwrap();
    let input = b.broadcast_to(&input_base, shape(&[1, 3, count])).unwrap();
    let weights_base = b.upload_f32(shape(&[1, 1, 1]), &[0.5]).unwrap();
    let weights = b.broadcast_to(&weights_base, shape(&[1, 3, 1])).unwrap();
    let expected: Vec<_> = values.iter().map(|x| x * 1.5).collect();
    for dtype in [None, Some(LowDtype::F16), Some(LowDtype::Bf16)] {
        let result = if let Some(dtype) = dtype {
            let x = b.cast_to_low(&input_base, dtype).unwrap();
            let w = b.cast_to_low(&weights_base, dtype).unwrap();
            let x = b.broadcast_low(&x, shape(&[1, 3, count])).unwrap();
            let w = b.broadcast_low(&w, shape(&[1, 3, 1])).unwrap();
            b.conv_low_f32(&x, &w, &ConvOptions::new(1)).unwrap()
        } else {
            b.conv(&input, &weights, &ConvOptions::new(1)).unwrap()
        };
        assert_eq!(b.read_f32(&result).unwrap(), expected);
    }
}

#[test]
fn unsigned_geometry_and_empty_padding_windows_are_checked() {
    let Some(b) = backend() else { return };
    let input = b.upload_f32(shape(&[1, 1, 2]), &[3., 4.]).unwrap();
    let weight = b.upload_f32(shape(&[1, 1, 1]), &[2.]).unwrap();
    let mut options = ConvOptions::new(1);
    options.strides[0] = usize::MAX;
    options.dilations[0] = usize::MAX;
    let result = b.conv(&input, &weight, &options).unwrap();
    assert_eq!(b.read_f32(&result).unwrap(), [6.]);
    options.padding_before[0] = usize::MAX - 3;
    let result = b.conv(&input, &weight, &options).unwrap();
    assert_eq!(b.read_f32(&result).unwrap(), [0.]);
    let empty = b.upload_f32(shape(&[2, 3, 0]), &[]).unwrap();
    let weight = b.upload_f32(shape(&[4, 3, 2]), &[1.; 24]).unwrap();
    let mut options = ConvOptions::new(1);
    options.padding_before[0] = 2;
    options.padding_after[0] = 3;
    let result = b.conv(&empty, &weight, &options).unwrap();
    assert_eq!(result.shape(), &shape(&[2, 4, 4]));
    assert_eq!(b.read_f32(&result).unwrap(), [0.; 32]);
}

#[test]
fn foreign_and_mixed_dtype_inputs_fail_before_empty_shortcuts() {
    let Some(b) = backend() else { return };
    let other = MlxBackend::new_gpu().unwrap();
    let empty = b.upload_f32(shape(&[0, 1, 3]), &[]).unwrap();
    let foreign = other.upload_f32(shape(&[1, 1, 1]), &[1.]).unwrap();
    assert!(matches!(
        b.conv(&empty, &foreign, &ConvOptions::new(1)),
        Err(MlxError::ForeignContext)
    ));
    let input = b.upload_low(LowDtype::F16, shape(&[0, 1, 3]), &[]).unwrap();
    let weight = b
        .upload_low(LowDtype::Bf16, shape(&[1, 1, 1]), &[0x3f80])
        .unwrap();
    assert!(matches!(
        b.conv_low_f32(&input, &weight, &ConvOptions::new(1)),
        Err(MlxError::Contract(TensorError::LowDtypeMismatch { .. }))
    ));
    let unsigned = b.upload_u32(shape(&[1, 1, 1]), &[1]).unwrap();
    assert!(matches!(
        b.conv(&empty, &unsigned, &ConvOptions::new(1)),
        Err(MlxError::Dtype)
    ));
    let mut invalid = ConvOptions::new(1);
    invalid.groups = 0;
    assert!(matches!(
        b.conv(&empty, &empty, &invalid),
        Err(MlxError::Contract(TensorError::InvalidConvolution(_)))
    ));
}
