//! Immediate data set in pieces reaches the shader as one block.
use super::*;

const SHADER: &str = "
struct Params {
    first: vec4<f32>,
    second: vec4<f32>,
}
var<immediate> params: Params;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}
@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return params.first + params.second;
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

fn floats(values: [f32; 4]) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

/// Draws one pixel after `set` has provided the immediate data and returns its colour.
fn pixel(set: impl FnOnce(&mut RenderPass<'_>)) -> Option<Vec<u8>> {
    let adapter = Adapter::from_metal(MetalDevice::create_system_default()?);
    let (device, queue) = block_on_ready(adapter.request_device(&DeviceDescriptor::default()))
        .expect("device creation must succeed");
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let extent = Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let target = device.create_texture(&TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&TextureViewDescriptor::default());
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    });
    let error = block_on_ready(scope.pop());
    assert!(error.is_none(), "{error:?}");

    let readback = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 4,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        set(&mut pass);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &readback,
            layout: TexelCopyBufferLayout::default(),
        },
        extent,
    );
    queue.submit(Some(encoder.finish()));
    let colour = readback.uma_slice().to_vec();
    Some(colour)
}

#[test]
fn one_block_of_immediates_reaches_the_shader() {
    let mut data = floats([0.25, 0.0, 0.0, 0.0]);
    data.extend(floats([0.0, 0.5, 0.0, 1.0]));
    let Some(colour) = pixel(|pass| pass.set_immediates(0, &data)) else {
        return;
    };
    assert_eq!(colour, [64, 128, 0, 255]);
}

#[test]
fn immediates_set_at_an_offset_keep_the_bytes_before_them() {
    let first = floats([0.25, 0.0, 0.0, 0.0]);
    let second = floats([0.0, 0.5, 0.0, 1.0]);
    let Some(colour) = pixel(|pass| {
        pass.set_immediates(0, &first);
        pass.set_immediates(16, &second);
    }) else {
        return;
    };
    assert_eq!(colour, [64, 128, 0, 255]);
}
