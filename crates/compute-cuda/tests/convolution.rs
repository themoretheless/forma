use compute_cuda::{CudaError, CudaRuntime};
use tensor_core::{
    ConvOptions, LowDtype, Shape, TensorBackend, TensorConvBackend, TensorLowBackend,
    TensorLowConvBackend,
};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn runtime() -> Result<Option<CudaRuntime>, CudaError> {
    match CudaRuntime::new() {
        Ok(runtime) => Ok(Some(runtime)),
        Err(error @ CudaError::Unavailable(_)) => {
            assert!(
                !["CUDA_REQUIRED", "COMPUTE_REQUIRE_CUDA"]
                    .iter()
                    .any(|name| std::env::var(name).as_deref() == Ok("1")),
                "required native CUDA convolution unavailable: {error}"
            );
            eprintln!("SKIP native CUDA convolution qualification: {error}");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[test]
fn native_convolution_shared_contract() -> Result<(), CudaError> {
    let Some(backend) = runtime()? else {
        return Ok(());
    };
    eprintln!("CUDA convolution device: {:?}", backend.capabilities());
    tensor_core::conformance::check_conv_backend(&backend)?;
    tensor_core::conformance::check_low_conv_backend(&backend)?;
    Ok(())
}

#[test]
fn native_offsets_changed_inputs_and_single_final_low_rounding() -> Result<(), CudaError> {
    let Some(b) = runtime()? else {
        return Ok(());
    };
    let options = ConvOptions::new(1);
    let weight = b.narrow(
        &b.upload_f32(shape(&[1, 1, 4]), &[99., 2., -1., 99.])?,
        2,
        1,
        2,
    )?;
    let first = b.narrow(
        &b.upload_f32(shape(&[1, 1, 6]), &[99., 1., 2., 3., 4., 99.])?,
        2,
        1,
        4,
    )?;
    let saved = b.conv(&first, &weight, &options)?;
    let second = b.narrow(
        &b.upload_f32(shape(&[1, 1, 6]), &[99., -1., 0., 1., 2., 99.])?,
        2,
        1,
        4,
    )?;
    assert_eq!(
        b.read_f32(&b.conv(&second, &weight, &options)?)?,
        [-2., -1., 0.]
    );
    assert_eq!(b.read_f32(&saved)?, [0., 1., 2.]);
    for (dtype, delta, one) in [
        (LowDtype::F16, 2_f32.powi(-11), 0x3c00),
        (LowDtype::Bf16, 2_f32.powi(-8), 0x3f80),
    ] {
        // Input/weight offsets are odd halfwords, with an ignored adjacent value.
        let input = b.cast_to_low(
            &b.upload_f32(shape(&[1, 1, 4]), &[99., 1., delta, 99.])?,
            dtype,
        )?;
        let input = b.narrow_low(&input, 2, 1, 2)?;
        let kernel = b.cast_to_low(
            &b.upload_f32(shape(&[1, 1, 4]), &[99., 1., 1., 99.])?,
            dtype,
        )?;
        let kernel = b.narrow_low(&kernel, 2, 1, 2)?;
        let wide = b.conv_low_f32(&input, &kernel, &options)?;
        let rounded = b.conv_low(&input, &kernel, &options)?;
        assert_eq!(b.read_f32(&wide)?, [1. + delta]);
        assert_eq!(b.read_low_bits(&rounded)?, [one]);
        assert_eq!(
            b.read_low_bits(&input)?,
            b.read_low_bits(
                &b.cast_to_low(&b.upload_f32(shape(&[1, 1, 2]), &[1., delta])?, dtype)?
            )?
        );
    }
    Ok(())
}

#[test]
fn native_preflight_validates_owners_and_low_dtype_for_empty_results() -> Result<(), CudaError> {
    let Some(b) = runtime()? else {
        return Ok(());
    };
    let other = CudaRuntime::new()?;
    let options = ConvOptions::new(1);
    let local = b.upload_f32(shape(&[0, 1, 4]), &[])?;
    let foreign = other.upload_f32(shape(&[0, 1, 4]), &[])?;
    let kernel = b.upload_f32(shape(&[1, 1, 1]), &[1.])?;
    let foreign_kernel = other.upload_f32(shape(&[1, 1, 1]), &[1.])?;
    assert!(matches!(
        b.conv(&foreign, &kernel, &options),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(matches!(
        b.conv(&local, &foreign_kernel, &options),
        Err(CudaError::ForeignRuntime)
    ));
    assert!(b.read_f32(&b.conv(&local, &kernel, &options)?)?.is_empty());
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let low = b.upload_low(dtype, shape(&[0, 1, 4]), &[])?;
        let bad_owner = other.upload_low(dtype, shape(&[0, 1, 4]), &[])?;
        let weight = b.cast_to_low(&kernel, dtype)?;
        let bad_weight = other.cast_to_low(&foreign_kernel, dtype)?;
        assert!(matches!(
            b.conv_low_f32(&bad_owner, &weight, &options),
            Err(CudaError::ForeignRuntime)
        ));
        assert!(matches!(
            b.conv_low_f32(&low, &bad_weight, &options),
            Err(CudaError::ForeignRuntime)
        ));
        let different = if dtype == LowDtype::F16 {
            LowDtype::Bf16
        } else {
            LowDtype::F16
        };
        let bad_dtype = b.cast_to_low(&kernel, different)?;
        assert!(matches!(
            b.conv_low_f32(&low, &bad_dtype, &options),
            Err(CudaError::Contract(_))
        ));
        assert!(
            b.read_f32(&b.conv_low_f32(&low, &weight, &options)?)?
                .is_empty()
        );
    }
    Ok(())
}
