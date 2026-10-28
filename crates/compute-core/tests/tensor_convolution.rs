use compute_core::{ComputeRuntime, GpuLowTensor, GpuTensor, gpu_compute::GpuContext};
use tensor_core::{
    ConvOptions, Layout, LowDtype, Shape, TensorBackend, TensorConvBackend, TensorLowBackend,
};

fn context() -> Option<GpuContext> {
    let context = GpuContext::new();
    assert!(
        context.is_some() || std::env::var_os("COMPUTE_REQUIRE_GPU").is_none(),
        "required WGSL convolution device unavailable"
    );
    if context.is_none() {
        eprintln!("SKIP WGSL convolution: no device");
    }
    context
}
fn shape(d: &[usize]) -> Shape {
    Shape::new(d.to_vec()).unwrap()
}

#[test]
fn shared_convolution_contracts() {
    let Some(context) = context() else { return };
    let backend = ComputeRuntime::new(&context).unwrap();
    eprintln!("convolution WGSL: {:?}", context.backend_report());
    tensor_core::conformance::check_conv_backend(&backend).unwrap();
    tensor_core::conformance::check_low_conv_backend(&backend).unwrap();
}

// Exactly representable values in these private replay/offset fixtures.
fn bits(dtype: LowDtype, x: f32) -> u16 {
    let b = x.to_bits();
    if dtype == LowDtype::Bf16 {
        return (b >> 16) as u16;
    }
    if x == 0. {
        return ((b >> 16) & 0x8000) as u16;
    }
    (((b >> 16) & 0x8000) | ((((b >> 23) & 255) - 112) << 10) | ((b & 0x7fffff) >> 13)) as u16
}
fn expected(input: &[f32], weight: &[f32]) -> Vec<f32> {
    (0..10)
        .map(|out| {
            let c = out / 5;
            let x = out % 5;
            (0..3)
                .filter_map(|k| {
                    let source = x as isize + k as isize - 1;
                    (0..5).contains(&source).then(|| {
                        f64::from(input[1 + c * 12 + source as usize * 2])
                            * f64::from(weight[1 + c * 9 + k * 2])
                    })
                })
                .sum::<f64>() as f32
        })
        .collect()
}

#[test]
fn recorded_convolution_replays_strided_inputs_and_preserves_offset_outputs() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let input_layout = Layout::new(shape(&[1, 2, 5]), vec![25, 12, 2], 1).unwrap();
    let weight_layout = Layout::new(shape(&[2, 1, 3]), vec![9, 6, 2], 1).unwrap();
    let input_back = rt.zeros::<f32>(25).unwrap();
    let weight_back = rt.zeros::<f32>(17).unwrap();
    let input = GpuTensor::from_layout(input_back.clone(), input_layout.clone()).unwrap();
    let weight = GpuTensor::from_layout(weight_back.clone(), weight_layout.clone()).unwrap();
    let output_back = rt.upload_f32(shape(&[14]), &[99.; 14]).unwrap();
    let output = output_back
        .narrow(0, 1, 10)
        .unwrap()
        .reshape(shape(&[1, 2, 5]))
        .unwrap();
    let mut options = ConvOptions::new(1);
    options.groups = 2;
    options.padding_before[0] = 1;
    options.padding_after[0] = 1;
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let low_input_back = rt.zeros_low(dtype, shape(&[25])).unwrap();
        let low_weight_back = rt.zeros_low(dtype, shape(&[17])).unwrap();
        let low_input = GpuLowTensor::from_packed(
            dtype,
            low_input_back.packed_words().clone(),
            25,
            input_layout.clone(),
        )
        .unwrap();
        let low_weight = GpuLowTensor::from_packed(
            dtype,
            low_weight_back.packed_words().clone(),
            17,
            weight_layout.clone(),
        )
        .unwrap();
        let low_output_back = rt
            .upload_low_bits(dtype, shape(&[14]), &[0x5aa5; 14])
            .unwrap();
        let low_output = low_output_back
            .narrow(0, 1, 10)
            .unwrap()
            .reshape(shape(&[1, 2, 5]))
            .unwrap();
        let mut p = rt.program();
        p.tensor_conv_into(&input, &weight, &options, &output)
            .unwrap();
        let wide = p
            .tensor_conv_low_f32(&low_input, &low_weight, &options)
            .unwrap();
        p.tensor_conv_low_into(&low_input, &low_weight, &options, &low_output)
            .unwrap();
        let total = p.tensor_sum(&wide, &[0, 1, 2], false).unwrap();
        for iteration in 0..3 {
            let mut a = vec![88.; 25];
            let mut b = vec![77.; 17];
            for c in 0..2 {
                for x in 0..5 {
                    a[1 + c * 12 + x * 2] = if iteration == 2 {
                        0.
                    } else {
                        (x + c + iteration) as f32 / 4. - 0.5
                    };
                }
                for k in 0..3 {
                    b[1 + c * 9 + k * 2] = (k + 1 + iteration) as f32 / 4.;
                }
            }
            rt.write(&input_back, 0, &a).unwrap();
            rt.write(&weight_back, 0, &b).unwrap();
            rt.write_low_storage_bits(
                &low_input_back,
                &a.iter().map(|&v| bits(dtype, v)).collect::<Vec<_>>(),
            )
            .unwrap();
            rt.write_low_storage_bits(
                &low_weight_back,
                &b.iter().map(|&v| bits(dtype, v)).collect::<Vec<_>>(),
            )
            .unwrap();
            p.submit();
            let expected = expected(&a, &b);
            assert_eq!(rt.read_f32(&output).unwrap(), expected);
            assert_eq!(rt.read_f32(&wide).unwrap(), expected);
            assert_eq!(rt.read_f32(&total).unwrap(), [expected.iter().sum::<f32>()]);
            let raw = rt.read_low_bits(&low_output_back).unwrap();
            assert_eq!(raw[0], 0x5aa5);
            assert_eq!(&raw[11..], &[0x5aa5; 3]);
            let rounded = rt.cast_to_low(&wide, dtype).unwrap();
            assert_eq!(&raw[1..11], rt.read_low_bits(&rounded).unwrap());
            let fraw = rt.read_f32(&output_back).unwrap();
            assert_eq!(fraw[0], 99.);
            assert_eq!(&fraw[11..], &[99.; 3]);
        }
    }
}

#[test]
fn validation_does_not_append_writes_and_zero_contractions_rewrite_outputs() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let foreign = ComputeRuntime::new(&context).unwrap();
    let s = shape(&[1, 1, 3]);
    let input = rt.upload_f32(s.clone(), &[1., 2., 3.]).unwrap();
    let weight = rt.upload_f32(shape(&[1, 1, 1]), &[1.]).unwrap();
    let other = foreign.upload_f32(s.clone(), &[1., 2., 3.]).unwrap();
    let output = rt.upload_f32(s, &[99.; 3]).unwrap();
    let opts = ConvOptions::new(1);
    let low = rt.cast_to_low(&input, LowDtype::Bf16).unwrap();
    let lw = rt.cast_to_low(&weight, LowDtype::Bf16).unwrap();
    let low_output = rt.cast_to_low(&output, LowDtype::Bf16).unwrap();
    let wrong = rt.cast_to_low(&weight, LowDtype::F16).unwrap();
    let foreign_low = foreign.cast_to_low(&other, LowDtype::Bf16).unwrap();
    let mut p = rt.program();
    assert!(p.tensor_conv_into(&input, &weight, &opts, &input).is_err());
    assert!(p.tensor_conv_into(&other, &weight, &opts, &output).is_err());
    assert!(p.tensor_conv_into(&input, &weight, &opts, &other).is_err());
    assert!(p.tensor_conv_low_into(&low, &lw, &opts, &low).is_err());
    assert!(
        p.tensor_conv_low_into(&foreign_low, &lw, &opts, &low_output)
            .is_err()
    );
    assert!(
        p.tensor_conv_low_into(&low, &wrong, &opts, &low_output)
            .is_err()
    );
    let bad_layout = GpuTensor::from_layout(
        output.values().clone(),
        Layout::new(shape(&[1, 1, 3]), vec![0, 0, 0], 0).unwrap(),
    )
    .unwrap();
    assert!(
        p.tensor_conv_into(&input, &weight, &opts, &bad_layout)
            .is_err()
    );
    let mut huge = opts.clone();
    huge.padding_before[0] = u32::MAX as usize;
    huge.strides[0] = u32::MAX as usize;
    let tiny_input = rt.upload_f32(shape(&[1, 1, 1]), &[1.]).unwrap();
    let pair_output = rt.upload_f32(shape(&[1, 1, 2]), &[99.; 2]).unwrap();
    assert!(
        p.tensor_conv_into(&tiny_input, &weight, &huge, &pair_output)
            .is_err()
    );
    p.submit();
    assert_eq!(rt.read_f32(&output).unwrap(), [99.; 3]);
    assert_eq!(rt.read_low_bits(&low_output).unwrap(), [0x42c6; 3]);
    assert_eq!(rt.read_f32(&pair_output).unwrap(), [99.; 2]);
    let empty = rt.upload_f32(shape(&[1, 0, 3]), &[]).unwrap();
    let empty_weight = rt.upload_f32(shape(&[1, 0, 1]), &[]).unwrap();
    let el = rt.cast_to_low(&empty, LowDtype::Bf16).unwrap();
    let ew = rt.cast_to_low(&empty_weight, LowDtype::Bf16).unwrap();
    let mut zeros = rt.program();
    zeros
        .tensor_conv_into(&empty, &empty_weight, &opts, &output)
        .unwrap();
    zeros
        .tensor_conv_low_into(&el, &ew, &opts, &low_output)
        .unwrap();
    for _ in 0..2 {
        rt.write(output.values(), 0, &[99.; 3]).unwrap();
        rt.write_low_storage_bits(&low_output, &[0x42c6; 3])
            .unwrap();
        zeros.submit();
        assert_eq!(rt.read_f32(&output).unwrap(), [0.; 3]);
        assert_eq!(rt.read_low_bits(&low_output).unwrap(), [0; 3]);
    }
}

#[test]
fn grid_stride_outputs_refresh_workgroup_partials() {
    let Some(context) = context() else { return };
    let rt = ComputeRuntime::new(&context).unwrap();
    let n = 65_537;
    let values: Vec<_> = (0..n).map(|i| (i % 17) as f32 - 8.).collect();
    let input = rt.upload_f32(shape(&[1, 1, n]), &values).unwrap();
    let weight = rt.upload_f32(shape(&[1, 1, 1]), &[2.]).unwrap();
    let output = rt.conv(&input, &weight, &ConvOptions::new(1)).unwrap();
    assert_eq!(
        rt.read_f32(&output).unwrap(),
        values.iter().map(|v| v * 2.).collect::<Vec<_>>()
    );
}
