//! Shared render and compute pipeline builders and 2D viewport uniform resources.
use crate::wgpu;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

/// Depth format of every depth-tested pass in this crate.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct ViewportUniforms {
    pub viewport: [f32; 2],
    pub _padding: [f32; 2],
}

pub(crate) fn wgsl_module(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

pub(crate) fn compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    entry_point: &str,
) -> wgpu::ComputePipeline {
    let module = wgsl_module(device, label, source);
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some(entry_point),
        compilation_options: Default::default(),
        cache: None,
    })
}

pub(crate) fn viewport_uniform_resources(
    device: &wgpu::Device,
    prefix: &str,
    viewport: [f32; 2],
) -> (wgpu::BindGroupLayout, wgpu::Buffer, wgpu::BindGroup) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{prefix} uniform layout")),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let uniforms = ViewportUniforms {
        viewport,
        _padding: [0.0; 2],
    };
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("{prefix} uniforms")),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("{prefix} uniform bind group")),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    (layout, buffer, bind_group)
}

pub(crate) fn write_viewport_uniforms(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    viewport: [f32; 2],
) -> [f32; 2] {
    let clamped = [viewport[0].max(1.0), viewport[1].max(1.0)];
    let uniforms = ViewportUniforms {
        viewport: clamped,
        _padding: [0.0; 2],
    };
    queue.write_buffer(buffer, 0, bytemuck::bytes_of(&uniforms));
    clamped
}

pub(crate) fn write_growing_vertex_buffer<T: Pod>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &mut wgpu::Buffer,
    capacity: &mut u64,
    label: &str,
    vertices: &[T],
) -> u32 {
    if vertices.is_empty() {
        return 0;
    }
    let bytes = bytemuck::cast_slice::<T, u8>(vertices);
    if bytes.len() as u64 > *capacity {
        *capacity = (bytes.len() as u64).next_power_of_two();
        *buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: *capacity,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
    }
    queue.write_buffer(buffer, 0, bytes);
    u32::try_from(vertices.len()).unwrap_or(u32::MAX)
}

/// Less-than depth test against a [`DEPTH_FORMAT`] attachment.
pub(crate) fn depth_test(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

/// Writes every channel of `format`, optionally blended.
pub(crate) fn color_target(
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    }
}

/// The parts that differ between this crate's render pipelines.
pub(crate) struct PipelineSpec<'a> {
    pub label: &'a str,
    pub layout: Option<&'a wgpu::PipelineLayout>,
    pub module: &'a wgpu::ShaderModule,
    pub buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub primitive: wgpu::PrimitiveState,
    pub depth: Option<wgpu::DepthStencilState>,
    pub target: wgpu::ColorTargetState,
}

/// Builds a single-target `vs_main` + `fs_main` render pipeline.
pub(crate) fn render_pipeline(device: &wgpu::Device, spec: PipelineSpec) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: spec.layout,
        vertex: wgpu::VertexState {
            module: spec.module,
            entry_point: Some("vs_main"),
            buffers: spec.buffers,
            compilation_options: Default::default(),
        },
        primitive: spec.primitive,
        depth_stencil: spec.depth,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: spec.module,
            entry_point: Some("fs_main"),
            targets: &[Some(spec.target)],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}
