use gpu_runtime::{
    Adapter, Backends, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BufferBinding, BufferBindingType,
    BufferDescriptor, BufferSize, BufferUsages, Color, ColorTargetState, ColorWrites,
    CommandEncoderDescriptor, ComputePassDescriptor, ComputePipelineDescriptor, Device,
    DeviceDescriptor, DeviceLostReason, Error, ErrorFilter, Extent3d, FragmentState, Instance,
    InstanceDescriptor, LoadOp, MapMode, Operations, Origin3d, PipelineCompilationOptions,
    PipelineLayoutDescriptor, PollType, Queue, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp,
    TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
    VertexState,
};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

fn block_on_test<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut pinned = std::pin::pin!(future);
    loop {
        match pinned.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn portable_hardware_device() -> Option<(Adapter, Device, Queue)> {
    let instance = Instance::new_portable_for_testing(false);
    let adapter = match block_on_test(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(err) => {
            assert!(
                std::env::var_os("WGPU_BACKEND").is_none(),
                "WGPU_BACKEND was set but no portable adapter initialized: {err}"
            );
            return None;
        }
    };
    let (device, queue) = block_on_test(adapter.request_device(&DeviceDescriptor::default()))
        .expect("portable hardware adapter failed to create logical device");
    Some((adapter, device, queue))
}

fn portable_any_device() -> (Adapter, Device, Queue) {
    if let Some(found) = portable_hardware_device() {
        return found;
    }
    let noop_instance = Instance::new_portable_for_testing(true);
    let adapter = block_on_test(noop_instance.request_adapter(&Default::default()))
        .expect("portable noop fallback adapter unavailable");
    let (device, queue) = block_on_test(adapter.request_device(&DeviceDescriptor::default()))
        .expect("portable noop device creation failed");
    (adapter, device, queue)
}

#[test]
fn portable_compute_dispatch_and_mapped_readback() {
    let Some((_, device, queue)) = portable_hardware_device() else {
        return;
    };

    const COUNT: usize = 16;
    let byte_len = (COUNT * std::mem::size_of::<u32>()) as u64;
    let storage = device.create_buffer(&BufferDescriptor {
        label: Some("portable compute storage"),
        size: byte_len,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("portable compute readback"),
        size: byte_len,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let input: Vec<u32> = (0..COUNT as u32).collect();
    let input_bytes: Vec<u8> = input.iter().flat_map(|v| v.to_le_bytes()).collect();
    queue.write_buffer(&storage, 0, &input_bytes);

    let wgsl = "
        @group(0) @binding(0)
        var<storage, read_write> data: array<u32>;

        @compute @workgroup_size(16)
        fn main(@builtin(global_invocation_id) id: vec3<u32>) {
            let i = id.x;
            if (i < arrayLength(&data)) {
                data[i] = data[i] * 3u + 7u;
            }
        }
    ";
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("portable compute shader"),
        source: ShaderSource::Wgsl(wgsl.into()),
    });
    let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("portable compute bgl"),
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: BufferSize::new(4),
            },
            count: None,
        }],
    });
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("portable compute pl"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some("portable compute pipeline"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: PipelineCompilationOptions::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("portable compute bg"),
        layout: &bgl,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::Buffer(BufferBinding {
                buffer: &storage,
                offset: 0,
                size: None,
            }),
        }],
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, Some(&bind_group), &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, byte_len);
    queue.submit([encoder.finish()]);

    let done = Arc::new(Mutex::new(None));
    let done_tx = done.clone();
    readback.slice(..).map_async(MapMode::Read, move |res| {
        *done_tx.lock().unwrap() = Some(res);
    });
    device.poll(PollType::wait_indefinitely()).unwrap();
    assert!(done.lock().unwrap().take().unwrap().is_ok());

    let mapped = readback.slice(..).get_mapped_range();
    let actual: Vec<u32> = mapped
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();
    let expected: Vec<u32> = input.iter().map(|&v| v * 3 + 7).collect();
    assert_eq!(actual, expected);
    drop(mapped);
    readback.unmap();
}

#[test]
fn portable_render_pass_and_texture_readback() {
    let Some((_, device, queue)) = portable_hardware_device() else {
        return;
    };

    const SIZE: u32 = 32;
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("portable render target"),
        size: Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());

    let wgsl = "
        @vertex
        fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
            let x = f32(i32(vid << 1u) & 2) * 2.0 - 1.0;
            let y = f32(i32(vid & 2u)) * 2.0 - 1.0;
            return vec4<f32>(x, y, 0.0, 1.0);
        }

        @fragment
        fn fs_main() -> @location(0) vec4<f32> {
            return vec4<f32>(0.25, 0.5, 0.75, 1.0);
        }
    ";
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("portable fullscreen shader"),
        source: ShaderSource::Wgsl(wgsl.into()),
    });
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor::default());
    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("portable fullscreen pipeline"),
        layout: Some(&layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: PipelineCompilationOptions::default(),
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    });

    let padded_bpr = gpu_runtime::COPY_BYTES_PER_ROW_ALIGNMENT;
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("portable render readback"),
        size: (padded_bpr * SIZE) as u64,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("portable clear and draw"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &readback,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(SIZE),
            },
        },
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let done = Arc::new(Mutex::new(None));
    let done_tx = done.clone();
    readback.slice(..).map_async(MapMode::Read, move |res| {
        *done_tx.lock().unwrap() = Some(res);
    });
    device.poll(PollType::wait_indefinitely()).unwrap();
    assert!(done.lock().unwrap().take().unwrap().is_ok());

    let mapped = readback.slice(..).get_mapped_range();
    let px_offset = (16 * padded_bpr as usize) + 16 * 4;
    let px = &mapped[px_offset..px_offset + 4];
    assert!((px[0] as i32 - 64).abs() <= 2, "red={}", px[0]);
    assert!((px[1] as i32 - 128).abs() <= 2, "green={}", px[1]);
    assert!((px[2] as i32 - 191).abs() <= 2, "blue={}", px[2]);
    assert_eq!(px[3], 255);
    drop(mapped);
    readback.unmap();
}

#[test]
fn portable_write_buffer_with_flushes_and_enforces_mapped_ranges() {
    let Some((_, device, queue)) = portable_hardware_device() else {
        return;
    };

    let staging = device.create_buffer(&BufferDescriptor {
        label: Some("portable write_buffer_with target"),
        size: 64,
        usage: BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("portable write_buffer_with readback"),
        size: 64,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // Holding a shared read view on the shadow buffer blocks overlapping write_buffer_with.
    let reader = staging.slice(0..16).get_mapped_range();
    assert!(
        queue
            .write_buffer_with(&staging, 0, BufferSize::new(16).unwrap())
            .is_none()
    );
    drop(reader);

    let payload = [42u8; 32];
    {
        let mut writer = queue
            .write_buffer_with(&staging, 16, BufferSize::new(32).unwrap())
            .expect("non-overlapping write_buffer_with must succeed");
        writer.copy_from_slice(&payload);
    }

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(&staging, 0, &readback, 0, 64);
    queue.submit([encoder.finish()]);

    let done = Arc::new(Mutex::new(None));
    let done_tx = done.clone();
    readback.slice(..).map_async(MapMode::Read, move |res| {
        *done_tx.lock().unwrap() = Some(res);
    });
    device.poll(PollType::wait_indefinitely()).unwrap();
    assert!(done.lock().unwrap().take().unwrap().is_ok());

    let mapped = readback.slice(..).get_mapped_range();
    assert_eq!(&mapped[16..48], &payload);
    drop(mapped);
    readback.unmap();
}

#[test]
fn portable_invalid_wgsl_rejected_without_driver_shader_compile() {
    let (_, device, _) = portable_any_device();
    let before = device.portable_shader_compilation_count_for_testing();

    let scope = device.push_error_scope(ErrorFilter::Validation);
    let invalid_module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("bad wgsl"),
        source: ShaderSource::Wgsl(
            "@compute @workgroup_size(1) fn main() { let x: u32 = 1.0; }".into(),
        ),
    });
    let err = block_on_test(scope.pop()).expect("ill-typed WGSL must fail in create_shader_module");
    assert!(matches!(err, Error::Validation { .. }), "{err:?}");
    assert_eq!(
        device.portable_shader_compilation_count_for_testing(),
        before,
        "invalid WGSL must never reach the backend shader compiler"
    );

    let pipe_scope = device.push_error_scope(ErrorFilter::Validation);
    let inert_pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some("inert compute"),
        layout: None,
        module: &invalid_module,
        entry_point: Some("main"),
        compilation_options: PipelineCompilationOptions::default(),
        cache: None,
    });
    let pipe_err =
        block_on_test(pipe_scope.pop()).expect("pipeline on invalid module must record validation");
    assert!(matches!(pipe_err, Error::Validation { .. }), "{pipe_err:?}");

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
        pass.set_pipeline(&inert_pipeline);
        pass.dispatch_workgroups(1, 1, 1);
    }
    let _ = encoder.finish();
}

#[test]
fn portable_oob_copy_and_write_texture_rejected_in_validation_scope() {
    let (_, device, queue) = portable_any_device();

    let src = device.create_buffer(&BufferDescriptor {
        label: Some("src"),
        size: 64,
        usage: BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let dst = device.create_buffer(&BufferDescriptor {
        label: Some("dst"),
        size: 64,
        usage: BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let copy_scope = device.push_error_scope(ErrorFilter::Validation);
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(&src, 48, &dst, 0, 32);
    let _ = encoder.finish();
    let copy_err = block_on_test(copy_scope.pop()).expect("OOB buffer copy must be rejected");
    assert!(matches!(copy_err, Error::Validation { .. }), "{copy_err:?}");

    let texture = device.create_texture(&TextureDescriptor {
        label: Some("small tex"),
        size: Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let tex_scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_texture(
        TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &[1u8; 4],
        TexelCopyBufferLayout::default(),
        Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
    );
    let tex_err = block_on_test(tex_scope.pop()).expect("short write_texture payload must fail");
    assert!(matches!(tex_err, Error::Validation { .. }), "{tex_err:?}");
}

#[test]
fn portable_device_destroy_isolates_siblings() {
    let (adapter, dev_a, q_a) = portable_any_device();
    let (dev_b, q_b) = block_on_test(adapter.request_device(&DeviceDescriptor::default()))
        .expect("sibling portable device must succeed");

    let lost_a = Arc::new(Mutex::new(Vec::new()));
    let lost_b = Arc::new(Mutex::new(Vec::new()));
    let tx_a = lost_a.clone();
    let tx_b = lost_b.clone();
    dev_a.set_device_lost_callback(move |reason, msg| {
        tx_a.lock().unwrap().push((reason, msg));
    });
    dev_b.set_device_lost_callback(move |reason, msg| {
        tx_b.lock().unwrap().push((reason, msg));
    });

    dev_a.destroy();
    let events_a = lost_a.lock().unwrap();
    assert_eq!(events_a.len(), 1);
    assert_eq!(events_a[0].0, DeviceLostReason::Destroyed);
    assert!(lost_b.lock().unwrap().is_empty());

    let destroyed_scope = dev_a.push_error_scope(ErrorFilter::Validation);
    q_a.submit(std::iter::empty());
    let destroyed_err =
        block_on_test(destroyed_scope.pop()).expect("submit on destroyed device must fail");
    assert!(
        destroyed_err.to_string().contains("destroyed"),
        "{destroyed_err}"
    );

    let sibling_buf = dev_b.create_buffer(&BufferDescriptor {
        label: Some("sibling buffer"),
        size: 64,
        usage: BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = dev_b.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.clear_buffer(&sibling_buf, 0, None);
    q_b.submit([encoder.finish()]);
    dev_b.poll(PollType::wait_indefinitely()).unwrap();
    assert!(lost_b.lock().unwrap().is_empty());
}

#[test]
fn cuda_adapters_remain_empty_and_env_backend_selection_works() {
    let cuda_instance = Instance::new(InstanceDescriptor {
        backends: Backends::CUDA,
        ..Default::default()
    });
    let cuda_adapters = block_on_test(cuda_instance.enumerate_adapters(Backends::CUDA));
    assert!(
        cuda_adapters.is_empty(),
        "Backends::CUDA must never fabricate wgpu adapters"
    );
    let _driver_present = gpu_runtime::cuda::probe_driver_available();

    let enabled = Instance::enabled_backend_features();
    assert_eq!(
        gpu_runtime::vulkan::SHADER_TARGET,
        gpu_shader::ComputeShaderTarget::VulkanSpirv
    );
    if gpu_runtime::vulkan::is_compiled() {
        assert!(enabled.contains(Backends::VULKAN));
    }
    if cfg!(all(
        feature = "metal",
        any(target_os = "macos", target_os = "ios")
    )) {
        assert!(enabled.contains(Backends::METAL));
    }
    if cfg!(all(feature = "dx12", target_os = "windows")) {
        assert!(enabled.contains(Backends::DX12));
    }
    assert_eq!(
        Backends::from_env_str("vulkan, gl, dx12, metal, webgpu"),
        Some(
            Backends::VULKAN
                | Backends::GL
                | Backends::DX12
                | Backends::METAL
                | Backends::BROWSER_WEBGPU
        )
    );
}

#[test]
fn portable_map_buffer_on_submit_reads_copied_bytes() {
    let Some((_, device, queue)) = portable_hardware_device() else {
        return;
    };

    let source = device.create_buffer(&BufferDescriptor {
        label: Some("portable map_on_submit source"),
        size: 8,
        usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("portable map_on_submit readback"),
        size: 8,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let expected = [17u32, u32::MAX];
    let payload: Vec<u8> = expected.iter().flat_map(|v| v.to_le_bytes()).collect();
    queue.write_buffer(&source, 0, &payload);

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(&source, 0, &readback, 0, 8);
    let done = Arc::new(Mutex::new(None));
    let done_tx = done.clone();
    encoder.map_buffer_on_submit(&readback, MapMode::Read, ..8, move |res| {
        *done_tx.lock().unwrap() = Some(res);
    });
    let idx = queue.submit([encoder.finish()]);
    device
        .poll(PollType::Wait {
            submission_index: Some(idx),
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    assert!(done.lock().unwrap().take().unwrap().is_ok());

    let mapped = readback.slice(..).get_mapped_range();
    let actual: Vec<u32> = mapped
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    assert_eq!(actual, expected);
    drop(mapped);
    readback.unmap();
}
