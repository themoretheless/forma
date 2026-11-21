use super::*;

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn block_on_ready<T>(fut: impl Future<Output = T>) -> T {
    let mut pinned = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match pinned.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("expected immediate future completion"),
    }
}

#[test]
fn pipeline_cache_keys_distinguish_blend_sample_count_and_threadgroup_multiple() {
    let base_render = metal::RenderPipelineCacheKey {
        vs_src: "vertex float4 vs_main() { return float4(0.0); }".into(),
        fs_src: Some("fragment float4 fs_main() { return float4(1.0); }".into()),
        vs_ep: "vs_main".into(),
        fs_ep: "fs_main".into(),
        vertex_buffers: vec![Some(metal::CachedVertexBufferLayout {
            array_stride: 16,
            step_mode: VertexStepMode::Vertex,
            attributes: vec![VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }],
        })],
        targets: vec![Some(ColorTargetState {
            format: TextureFormat::Bgra8Unorm,
            blend: Some(BlendState::ALPHA_BLENDING),
            write_mask: ColorWrites::ALL,
        })],
        depth_stencil: None,
        sample_count: 1,
    };

    let mut premul_render = base_render.clone();
    premul_render.targets[0].as_mut().unwrap().blend =
        Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
    assert_ne!(base_render, premul_render);

    let mut msaa_render = base_render.clone();
    msaa_render.sample_count = 4;
    assert_ne!(base_render, msaa_render);

    let base_compute = metal::ComputePipelineCacheKey {
        msl_source: "kernel void cs_main() {}".into(),
        entry_point: "cs_main".into(),
        threadgroup_multiple_of_32: true,
    };
    let mut non_multiple = base_compute.clone();
    non_multiple.threadgroup_multiple_of_32 = false;
    assert_ne!(base_compute, non_multiple);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn ray_tracing_is_not_advertised_and_its_buffers_are_rejected() {
    let Some(metal_dev) = MetalDevice::create_system_default() else {
        return;
    };
    let adapter = Adapter::from_metal(metal_dev);
    let ray_tracing = Features::EXPERIMENTAL_RAY_QUERY
        | Features::EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE
        | Features::RAY_TRACING_PIPELINE;
    assert!(!adapter.features().intersects(ray_tracing));

    // Asking for the features does not grant them: nothing here builds acceleration structures.
    let (device, _queue) = block_on_ready(adapter.request_device(&DeviceDescriptor {
        required_features: ray_tracing,
        ..Default::default()
    }))
    .expect("device creation must succeed");
    assert!(!device.features().intersects(ray_tracing));
    let scope = device.push_error_scope(ErrorFilter::Validation);
    device.create_buffer(&BufferDescriptor {
        label: None,
        size: 64,
        usage: BufferUsages::BLAS_INPUT | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    assert!(matches!(
        block_on_ready(scope.pop()),
        Some(Error::Validation { .. })
    ));
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[test]
fn oversized_buffer_and_overflowing_write_offsets_fail_safely() {
    let Some(metal_dev) = MetalDevice::create_system_default() else {
        return;
    };
    let adapter = Adapter::from_metal(metal_dev);
    let (device, queue) = block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
        .expect("device creation must succeed");

    // G-4: Oversized buffer creation records validation error and returns an inert buffer
    // whose UMA/mapped views never dereference null pointers.
    let validation_scope = device.push_error_scope(ErrorFilter::Validation);
    let mut invalid_buf = device.create_buffer(&BufferDescriptor {
        label: Some("oversized"),
        size: u64::MAX,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let err = block_on_ready(validation_scope.pop());
    assert!(matches!(err, Some(Error::Validation { .. })));
    assert!(invalid_buf.uma_slice().is_empty());
    assert!(invalid_buf.uma_slice_mut().is_empty());
    assert!(invalid_buf.slice(..).get_mapped_range().is_empty());
    assert!(invalid_buf.slice(..).get_mapped_range_mut().is_empty());

    // Direct MetalDevice::alloc_buffer overflow check returns None cleanly.
    assert!(device.expect_metal().alloc_buffer(usize::MAX, 0).is_none());

    // G-5: Overflowing write offsets on a valid buffer are rejected instead of wrapping.
    let valid_buf = device.create_buffer(&BufferDescriptor {
        label: Some("valid"),
        size: 64,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    assert!(
        queue
            .write_buffer_with(&valid_buf, u64::MAX - 3, BufferSize::new(8).unwrap())
            .is_none()
    );
    let write_scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_buffer(&valid_buf, u64::MAX - 3, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let write_err = block_on_ready(write_scope.pop());
    assert!(matches!(write_err, Some(Error::Validation { .. })));
}
