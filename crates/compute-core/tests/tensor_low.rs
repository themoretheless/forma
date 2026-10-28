use compute_core::{
    ComputeError, ComputeRuntime, GpuLowTensor, GpuTensor, TensorComputeError,
    gpu_compute::GpuContext,
    tensor_core::{Layout, LowDtype, LowStorage, Shape, TensorLowBackend},
};
use std::time::Duration;
const TIMEOUT: Duration = Duration::from_secs(30);
fn context() -> Option<GpuContext> {
    let value = GpuContext::new();
    assert!(value.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
    value
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}

#[test]
fn shader_low_backend_satisfies_exhaustive_shared_conformance() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let support = rt.low_precision_support(dtype);
        assert_eq!(support.storage, LowStorage::Packed16x2);
        assert!(support.matmul && support.matmul_f32);
    }
    tensor_core::conformance::check_low_backend(&rt).unwrap();
}

#[test]
fn packed_views_preserve_all_neighboring_halves_with_odd_offsets_and_reuse() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for n in [0, 1, 2, 3, 255, 256, 257] {
            let allocation = rt.zeros_low(dtype, shape(&[n])).unwrap();
            assert_eq!(
                allocation.allocation_bytes(),
                (n.div_ceil(2) * 4).max(4) as u64
            );
            assert_eq!(allocation.packed_words().len(), n.div_ceil(2));
            let input_backing = rt
                .upload_low_bits(dtype, shape(&[n + 4]), &vec![0; n + 4])
                .unwrap();
            let input = input_backing.narrow(0, 1, n).unwrap();
            for offset in [1, 2] {
                let out_backing = rt
                    .upload_low_bits(dtype, shape(&[n + 5]), &vec![0xa55a; n + 5])
                    .unwrap();
                let output = out_backing.narrow(0, offset, n).unwrap();
                let mut p = rt.program();
                p.tensor_materialize_low_into(&input, &output).unwrap();
                for iteration in 0..2 {
                    let bits: Vec<u16> = (0..n + 4)
                        .map(|i| {
                            [0x8000, 1, 0x7fff, 0xffff, 0x1234, 0x7c00, 0xfc00][(i + iteration) % 7]
                        })
                        .collect();
                    rt.write_low_storage_bits(&input_backing, &bits).unwrap();
                    let words = p
                        .submit_read(out_backing.packed_words())
                        .unwrap()
                        .wait(TIMEOUT)
                        .unwrap();
                    let all: Vec<u16> = words
                        .iter()
                        .flat_map(|&w| [w as u16, (w >> 16) as u16])
                        .collect();
                    let mut expected = vec![0xa55a; n + 5];
                    expected[offset..offset + n].copy_from_slice(&bits[1..1 + n]);
                    assert_eq!(
                        &all[..n + 5],
                        expected,
                        "{dtype:?}, n={n}, offset={offset}, iteration={iteration}"
                    );
                    if (n + 5) % 2 == 1 {
                        assert_eq!(all[n + 5], 0, "padding lane changed");
                    }
                }
            }
        }
    }
}

#[test]
fn recorded_cast_strided_batched_matmul_and_low_output_reuse_without_expansion() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let (m, k, n) = (17, 19, 21);
    let count = 2 * m * n;
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let a_raw = rt.zeros::<f32>(2 * k * m).unwrap();
        let b_raw = rt.zeros::<f32>(n * k).unwrap();
        let a = GpuTensor::from_array(a_raw.clone(), shape(&[2, k, m])).unwrap();
        let b = GpuTensor::from_array(b_raw.clone(), shape(&[1, n, k])).unwrap();
        let out_raw = rt.upload(&vec![-999.0_f32; count + 6]).unwrap();
        let out = GpuTensor::from_layout(
            out_raw.clone(),
            Layout::new(shape(&[2, m, n]), vec![m * n, n, 1], 3).unwrap(),
        )
        .unwrap();
        let low_backing = rt
            .upload_low_bits(dtype, shape(&[count + 3]), &vec![0xa55a; count + 3])
            .unwrap();
        let low_output = GpuLowTensor::from_packed(
            dtype,
            low_backing.packed_words().clone(),
            count + 3,
            Layout::new(shape(&[2, m, n]), vec![m * n, n, 1], 1).unwrap(),
        )
        .unwrap();
        let mut p = rt.program();
        let alow = p
            .tensor_cast_to_low(&a, dtype)
            .unwrap()
            .permute(&[0, 2, 1])
            .unwrap();
        let blow = p
            .tensor_cast_to_low(&b, dtype)
            .unwrap()
            .permute(&[0, 2, 1])
            .unwrap();
        p.tensor_matmul_low_f32_into(&alow, &blow, &out).unwrap();
        p.tensor_matmul_low_into(&alow, &blow, &low_output).unwrap();
        let decoded = p.tensor_cast_to_f32(&low_output).unwrap();
        let sum = p.tensor_sum(&decoded, &[0, 1, 2], false).unwrap();
        assert_eq!(alow.allocation_bytes(), (2 * k * m).div_ceil(2) as u64 * 4);
        for iteration in 0..3 {
            let av: Vec<f32> = (0..2 * k * m)
                .map(|i| ((i + iteration * 7) % 5) as f32 - 2.)
                .collect();
            let bv: Vec<f32> = (0..n * k)
                .map(|i| ((i * 2 + iteration) % 3) as f32 - 1.)
                .collect();
            rt.write(&a_raw, 0, &av).unwrap();
            rt.write(&b_raw, 0, &bv).unwrap();
            let mut expected = Vec::with_capacity(count);
            for batch in 0..2 {
                for row in 0..m {
                    for col in 0..n {
                        expected.push(
                            (0..k)
                                .map(|inner| {
                                    f64::from(av[batch * k * m + inner * m + row])
                                        * f64::from(bv[col * k + inner])
                                })
                                .sum::<f64>() as f32,
                        );
                    }
                }
            }
            let mut encoder = rt.device().create_command_encoder(&Default::default());
            p.record(&mut encoder);
            let mut direct = rt.record_read(&mut encoder, &out_raw).unwrap();
            let mut rounded = rt.record_read(&mut encoder, decoded.values()).unwrap();
            let mut raw_low = rt
                .record_read(&mut encoder, low_backing.packed_words())
                .unwrap();
            let mut total = rt.record_read(&mut encoder, sum.values()).unwrap();
            let submitted = rt.queue().submit([encoder.finish()]);
            direct.submitted(submitted.clone());
            rounded.submitted(submitted.clone());
            raw_low.submitted(submitted.clone());
            total.submitted(submitted);
            let direct = direct.wait(TIMEOUT).unwrap();
            assert_eq!(&direct[3..count + 3], expected);
            assert_eq!(&direct[..3], &[-999.; 3]);
            assert_eq!(&direct[count + 3..], &[-999.; 3]);
            assert_eq!(rounded.wait(TIMEOUT).unwrap(), expected);
            assert_eq!(total.wait(TIMEOUT).unwrap(), [expected.iter().sum::<f32>()]);
            let words = raw_low.wait(TIMEOUT).unwrap();
            let raw: Vec<u16> = words
                .iter()
                .flat_map(|&w| [w as u16, (w >> 16) as u16])
                .collect();
            assert_eq!(raw[0], 0xa55a);
            assert_eq!(&raw[count + 1..count + 3], &[0xa55a; 2]);
        }
    }
}

#[test]
fn low_metadata_dtype_ownership_and_cross_type_aliases_are_rejected() {
    let Some(ctx) = context() else { return };
    let rt = ComputeRuntime::new(&ctx).unwrap();
    let other = ComputeRuntime::new(&ctx).unwrap();
    let a = rt
        .upload_low_bits(LowDtype::F16, shape(&[2, 2]), &[0x3c00; 4])
        .unwrap();
    let b = rt
        .upload_low_bits(LowDtype::Bf16, shape(&[2, 2]), &[0x3f80; 4])
        .unwrap();
    let foreign = other.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    assert!(rt.read_low_bits(&foreign).is_err());
    assert!(rt.permute_low(&foreign, &[1, 0]).is_err());
    assert!(rt.broadcast_low(&foreign, shape(&[2, 2])).is_err());
    assert!(
        GpuLowTensor::from_packed(
            LowDtype::F16,
            a.packed_words().clone(),
            5,
            Layout::contiguous(shape(&[5])).unwrap()
        )
        .is_err()
    );
    assert!(
        GpuLowTensor::from_packed(
            LowDtype::F16,
            a.packed_words().clone(),
            3,
            Layout::new(shape(&[1]), vec![1], 3).unwrap()
        )
        .is_err()
    );
    let buffer = compute_core::gpu_compute::GpuBuffer::new(
        &ctx,
        16,
        compute_core::wgpu::BufferUsages::STORAGE
            | compute_core::wgpu::BufferUsages::COPY_SRC
            | compute_core::wgpu::BufferUsages::COPY_DST,
    )
    .unwrap();
    let float_alias = GpuTensor::from_array(
        rt.import_buffer::<f32>(buffer.clone(), 4).unwrap(),
        shape(&[2, 2]),
    )
    .unwrap();
    let low_alias = GpuLowTensor::from_packed(
        LowDtype::F16,
        rt.import_buffer::<u32>(buffer, 4).unwrap(),
        8,
        Layout::contiguous(shape(&[2, 2])).unwrap(),
    )
    .unwrap();
    let mut p = rt.program();
    assert!(matches!(
        p.tensor_cast_to_low_into(&float_alias, &low_alias),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_cast_to_f32_into(&low_alias, &float_alias),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_matmul_low(&a, &b),
        Err(TensorComputeError::LowDtypeMismatch { .. })
    ));
    assert!(matches!(
        p.tensor_materialize_low_into(&a, &a),
        Err(TensorComputeError::Compute(ComputeError::AliasedOutput))
    ));
    assert!(matches!(
        p.tensor_matmul_low_into(&a, &a, &foreign),
        Err(TensorComputeError::Compute(ComputeError::ForeignArray))
    ));
    assert!(matches!(
        p.tensor_materialize_low_into(&a, &b),
        Err(TensorComputeError::LowDtypeMismatch { .. })
    ));
    let bad = GpuLowTensor::from_packed(
        LowDtype::F16,
        a.packed_words().clone(),
        4,
        Layout::new(shape(&[2, 2]), vec![1, 2], 0).unwrap(),
    )
    .unwrap();
    let source = rt.zeros_low(LowDtype::F16, shape(&[2, 2])).unwrap();
    assert!(matches!(
        p.tensor_materialize_low_into(&source, &bad),
        Err(TensorComputeError::OutputNotContiguous)
    ));
    let out = p.tensor_matmul_low_f32(&a, &a).unwrap();
    assert_eq!(
        p.submit_read(out.values()).unwrap().wait(TIMEOUT).unwrap(),
        [2.; 4]
    );
}
