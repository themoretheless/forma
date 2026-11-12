//! Cached render pipelines for every shipped shader, mirroring the browser
//! renderer's pipeline set (blend, depth compare, vertex layouts).

use crate::chunks::expand_chunks;
use crate::shaders::{
    DEEP_MESH_WGSL, EDGE_WGSL, GRID_WGSL, LINE_WGSL, MESH_SECTION_CAP_WGSL, MESH_SHADOW_WGSL,
    MESH_VERTEX_STRIDE, MESH_WGSL, MORPH_VERTEX_STRIDE, SELECTION_OVERLAY_WGSL,
};
use crate::uniform::OBJECT_UNIFORM_BYTES;
use crate::variants::{VertexOutput, instanced_object_shader};
use wgpu::{
    BindGroupLayout, BlendComponent, ColorTargetState, ColorWrites, CompareFunction, Device,
    FragmentState, PipelineLayout, PrimitiveState, RenderPipeline, RenderPipelineDescriptor,
    ShaderModule, TextureFormat, VertexAttribute, VertexBufferLayout, VertexState,
};

fn alpha_blend() -> BlendComponent {
    BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    }
}

fn transparent_target(format: TextureFormat) -> ColorTargetState {
    ColorTargetState {
        format,
        blend: Some(wgpu::BlendState {
            color: alpha_blend(),
            alpha: BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        }),
        write_mask: ColorWrites::ALL,
    }
}

fn mesh_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: MESH_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
        ],
    }
}

fn morph_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: MORPH_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 2,
        }],
    }
}

fn edge_slot0_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: MESH_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 0,
        }],
    }
}

fn edge_morph_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: MORPH_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 0,
            shader_location: 1,
        }],
    }
}

fn line_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: 28,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 12,
                shader_location: 1,
            },
        ],
    }
}

fn grid_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: 8,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        }],
    }
}

pub struct RasterPipelines {
    pub scene_layout: PipelineLayout,
    pub object_layout: PipelineLayout,
    pub instance_layout: PipelineLayout,
    pub mesh_opaque: RenderPipeline,
    pub mesh_transparent: RenderPipeline,
    /// Stencil-free section cap: mesh_section_cap with front-face culling, so
    /// clipped back faces of closed solids read as a filled cut surface.
    pub section_cap: RenderPipeline,
    pub deep_mesh: RenderPipeline,
    pub edge: RenderPipeline,
    pub deep_edge: RenderPipeline,
    pub line: RenderPipeline,
    pub grid: RenderPipeline,
    pub selection_overlay: RenderPipeline,
    pub instanced_mesh_opaque: RenderPipeline,
    pub instanced_mesh_transparent: RenderPipeline,
    pub instanced_edge: RenderPipeline,
    /// Depth-only key-light shadow pass (mesh_shadow.wgsl): no color targets,
    /// no fragment stage, writes the depth32float shadow map.
    pub shadow: RenderPipeline,
    pub scene_bgl: BindGroupLayout,
    pub object_bgl: BindGroupLayout,
    pub instance_bgl: BindGroupLayout,
    /// Key-light shadow map (depth texture + comparison sampler). Bound by
    /// mesh-surface shaders at group(2) and the grid at group(1); the native
    /// rasterizer binds a 1×1 dummy and keeps `shadow_params` disabled.
    pub shadow_bgl: BindGroupLayout,
    pub format: TextureFormat,
}

fn module(device: &Device, label: &str, source: &str) -> ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

/// A layout with one buffer at binding 0, visible to both stages.
fn buffer_bgl(
    device: &Device,
    label: &str,
    ty: wgpu::BufferBindingType,
    min_binding_size: Option<wgpu::BufferSize>,
) -> BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size,
            },
            count: None,
        }],
    })
}

/// What differs between the raster pipelines; every one runs `vs` and, unless it is depth-only,
/// `fs` of a single module into one color target.
struct PipelineSpec<'a> {
    label: &'a str,
    layout: &'a PipelineLayout,
    module: &'a ShaderModule,
    buffers: &'a [Option<VertexBufferLayout<'static>>],
    /// `None` for a depth-only pass, which has no fragment stage.
    target: Option<ColorTargetState>,
    primitive: PrimitiveState,
    depth: wgpu::DepthStencilState,
}

fn pipeline(device: &Device, spec: PipelineSpec<'_>) -> RenderPipeline {
    let targets = [spec.target];
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: Some(spec.layout),
        vertex: VertexState {
            module: spec.module,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: spec.buffers,
        },
        fragment: targets[0].is_some().then(|| FragmentState {
            module: spec.module,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        primitive: spec.primitive,
        depth_stencil: Some(spec.depth),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl RasterPipelines {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let scene_bgl = buffer_bgl(
            device,
            "raster scene bgl",
            wgpu::BufferBindingType::Uniform,
            None,
        );
        let object_bgl = buffer_bgl(
            device,
            "raster object bgl",
            wgpu::BufferBindingType::Uniform,
            wgpu::BufferSize::new(OBJECT_UNIFORM_BYTES),
        );
        let instance_bgl = buffer_bgl(
            device,
            "raster instance bgl",
            wgpu::BufferBindingType::Storage { read_only: true },
            None,
        );
        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raster shadow bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raster scene layout"),
            bind_group_layouts: &[Some(&scene_bgl), Some(&shadow_bgl)],
            immediate_size: 0,
        });
        // Depth-only shadow pass: scene+object groups only, no shadow group —
        // mirroring the browser renderer's shadowObjectLayout.
        let shadow_object_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raster shadow object layout"),
            bind_group_layouts: &[Some(&scene_bgl), Some(&object_bgl)],
            immediate_size: 0,
        });
        let object_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raster object layout"),
            bind_group_layouts: &[Some(&scene_bgl), Some(&object_bgl), Some(&shadow_bgl)],
            immediate_size: 0,
        });
        let instance_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raster instance layout"),
            bind_group_layouts: &[Some(&scene_bgl), Some(&instance_bgl), Some(&shadow_bgl)],
            immediate_size: 0,
        });

        let depth = wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::Less),
            stencil: wgpu::StencilState,
            bias: wgpu::DepthBiasState::default(),
        };

        let mesh_mod = module(device, "mesh", &expand_chunks(MESH_WGSL));
        let cap_mod = module(
            device,
            "mesh section cap",
            &expand_chunks(MESH_SECTION_CAP_WGSL),
        );
        let deep_mod = module(device, "deep mesh", &expand_chunks(DEEP_MESH_WGSL));
        let edge_mod = module(device, "edge", &expand_chunks(EDGE_WGSL));
        let line_mod = module(device, "line", &expand_chunks(LINE_WGSL));
        let grid_mod = module(device, "grid", &expand_chunks(GRID_WGSL));
        let overlay_mod = module(
            device,
            "selection overlay",
            &expand_chunks(SELECTION_OVERLAY_WGSL),
        );
        let shadow_mod = module(device, "mesh shadow", &expand_chunks(MESH_SHADOW_WGSL));
        let instanced_mesh_mod = module(
            device,
            "instanced mesh",
            &instanced_object_shader(&expand_chunks(MESH_WGSL), VertexOutput::V),
        );
        let instanced_edge_mod = module(
            device,
            "instanced edge",
            &instanced_object_shader(&expand_chunks(EDGE_WGSL), VertexOutput::EdgeV),
        );

        let opaque_target = || {
            Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })
        };
        let mesh_primitive = PrimitiveState {
            cull_mode: None,
            ..PrimitiveState::default()
        };
        let line_primitive = PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            ..PrimitiveState::default()
        };

        let mesh_buffers = [Some(mesh_vbl()), Some(morph_vbl())];
        let edge_buffers = [Some(edge_slot0_vbl()), Some(edge_morph_vbl())];
        let line_buffers = [Some(line_vbl())];
        let grid_buffers = [Some(grid_vbl())];
        // Blended passes test depth without writing it.
        let read_only = |depth_compare| wgpu::DepthStencilState {
            depth_write_enabled: Some(false),
            depth_compare: Some(depth_compare),
            ..depth
        };
        let build = |spec: PipelineSpec<'_>| pipeline(device, spec);

        let mesh_opaque = build(PipelineSpec {
            label: "mesh opaque",
            layout: &object_layout,
            module: &mesh_mod,
            buffers: &mesh_buffers,
            target: opaque_target(),
            primitive: mesh_primitive,
            depth,
        });
        let section_cap = build(PipelineSpec {
            label: "mesh section cap",
            layout: &object_layout,
            module: &cap_mod,
            buffers: &mesh_buffers,
            target: opaque_target(),
            // Front-face culling draws only the back faces: after the clipped
            // surface pass, the surviving interior fragments form the cap.
            primitive: PrimitiveState {
                cull_mode: Some(wgpu::Face::Front),
                ..PrimitiveState::default()
            },
            depth,
        });
        let mesh_transparent = build(PipelineSpec {
            label: "mesh transparent",
            layout: &object_layout,
            module: &mesh_mod,
            buffers: &mesh_buffers,
            target: Some(transparent_target(format)),
            primitive: mesh_primitive,
            depth: read_only(CompareFunction::Less),
        });
        let deep_mesh = build(PipelineSpec {
            label: "deep mesh",
            layout: &object_layout,
            module: &deep_mod,
            buffers: &mesh_buffers,
            target: Some(transparent_target(format)),
            primitive: mesh_primitive,
            depth: read_only(CompareFunction::Always),
        });
        let edge = build(PipelineSpec {
            label: "edge",
            layout: &object_layout,
            module: &edge_mod,
            buffers: &edge_buffers,
            target: Some(transparent_target(format)),
            primitive: line_primitive,
            depth: read_only(CompareFunction::LessEqual),
        });
        let deep_edge = build(PipelineSpec {
            label: "deep edge",
            layout: &object_layout,
            module: &edge_mod,
            buffers: &edge_buffers,
            target: Some(transparent_target(format)),
            primitive: line_primitive,
            depth: read_only(CompareFunction::Always),
        });
        let line = build(PipelineSpec {
            label: "line",
            layout: &scene_layout,
            module: &line_mod,
            buffers: &line_buffers,
            target: Some(transparent_target(format)),
            primitive: line_primitive,
            depth,
        });
        let grid = build(PipelineSpec {
            label: "grid",
            layout: &scene_layout,
            module: &grid_mod,
            buffers: &grid_buffers,
            target: Some(transparent_target(format)),
            primitive: PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..PrimitiveState::default()
            },
            depth,
        });
        let selection_overlay = build(PipelineSpec {
            label: "selection overlay",
            layout: &scene_layout,
            module: &overlay_mod,
            buffers: &line_buffers,
            target: Some(transparent_target(format)),
            primitive: mesh_primitive,
            depth: read_only(CompareFunction::Less),
        });
        // Depth-only shadow-map pass: no color targets, no fragment stage;
        // writes depth32float from the key light's orthographic VP.
        let shadow = build(PipelineSpec {
            label: "mesh shadow",
            layout: &shadow_object_layout,
            module: &shadow_mod,
            buffers: &mesh_buffers,
            target: None,
            primitive: mesh_primitive,
            depth: wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                ..depth
            },
        });
        let instanced_mesh_opaque = build(PipelineSpec {
            label: "instanced mesh opaque",
            layout: &instance_layout,
            module: &instanced_mesh_mod,
            buffers: &mesh_buffers,
            target: opaque_target(),
            primitive: mesh_primitive,
            depth,
        });
        let instanced_mesh_transparent = build(PipelineSpec {
            label: "instanced mesh transparent",
            layout: &instance_layout,
            module: &instanced_mesh_mod,
            buffers: &mesh_buffers,
            target: Some(transparent_target(format)),
            primitive: mesh_primitive,
            depth: read_only(CompareFunction::Less),
        });
        let instanced_edge = build(PipelineSpec {
            label: "instanced edge",
            layout: &instance_layout,
            module: &instanced_edge_mod,
            buffers: &edge_buffers,
            target: Some(transparent_target(format)),
            primitive: line_primitive,
            depth: read_only(CompareFunction::LessEqual),
        });

        Self {
            scene_layout,
            object_layout,
            instance_layout,
            mesh_opaque,
            mesh_transparent,
            section_cap,
            deep_mesh,
            edge,
            deep_edge,
            line,
            grid,
            selection_overlay,
            instanced_mesh_opaque,
            instanced_mesh_transparent,
            instanced_edge,
            shadow,
            scene_bgl,
            object_bgl,
            instance_bgl,
            shadow_bgl,
            format,
        }
    }
}
