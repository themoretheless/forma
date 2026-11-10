//! Render pipelines shared by the model, material and mip passes: one WGSL module with `vs_main`
//! and `fs_main`, one colour target and a bind group layout derived from the shader.
use crate::wgpu;

/// Depth format of every depth-tested pass in this crate.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub(crate) fn wgsl_module(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
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
    pub module: &'a wgpu::ShaderModule,
    pub buffers: &'a [Option<wgpu::VertexBufferLayout<'a>>],
    pub primitive: wgpu::PrimitiveState,
    pub depth: Option<wgpu::DepthStencilState>,
    pub target: wgpu::ColorTargetState,
}

/// Builds the pipeline with `layout: None`; bind groups use `get_bind_group_layout`.
pub(crate) fn render_pipeline(device: &wgpu::Device, spec: PipelineSpec) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: None,
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
