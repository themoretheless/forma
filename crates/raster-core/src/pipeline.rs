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

const MESH_SLOT0: VertexBufferLayout<'static> = VertexBufferLayout {
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
};
const MORPH_SLOT1: VertexBufferLayout<'static> = VertexBufferLayout {
    array_stride: MORPH_VERTEX_STRIDE,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &[VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 2,
    }],
};
const EDGE_SLOT0: VertexBufferLayout<'static> = VertexBufferLayout {
    array_stride: MESH_VERTEX_STRIDE,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &[VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    }],
};
const EDGE_MORPH_SLOT1: VertexBufferLayout<'static> = VertexBufferLayout {
    array_stride: MORPH_VERTEX_STRIDE,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &[VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 1,
    }],
};
const LINE_SLOT0: VertexBufferLayout<'static> = VertexBufferLayout {
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
};
const GRID_SLOT0: VertexBufferLayout<'static> = VertexBufferLayout {
    array_stride: 8,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &[VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 0,
        shader_location: 0,
    }],
};

const MESH_BUFFERS: [Option<VertexBufferLayout<'static>>; 2] =
    [Some(MESH_SLOT0), Some(MORPH_SLOT1)];
const EDGE_BUFFERS: [Option<VertexBufferLayout<'static>>; 2] =
    [Some(EDGE_SLOT0), Some(EDGE_MORPH_SLOT1)];
const LINE_BUFFERS: [Option<VertexBufferLayout<'static>>; 1] = [Some(LINE_SLOT0)];
const GRID_BUFFERS: [Option<VertexBufferLayout<'static>>; 1] = [Some(GRID_SLOT0)];

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

#[derive(Clone, Copy)]
enum LayoutId {
    Scene,
    Object,
    Instance,
    ShadowObject,
}

#[derive(Clone, Copy)]
enum ModuleId {
    Mesh = 0,
    Cap = 1,
    Deep = 2,
    Edge = 3,
    Line = 4,
    Grid = 5,
    Overlay = 6,
    Shadow = 7,
    InstancedMesh = 8,
    InstancedEdge = 9,
}

#[derive(Clone, Copy)]
enum BufferSet {
    Mesh,
    Edge,
    Line,
    Grid,
}

#[derive(Clone, Copy)]
enum TargetMode {
    Opaque,
    Transparent,
    DepthOnly,
}

#[derive(Clone, Copy)]
enum DepthMode {
    WriteLess,
    ReadLess,
    ReadLessEqual,
    ReadAlways,
    ShadowWriteLess,
}

#[derive(Clone, Copy)]
enum ShaderSpec {
    Expanded(&'static str, &'static str),
    Instanced(&'static str, &'static str, VertexOutput),
}

const SHADERS: [ShaderSpec; 10] = [
    ShaderSpec::Expanded("mesh", MESH_WGSL),
    ShaderSpec::Expanded("mesh section cap", MESH_SECTION_CAP_WGSL),
    ShaderSpec::Expanded("deep mesh", DEEP_MESH_WGSL),
    ShaderSpec::Expanded("edge", EDGE_WGSL),
    ShaderSpec::Expanded("line", LINE_WGSL),
    ShaderSpec::Expanded("grid", GRID_WGSL),
    ShaderSpec::Expanded("selection overlay", SELECTION_OVERLAY_WGSL),
    ShaderSpec::Expanded("mesh shadow", MESH_SHADOW_WGSL),
    ShaderSpec::Instanced("instanced mesh", MESH_WGSL, VertexOutput::V),
    ShaderSpec::Instanced("instanced edge", EDGE_WGSL, VertexOutput::EdgeV),
];

#[derive(Clone, Copy)]
struct PipelineRow {
    label: &'static str,
    layout: LayoutId,
    module: ModuleId,
    buffers: BufferSet,
    target: TargetMode,
    cull_mode: Option<wgpu::Face>,
    topology: wgpu::PrimitiveTopology,
    depth: DepthMode,
}

const PIPELINE_TABLE: [PipelineRow; 13] = [
    PipelineRow {
        label: "mesh opaque",
        layout: LayoutId::Object,
        module: ModuleId::Mesh,
        buffers: BufferSet::Mesh,
        target: TargetMode::Opaque,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::WriteLess,
    },
    PipelineRow {
        label: "mesh transparent",
        layout: LayoutId::Object,
        module: ModuleId::Mesh,
        buffers: BufferSet::Mesh,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::ReadLess,
    },
    PipelineRow {
        label: "mesh section cap",
        layout: LayoutId::Object,
        module: ModuleId::Cap,
        buffers: BufferSet::Mesh,
        target: TargetMode::Opaque,
        // Front-face culling draws back faces so surviving interior fragments form the cap.
        cull_mode: Some(wgpu::Face::Front),
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::WriteLess,
    },
    PipelineRow {
        label: "deep mesh",
        layout: LayoutId::Object,
        module: ModuleId::Deep,
        buffers: BufferSet::Mesh,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::ReadAlways,
    },
    PipelineRow {
        label: "edge",
        layout: LayoutId::Object,
        module: ModuleId::Edge,
        buffers: BufferSet::Edge,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::LineList,
        depth: DepthMode::ReadLessEqual,
    },
    PipelineRow {
        label: "deep edge",
        layout: LayoutId::Object,
        module: ModuleId::Edge,
        buffers: BufferSet::Edge,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::LineList,
        depth: DepthMode::ReadAlways,
    },
    PipelineRow {
        label: "line",
        layout: LayoutId::Scene,
        module: ModuleId::Line,
        buffers: BufferSet::Line,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::LineList,
        depth: DepthMode::WriteLess,
    },
    PipelineRow {
        label: "grid",
        layout: LayoutId::Scene,
        module: ModuleId::Grid,
        buffers: BufferSet::Grid,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::WriteLess,
    },
    PipelineRow {
        label: "selection overlay",
        layout: LayoutId::Scene,
        module: ModuleId::Overlay,
        buffers: BufferSet::Line,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::ReadLess,
    },
    PipelineRow {
        label: "instanced mesh opaque",
        layout: LayoutId::Instance,
        module: ModuleId::InstancedMesh,
        buffers: BufferSet::Mesh,
        target: TargetMode::Opaque,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::WriteLess,
    },
    PipelineRow {
        label: "instanced mesh transparent",
        layout: LayoutId::Instance,
        module: ModuleId::InstancedMesh,
        buffers: BufferSet::Mesh,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::ReadLess,
    },
    PipelineRow {
        label: "instanced edge",
        layout: LayoutId::Instance,
        module: ModuleId::InstancedEdge,
        buffers: BufferSet::Edge,
        target: TargetMode::Transparent,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::LineList,
        depth: DepthMode::ReadLessEqual,
    },
    PipelineRow {
        label: "mesh shadow",
        layout: LayoutId::ShadowObject,
        module: ModuleId::Shadow,
        buffers: BufferSet::Mesh,
        target: TargetMode::DepthOnly,
        cull_mode: None,
        topology: wgpu::PrimitiveTopology::TriangleList,
        depth: DepthMode::ShadowWriteLess,
    },
];

struct Layouts {
    scene_bgl: BindGroupLayout,
    object_bgl: BindGroupLayout,
    instance_bgl: BindGroupLayout,
    shadow_bgl: BindGroupLayout,
    scene_layout: PipelineLayout,
    object_layout: PipelineLayout,
    instance_layout: PipelineLayout,
    shadow_object_layout: PipelineLayout,
}

impl Layouts {
    fn new(device: &Device) -> Self {
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
        Self {
            scene_bgl,
            object_bgl,
            instance_bgl,
            shadow_bgl,
            scene_layout,
            object_layout,
            instance_layout,
            shadow_object_layout,
        }
    }

    fn layout(&self, id: LayoutId) -> &PipelineLayout {
        match id {
            LayoutId::Scene => &self.scene_layout,
            LayoutId::Object => &self.object_layout,
            LayoutId::Instance => &self.instance_layout,
            LayoutId::ShadowObject => &self.shadow_object_layout,
        }
    }
}

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

fn compile_modules(device: &Device) -> [ShaderModule; 10] {
    SHADERS.map(|spec| {
        let (label, source) = match spec {
            ShaderSpec::Expanded(label, wgsl) => (label, expand_chunks(wgsl)),
            ShaderSpec::Instanced(label, wgsl, output) => (
                label,
                std::borrow::Cow::Owned(instanced_object_shader(&expand_chunks(wgsl), output)),
            ),
        };
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source),
        })
    })
}

fn color_target(format: TextureFormat, mode: TargetMode) -> Option<ColorTargetState> {
    match mode {
        TargetMode::Opaque => Some(ColorTargetState {
            format,
            blend: None,
            write_mask: ColorWrites::ALL,
        }),
        TargetMode::Transparent => Some(ColorTargetState {
            format,
            blend: Some(wgpu::BlendState {
                color: BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            write_mask: ColorWrites::ALL,
        }),
        TargetMode::DepthOnly => None,
    }
}

fn depth_state(mode: DepthMode) -> wgpu::DepthStencilState {
    let (format, write, compare) = match mode {
        DepthMode::WriteLess => (
            wgpu::TextureFormat::Depth24Plus,
            true,
            CompareFunction::Less,
        ),
        DepthMode::ReadLess => (
            wgpu::TextureFormat::Depth24Plus,
            false,
            CompareFunction::Less,
        ),
        DepthMode::ReadLessEqual => (
            wgpu::TextureFormat::Depth24Plus,
            false,
            CompareFunction::LessEqual,
        ),
        DepthMode::ReadAlways => (
            wgpu::TextureFormat::Depth24Plus,
            false,
            CompareFunction::Always,
        ),
        DepthMode::ShadowWriteLess => (
            wgpu::TextureFormat::Depth32Float,
            true,
            CompareFunction::Less,
        ),
    };
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: wgpu::StencilState,
        bias: wgpu::DepthBiasState::default(),
    }
}

fn build_pipeline(
    device: &Device,
    format: TextureFormat,
    layouts: &Layouts,
    modules: &[ShaderModule; 10],
    row: PipelineRow,
) -> RenderPipeline {
    let buffers: &[Option<VertexBufferLayout<'static>>] = match row.buffers {
        BufferSet::Mesh => &MESH_BUFFERS,
        BufferSet::Edge => &EDGE_BUFFERS,
        BufferSet::Line => &LINE_BUFFERS,
        BufferSet::Grid => &GRID_BUFFERS,
    };
    let module = &modules[row.module as usize];
    let targets = [color_target(format, row.target)];
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(row.label),
        layout: Some(layouts.layout(row.layout)),
        vertex: VertexState {
            module,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers,
        },
        fragment: targets[0].is_some().then(|| FragmentState {
            module,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        primitive: PrimitiveState {
            topology: row.topology,
            cull_mode: row.cull_mode,
            ..PrimitiveState::default()
        },
        depth_stencil: Some(depth_state(row.depth)),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl RasterPipelines {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let layouts = Layouts::new(device);
        let modules = compile_modules(device);
        let [
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
        ] = PIPELINE_TABLE.map(|row| build_pipeline(device, format, &layouts, &modules, row));
        Self {
            scene_layout: layouts.scene_layout,
            object_layout: layouts.object_layout,
            instance_layout: layouts.instance_layout,
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
            scene_bgl: layouts.scene_bgl,
            object_bgl: layouts.object_bgl,
            instance_bgl: layouts.instance_bgl,
            shadow_bgl: layouts.shadow_bgl,
            format,
        }
    }
}
