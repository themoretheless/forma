//! A compute shader writes a storage texture and the texels come back as written.
use super::*;

const FILL: &str = "
@group(0) @binding(0) var target: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(8, 8)
fn run(@builtin(global_invocation_id) id: vec3<u32>) {
    textureStore(target, id.xy, vec4<f32>(f32(id.x) / 255.0, f32(id.y) / 255.0, 0.5, 1.0));
}
";

fn block_on_ready<T>(fut: impl Future<Output = T>) -> T {
    let mut pinned = std::pin::pin!(fut);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match pinned.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(val) => val,
        std::task::Poll::Pending => panic!("expected immediate future completion"),
    }
}

#[test]
fn a_compute_shader_fills_a_storage_texture() {
    let Some(metal) = MetalDevice::create_system_default() else {
        return;
    };
    let adapter = Adapter::from_metal(metal);
    let (device, queue) = block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
        .expect("device creation must succeed");
    let scope = device.push_error_scope(ErrorFilter::Validation);

    const SIZE: u32 = 8;
    let extent = Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("storage target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(FILL.into()),
    });
    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("run"),
        compilation_options: PipelineCompilationOptions::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(&view),
        }],
    });
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("readback"),
        size: u64::from(SIZE * SIZE * 4),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // The shader is compiled by Metal when the pipeline is created.
    assert!(block_on_ready(scope.pop()).is_none());

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
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
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        extent,
    );
    queue.submit(Some(encoder.finish()));

    let texels = readback.uma_slice();
    for (index, texel) in texels.chunks_exact(4).enumerate() {
        let (x, y) = (index as u32 % SIZE, index as u32 / SIZE);
        assert_eq!(texel, [x as u8, y as u8, 128, 255], "texel {x},{y}");
    }
}

const DIMENSIONS: &str = "
@group(0) @binding(0) var line: texture_storage_1d<r32float, write>;
@group(0) @binding(1) var volume: texture_3d<f32>;
@group(0) @binding(2) var counts: texture_storage_2d<r32uint, read>;
@compute @workgroup_size(1)
fn run(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(volume);
    let count = textureLoad(counts, id.xy).x;
    let width = textureDimensions(line);
    textureStore(line, id.x % width, vec4<f32>(f32(size.z + count), 0.0, 0.0, 1.0));
}
";

#[test]
fn metal_compiles_dimension_queries_and_typed_storage_reads() {
    let Some(metal) = MetalDevice::create_system_default() else {
        return;
    };
    let adapter = Adapter::from_metal(metal);
    let (device, _queue) = block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
        .expect("device creation must succeed");
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(DIMENSIONS.into()),
    });
    let _pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("run"),
        compilation_options: PipelineCompilationOptions::default(),
        cache: None,
    });
    let error = block_on_ready(scope.pop());
    assert!(error.is_none(), "{error:?}");
}

const COUNT: &str = "
@group(0) @binding(0) var target: texture_storage_2d<r32uint, write>;
@compute @workgroup_size(8, 8)
fn run(@builtin(global_invocation_id) id: vec3<u32>) {
    textureStore(target, id.xy, vec4<u32>(id.x + id.y * 1000u, 0u, 0u, 0u));
}
";

#[test]
fn a_compute_shader_fills_an_unsigned_integer_storage_texture() {
    let Some(metal) = MetalDevice::create_system_default() else {
        return;
    };
    let adapter = Adapter::from_metal(metal);
    let (device, queue) = block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
        .expect("device creation must succeed");
    let scope = device.push_error_scope(ErrorFilter::Validation);

    const SIZE: u32 = 8;
    let extent = Extent3d {
        width: SIZE,
        height: SIZE,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("integer storage target"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::R32Uint,
        usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    assert_eq!(TextureFormat::R32Uint.bytes_per_pixel(), 4);
    let view = texture.create_view(&TextureViewDescriptor::default());
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(COUNT.into()),
    });
    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("run"),
        compilation_options: PipelineCompilationOptions::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(&view),
        }],
    });
    let readback = device.create_buffer(&BufferDescriptor {
        label: Some("readback"),
        size: u64::from(SIZE * SIZE * 4),
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let error = block_on_ready(scope.pop());
    assert!(error.is_none(), "{error:?}");

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
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
                bytes_per_row: Some(SIZE * 4),
                rows_per_image: Some(SIZE),
            },
        },
        extent,
    );
    queue.submit(Some(encoder.finish()));

    let texels = readback.uma_slice();
    for (index, texel) in texels.chunks_exact(4).enumerate() {
        let (x, y) = (index as u32 % SIZE, index as u32 / SIZE);
        assert_eq!(texel, (x + y * 1000).to_le_bytes(), "texel {x},{y}");
    }
}
