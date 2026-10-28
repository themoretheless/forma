//! GPU ShaderModule, BindGroup, PipelineLayout, ComputePipeline, and RenderPipeline descriptors & states.

use crate::metal::ObjCObject;
use crate::{
    Buffer, BufferAddress, BufferSize, ColorWrites, Label, Sampler, ShaderStages, Texture,
    TextureFormat, TextureView,
};
use gpu_shader as naga;
use gpu_shader::back::msl::BindingMap;
use std::borrow::Cow;
use std::num::NonZeroU32;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum ShaderSource<'a> {
    Wgsl(Cow<'a, str>),
}

#[derive(Debug, Clone)]
pub struct ShaderModuleDescriptor<'a> {
    pub label: Label<'a>,
    pub source: ShaderSource<'a>,
}

#[derive(Clone)]
pub struct ShaderModule {
    pub(crate) module: Arc<naga::Module>,
    pub(crate) info: Arc<naga::valid::ModuleInfo>,
    pub(crate) msl_source: Arc<String>,
    pub(crate) bindings: Arc<BindingMap>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferBindingType {
    Uniform,
    Storage { read_only: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplerBindingType {
    Filtering,
    NonFiltering,
    Comparison,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureSampleType {
    Float { filterable: bool },
    Depth,
    Sint,
    Uint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureViewDimension {
    D1,
    #[default]
    D2,
    D2Array,
    Cube,
    CubeArray,
    D3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTextureAccess {
    WriteOnly,
    ReadOnly,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingType {
    Buffer {
        ty: BufferBindingType,
        has_dynamic_offset: bool,
        min_binding_size: Option<BufferSize>,
    },
    Sampler(SamplerBindingType),
    Texture {
        sample_type: TextureSampleType,
        view_dimension: TextureViewDimension,
        multisampled: bool,
    },
    StorageTexture {
        access: StorageTextureAccess,
        format: TextureFormat,
        view_dimension: TextureViewDimension,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: ShaderStages,
    pub ty: BindingType,
    pub count: Option<NonZeroU32>,
}

#[derive(Debug, Clone)]
pub struct BindGroupLayoutDescriptor<'a> {
    pub label: Label<'a>,
    pub entries: &'a [BindGroupLayoutEntry],
}

#[derive(Clone, Debug)]
pub struct BindGroupLayout {
    pub(crate) entries: Vec<BindGroupLayoutEntry>,
}

#[derive(Clone)]
pub struct BufferBinding<'a> {
    pub buffer: &'a Buffer,
    pub offset: BufferAddress,
    pub size: Option<BufferSize>,
}

#[derive(Clone)]
pub enum BindingResource<'a> {
    Buffer(BufferBinding<'a>),
    Sampler(&'a Sampler),
    TextureView(&'a TextureView),
}

#[derive(Clone)]
pub struct BindGroupEntry<'a> {
    pub binding: u32,
    pub resource: BindingResource<'a>,
}

#[derive(Clone)]
pub struct BindGroupDescriptor<'a> {
    pub label: Label<'a>,
    pub layout: &'a BindGroupLayout,
    pub entries: &'a [BindGroupEntry<'a>],
}

#[derive(Clone)]
pub struct BindGroup {
    pub(crate) buffers: Vec<(u32, Buffer, u64, u64)>,
    pub(crate) textures: Vec<(u32, Texture)>,
    pub(crate) samplers: Vec<(u32, Sampler)>,
}

#[derive(Debug, Clone)]
pub struct PipelineLayoutDescriptor<'a> {
    pub label: Label<'a>,
    pub bind_group_layouts: &'a [Option<&'a BindGroupLayout>],
    pub immediate_size: u32,
}

impl Default for PipelineLayoutDescriptor<'_> {
    fn default() -> Self {
        Self {
            label: None,
            bind_group_layouts: &[],
            immediate_size: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PipelineLayout {
    pub(crate) immediate_size: u32,
}

#[derive(Debug, Clone, Default)]
pub struct PipelineCompilationOptions<'a> {
    pub constants: &'a [(&'a str, f64)],
    pub zero_initialize_workgroup_memory: bool,
}

#[derive(Clone)]
pub struct ComputePipelineDescriptor<'a> {
    pub label: Label<'a>,
    pub layout: Option<&'a PipelineLayout>,
    pub module: &'a ShaderModule,
    pub entry_point: Option<&'a str>,
    pub compilation_options: PipelineCompilationOptions<'a>,
    pub cache: Option<&'a ()>,
}

#[derive(Clone)]
pub struct ComputePipeline {
    pub(crate) pso: Arc<ObjCObject>,
    pub(crate) workgroup_size: [u32; 3],
    pub(crate) bindings: Arc<BindingMap>,
}

impl ComputePipeline {
    pub fn get_bind_group_layout(&self, _index: u32) -> BindGroupLayout {
        BindGroupLayout {
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VertexStepMode {
    #[default]
    Vertex,
    Instance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Uint32,
    Uint32x2,
    Uint32x3,
    Uint32x4,
    Sint32,
    Sint32x2,
    Sint32x3,
    Sint32x4,
}

impl VertexFormat {
    pub(crate) const fn to_mtl(self) -> u64 {
        match self {
            Self::Float32 => 28,   // MTLVertexFormatFloat
            Self::Float32x2 => 29, // MTLVertexFormatFloat2
            Self::Float32x3 => 30, // MTLVertexFormatFloat3
            Self::Float32x4 => 31, // MTLVertexFormatFloat4
            Self::Sint32 => 32,
            Self::Sint32x2 => 33,
            Self::Sint32x3 => 34,
            Self::Sint32x4 => 35,
            Self::Uint32 => 36,
            Self::Uint32x2 => 37,
            Self::Uint32x3 => 38,
            Self::Uint32x4 => 39,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VertexAttribute {
    pub format: VertexFormat,
    pub offset: BufferAddress,
    pub shader_location: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct VertexBufferLayout<'a> {
    pub array_stride: BufferAddress,
    pub step_mode: VertexStepMode,
    pub attributes: &'a [VertexAttribute],
}

#[derive(Clone)]
pub struct VertexState<'a> {
    pub module: &'a ShaderModule,
    pub entry_point: Option<&'a str>,
    pub compilation_options: PipelineCompilationOptions<'a>,
    pub buffers: &'a [Option<VertexBufferLayout<'a>>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendFactor {
    #[default]
    Zero,
    One,
    Src,
    OneMinusSrc,
    SrcAlpha,
    OneMinusSrcAlpha,
    Dst,
    OneMinusDst,
    DstAlpha,
    OneMinusDstAlpha,
}

impl BlendFactor {
    pub(crate) const fn to_mtl(self) -> u64 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Src => 2,
            Self::OneMinusSrc => 3,
            Self::SrcAlpha => 4,
            Self::OneMinusSrcAlpha => 5,
            Self::Dst => 6,
            Self::OneMinusDst => 7,
            Self::DstAlpha => 8,
            Self::OneMinusDstAlpha => 9,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendOperation {
    #[default]
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

impl BlendOperation {
    pub(crate) const fn to_mtl(self) -> u64 {
        match self {
            Self::Add => 0,
            Self::Subtract => 1,
            Self::ReverseSubtract => 2,
            Self::Min => 3,
            Self::Max => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlendComponent {
    pub src_factor: BlendFactor,
    pub dst_factor: BlendFactor,
    pub operation: BlendOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlendState {
    pub color: BlendComponent,
    pub alpha: BlendComponent,
}

impl BlendState {
    pub const ALPHA_BLENDING: Self = Self {
        color: BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
    };
}

#[derive(Debug, Clone)]
pub struct ColorTargetState {
    pub format: TextureFormat,
    pub blend: Option<BlendState>,
    pub write_mask: ColorWrites,
}

#[derive(Clone)]
pub struct FragmentState<'a> {
    pub module: &'a ShaderModule,
    pub entry_point: Option<&'a str>,
    pub compilation_options: PipelineCompilationOptions<'a>,
    pub targets: &'a [Option<ColorTargetState>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PrimitiveTopology {
    PointList,
    LineList,
    LineStrip,
    #[default]
    TriangleList,
    TriangleStrip,
}

impl PrimitiveTopology {
    pub(crate) const fn to_mtl(self) -> u64 {
        match self {
            Self::PointList => 0,
            Self::LineList => 1,
            Self::LineStrip => 2,
            Self::TriangleList => 3,
            Self::TriangleStrip => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IndexFormat {
    Uint16,
    #[default]
    Uint32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FrontFace {
    #[default]
    Ccw,
    Cw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Front,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PolygonMode {
    #[default]
    Fill,
    Line,
    Point,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PrimitiveState {
    pub topology: PrimitiveTopology,
    pub strip_index_format: Option<IndexFormat>,
    pub front_face: FrontFace,
    pub cull_mode: Option<Face>,
    pub unclipped_depth: bool,
    pub polygon_mode: PolygonMode,
    pub conservative: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CompareFunction {
    Never = 1,
    Less = 2,
    Equal = 3,
    #[default]
    LessEqual = 4,
    Greater = 5,
    NotEqual = 6,
    GreaterEqual = 7,
    Always = 8,
}

impl CompareFunction {
    pub(crate) const fn to_mtl(self) -> u64 {
        match self {
            Self::Never => 0,
            Self::Less => 1,
            Self::Equal => 2,
            Self::LessEqual => 3,
            Self::Greater => 4,
            Self::NotEqual => 5,
            Self::GreaterEqual => 6,
            Self::Always => 7,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StencilState;

#[derive(Debug, Clone, Copy, Default)]
pub struct DepthBiasState {
    pub constant: i32,
    pub slope_scale: f32,
    pub clamp: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct DepthStencilState {
    pub format: TextureFormat,
    pub depth_write_enabled: Option<bool>,
    pub depth_compare: Option<CompareFunction>,
    pub stencil: StencilState,
    pub bias: DepthBiasState,
}

#[derive(Debug, Clone, Copy)]
pub struct MultisampleState {
    pub count: u32,
    pub mask: u64,
    pub alpha_to_coverage_enabled: bool,
}

impl Default for MultisampleState {
    fn default() -> Self {
        Self {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        }
    }
}

#[derive(Clone)]
pub struct RenderPipelineDescriptor<'a> {
    pub label: Label<'a>,
    pub layout: Option<&'a PipelineLayout>,
    pub vertex: VertexState<'a>,
    pub primitive: PrimitiveState,
    pub depth_stencil: Option<DepthStencilState>,
    pub multisample: MultisampleState,
    pub fragment: Option<FragmentState<'a>>,
    pub multiview_mask: Option<NonZeroU32>,
    pub cache: Option<&'a ()>,
}

#[derive(Clone)]
pub struct RenderPipeline {
    pub(crate) pso: Arc<ObjCObject>,
    pub(crate) depth_stencil_state: Option<Arc<ObjCObject>>,
    pub(crate) primitive: PrimitiveState,
    pub(crate) depth_bias: DepthBiasState,
}

impl RenderPipeline {
    pub fn get_bind_group_layout(&self, _index: u32) -> BindGroupLayout {
        BindGroupLayout {
            entries: Vec::new(),
        }
    }
}
