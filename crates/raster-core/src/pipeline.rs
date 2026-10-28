//! Cached render pipelines for every shipped shader, mirroring the browser
//! renderer's pipeline set (blend, depth compare, vertex layouts).

use crate::chunks::expand_chunks;
use crate::shaders::{
    DEEP_MESH_WGSL, EDGE_WGSL, GRID_WGSL, LINE_WGSL, MESH_SECTION_CAP_WGSL, MESH_SHADOW_WGSL,
    MESH_VERTEX_STRIDE, MORPH_VERTEX_STRIDE, MESH_WGSL, SELECTION_OVERLAY_WGSL,
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
            VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
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
        attributes: &[VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }],
    }
}

fn edge_morph_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: MORPH_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 1 }],
    }
}

fn line_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: 28,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 12, shader_location: 1 },
        ],
    }
}

fn grid_vbl() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: 8,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 0, shader_location: 0 }],
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

impl RasterPipelines {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raster scene bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let object_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raster object bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(OBJECT_UNIFORM_BYTES),
                },
                count: None,
            }],
        });
        let instance_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raster instance bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
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
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        };

        let mesh_mod = module(device, "mesh", &expand_chunks(MESH_WGSL));
        let cap_mod = module(device, "mesh section cap", &expand_chunks(MESH_SECTION_CAP_WGSL));
        let deep_mod = module(device, "deep mesh", &expand_chunks(DEEP_MESH_WGSL));
        let edge_mod = module(device, "edge", &expand_chunks(EDGE_WGSL));
        let line_mod = module(device, "line", &expand_chunks(LINE_WGSL));
        let grid_mod = module(device, "grid", &expand_chunks(GRID_WGSL));
        let overlay_mod = module(device, "selection overlay", &expand_chunks(SELECTION_OVERLAY_WGSL));
        let shadow_mod = module(device, "mesh shadow", &expand_chunks(MESH_SHADOW_WGSL));
        let instanced_mesh_mod = module(device, "instanced mesh", &instanced_object_shader(&expand_chunks(MESH_WGSL), VertexOutput::V));
        let instanced_edge_mod = module(device, "instanced edge", &instanced_object_shader(&expand_chunks(EDGE_WGSL), VertexOutput::EdgeV));

        let opaque_target = || Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL });
        let mesh_primitive = PrimitiveState { cull_mode: None, ..PrimitiveState::default() };
        let line_primitive = PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..PrimitiveState::default() };

        let mesh_opaque = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("mesh opaque"),
            layout: Some(&object_layout),
            vertex: VertexState {
                module: &mesh_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vbl()), Some(morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &mesh_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[opaque_target()],
            }),
            primitive: mesh_primitive,
            depth_stencil: Some(depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let section_cap = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("mesh section cap"),
            layout: Some(&object_layout),
            vertex: VertexState {
                module: &cap_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vbl()), Some(morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &cap_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[opaque_target()],
            }),
            // Front-face culling draws only the back faces: after the clipped
            // surface pass, the surviving interior fragments form the cap.
            primitive: PrimitiveState { cull_mode: Some(wgpu::Face::Front), ..PrimitiveState::default() },
            depth_stencil: Some(depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let mesh_transparent = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("mesh transparent"),
            layout: Some(&object_layout),
            vertex: VertexState {
                module: &mesh_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vbl()), Some(morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &mesh_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: mesh_primitive,
            depth_stencil: Some(wgpu::DepthStencilState { depth_write_enabled: Some(false), ..depth.clone() }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let deep_mesh = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("deep mesh"),
            layout: Some(&object_layout),
            vertex: VertexState {
                module: &deep_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vbl()), Some(morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &deep_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: mesh_primitive,
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::Always),
                ..depth.clone()
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let edge = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("edge"),
            layout: Some(&object_layout),
            vertex: VertexState {
                module: &edge_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(edge_slot0_vbl()), Some(edge_morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &edge_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: line_primitive,
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::LessEqual),
                ..depth.clone()
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let deep_edge = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("deep edge"),
            layout: Some(&object_layout),
            vertex: VertexState {
                module: &edge_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(edge_slot0_vbl()), Some(edge_morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &edge_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: line_primitive,
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::Always),
                ..depth.clone()
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let line = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("line"),
            layout: Some(&scene_layout),
            vertex: VertexState {
                module: &line_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(line_vbl())],
            },
            fragment: Some(FragmentState {
                module: &line_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: line_primitive,
            depth_stencil: Some(depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let grid = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grid"),
            layout: Some(&scene_layout),
            vertex: VertexState {
                module: &grid_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(grid_vbl())],
            },
            fragment: Some(FragmentState {
                module: &grid_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..PrimitiveState::default() },
            depth_stencil: Some(depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let selection_overlay = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("selection overlay"),
            layout: Some(&scene_layout),
            vertex: VertexState {
                module: &overlay_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(line_vbl())],
            },
            fragment: Some(FragmentState {
                module: &overlay_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: mesh_primitive,
            depth_stencil: Some(wgpu::DepthStencilState { depth_write_enabled: Some(false), ..depth.clone() }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // Depth-only shadow-map pass: no color targets, no fragment stage;
        // writes depth32float from the key light's orthographic VP.
        let shadow = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("mesh shadow"),
            layout: Some(&shadow_object_layout),
            vertex: VertexState {
                module: &shadow_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vbl()), Some(morph_vbl())],
            },
            fragment: None,
            primitive: mesh_primitive,
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let instanced_mesh_opaque = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("instanced mesh opaque"),
            layout: Some(&instance_layout),
            vertex: VertexState {
                module: &instanced_mesh_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vbl()), Some(morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &instanced_mesh_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[opaque_target()],
            }),
            primitive: mesh_primitive,
            depth_stencil: Some(depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let instanced_mesh_transparent = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("instanced mesh transparent"),
            layout: Some(&instance_layout),
            vertex: VertexState {
                module: &instanced_mesh_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(mesh_vbl()), Some(morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &instanced_mesh_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: mesh_primitive,
            depth_stencil: Some(wgpu::DepthStencilState { depth_write_enabled: Some(false), ..depth.clone() }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let instanced_edge = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("instanced edge"),
            layout: Some(&instance_layout),
            vertex: VertexState {
                module: &instanced_edge_mod,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(edge_slot0_vbl()), Some(edge_morph_vbl())],
            },
            fragment: Some(FragmentState {
                module: &instanced_edge_mod,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(transparent_target(format))],
            }),
            primitive: line_primitive,
            depth_stencil: Some(wgpu::DepthStencilState {
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::LessEqual),
                ..depth
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
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
