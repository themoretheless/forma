use super::*;
use crate::test_support::validate_wgsl;

#[test]
fn shader_parses_and_validates_with_naga() {
    validate_wgsl(
        LINEAR_EXPOSURE_SHADER,
        wgpu::naga::valid::Capabilities::empty(),
    );
}

#[cfg(target_os = "macos")]
#[test]
fn exposure_readback_cancellation_releases_mapping_and_allows_retry() {
    let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
    let compute = LinearExposureCompute::new(&device);
    let input = vec![[4.0, -2.0, 0.125, 0.375]; 8200];
    let bytes = input.len() as u64 * 16;
    let cpu = MemoryBudget::new(bytes);
    let gpu = MemoryBudget::new(bytes * 3 + 16);
    for flat in [false, true] {
        for checkpoint in 1..=6 {
            let mut admitted_polls = 0;
            let mut cancel = || {
                if gpu.used() > 0 {
                    admitted_polls += 1;
                }
                admitted_polls == checkpoint
            };
            let cancelled = if flat {
                matches!(
                    compute.execute_interleaved_with_cancel(
                        &queue,
                        bytemuck::cast_slice(&input),
                        1.0,
                        &gpu,
                        &cpu,
                        &mut cancel,
                    ),
                    Err(ExposureError::Cancelled)
                )
            } else {
                matches!(
                    compute.execute_managed_with_cancel(
                        &queue,
                        &input,
                        1.0,
                        &gpu,
                        &cpu,
                        &mut cancel,
                    ),
                    Err(ExposureError::Cancelled)
                )
            };
            assert!(cancelled);
            assert_eq!(admitted_polls, checkpoint);
            assert_eq!(cpu.used(), 0);
            assert_eq!(gpu.used(), 0);
            let retry = compute
                .execute_managed(&queue, &input, 1.0, &gpu, &cpu)
                .unwrap();
            assert!(retry.iter().all(|pixel| *pixel == [8.0, -4.0, 0.25, 0.375]));
            drop(retry);
            assert_eq!(cpu.used(), 0);
            assert_eq!(gpu.used(), 0);
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn linear_exposure_compute_preserves_hdr_alpha_and_dispatch_tail() {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffer_binding_size: 1024,
            ..wgpu::Limits::default()
        },
        ..Default::default()
    }))
    .unwrap();
    let source_pixels: Vec<[f32; 4]> = (0..129)
        .map(|i| [128.0 + i as f32, -2.0, 0.125, 0.25])
        .collect();
    let initial = vec![[9.0_f32; 4]; 130];
    let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&source_pixels),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&initial),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let compute = LinearExposureCompute::new(&device);
    let finite_cpu = MemoryBudget::new(16);
    let finite_gpu = MemoryBudget::new(64);
    for pixel in [
        [f32::MAX, 0.0, 0.0, 1.0],
        [-f32::MAX, 0.0, 0.0, 1.0],
        [f32::INFINITY, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, f32::NAN],
    ] {
        assert!(matches!(
            compute.execute_managed(&queue, &[pixel], 1.0, &finite_gpu, &finite_cpu),
            Err(ExposureError::Invalid(_))
        ));
        assert_eq!(finite_cpu.peak(), 0);
        assert_eq!(finite_gpu.peak(), 0);
    }
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    assert!(
        compute
            .encode(&mut encoder, &source, &output, 129, f32::NAN)
            .is_err()
    );
    assert!(
        compute
            .encode(&mut encoder, &source, &source.clone(), 129, 1.0)
            .is_err()
    );
    assert!(
        compute
            .encode(&mut encoder, &source, &output, 130, 1.0)
            .is_err()
    );
    compute
        .encode(&mut encoder, &source, &output, 129, 1.0)
        .unwrap();
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
    let submission = queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    {
        let mapped = readback.slice(..).get_mapped_range().unwrap();
        let pixels: &[[f32; 4]] = bytemuck::cast_slice(&mapped);
        for (i, pixel) in pixels[..129].iter().enumerate() {
            assert_eq!(pixel, &[256.0 + 2.0 * i as f32, -4.0, 0.25, 0.25]);
        }
        assert_eq!(pixels[129], [9.0; 4]);
    }
    readback.unmap();
}

#[cfg(target_os = "macos")]
#[test]
fn resident_exposure_preserves_hdr_tail_and_budgeted_gpu_ownership() {
    let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
    let compute = LinearExposureCompute::new(&device);
    let input = vec![[4., -2., 0.125, 0.375]; 8200];
    let bytes = input.len() as u64 * 16;
    let root = MemoryBudget::new(bytes * 2 + 16);
    for checkpoint in 1..=6 {
        let mut polls = 0;
        assert!(matches!(
            compute.execute_resident(&queue, &input, 1., &root, || {
                polls += 1;
                polls == checkpoint
            }),
            Err(ExposureError::Cancelled)
        ));
        assert_eq!(root.used(), 0);
    }
    let refused = MemoryBudget::new(bytes * 2 + 15);
    assert!(
        compute
            .execute_resident(&queue, &input, 1., &refused, || false)
            .is_err()
    );
    assert_eq!(refused.used(), 0);
    let output = compute
        .execute_resident(&queue, &input, 1., &root, || false)
        .unwrap();
    assert_eq!(output.pixels(), 8200);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(root.used(), bytes);
    let held = output.clone();
    drop(output);
    assert_eq!(root.used(), bytes);
    let downstream = held.resource_lease();
    drop(held);
    assert_eq!(root.used(), bytes);
    drop(downstream);
    assert_eq!(root.used(), 0);
    let abandoned = compute
        .execute_resident(&queue, &input, 1., &root, || false)
        .unwrap();
    drop(abandoned);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(root.used(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn exposure_signed_f32_boundary_values_remain_exact_and_finite() {
    let (device, queue) = crate::test_support::metal_device(wgpu::Features::empty());
    let compute = LinearExposureCompute::new(&device);
    let cpu = MemoryBudget::new(16);
    let gpu = MemoryBudget::new(64);
    for (stops, magnitude) in [
        (0f32, f32::MAX),
        (1., f32::MAX / 2.),
        (32., f32::MAX / 4294967296.),
        (-32., f32::MAX),
    ] {
        let pixel = [magnitude, -magnitude, -0., -f32::MAX];
        let gain = stops.exp2();
        let expected = [pixel[0] * gain, pixel[1] * gain, pixel[2] * gain, pixel[3]];
        let result = compute
            .execute_managed(&queue, &[pixel], stops, &gpu, &cpu)
            .unwrap();
        assert!(result[0].iter().all(|v| v.is_finite()));
        assert_eq!(result[0].map(f32::to_bits), expected.map(f32::to_bits));
        drop(result);
        assert_eq!((cpu.used(), gpu.used()), (0, 0));
    }
}
