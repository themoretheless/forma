//! Conversions between `gpu-runtime` public API types and `upstream_wgpu` types.

use crate::{
    AdapterInfo, AddressMode, Backend, Backends, BindGroupLayoutEntry, BindingType, BlendComponent,
    BlendFactor, BlendOperation, BlendState, BufferBindingType, BufferUsages, Color, ColorWrites,
    CompareFunction, CompositeAlphaMode, DepthBiasState, DepthStencilState, DeviceLostReason,
    DeviceType, Error, Face, Features, FilterMode, FrontFace, InstanceFlags, Limits, MapMode,
    MipmapFilterMode, PollError, PollStatus, PollType, PolygonMode, PowerPreference, PresentMode,
    PrimitiveState, PrimitiveTopology, QueryType, SamplerBindingType, ShaderStages,
    StorageTextureAccess, SubmissionIndex, SurfaceCapabilities, SurfaceColorSpace,
    SurfaceColorSpaces, SurfaceConfiguration, SurfaceFormatCapabilities, TextureAspect,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureViewDimension,
    VertexAttribute, VertexFormat, VertexStepMode,
};
use std::sync::Arc;

pub(crate) fn to_wgpu_backends(b: Backends) -> upstream_wgpu::Backends {
    let mut out = upstream_wgpu::Backends::empty();
    if b.contains(Backends::VULKAN) {
        out |= upstream_wgpu::Backends::VULKAN;
    }
    if b.contains(Backends::GL) {
        out |= upstream_wgpu::Backends::GL;
    }
    if b.contains(Backends::METAL) {
        out |= upstream_wgpu::Backends::METAL;
    }
    if b.contains(Backends::DX12) {
        out |= upstream_wgpu::Backends::DX12;
    }
    if b.contains(Backends::BROWSER_WEBGPU) {
        out |= upstream_wgpu::Backends::BROWSER_WEBGPU;
    }
    out
}

pub(crate) fn from_wgpu_backends(b: upstream_wgpu::Backends) -> Backends {
    let mut out = Backends::empty();
    if b.contains(upstream_wgpu::Backends::VULKAN) {
        out |= Backends::VULKAN;
    }
    if b.contains(upstream_wgpu::Backends::GL) {
        out |= Backends::GL;
    }
    if b.contains(upstream_wgpu::Backends::METAL) {
        out |= Backends::METAL;
    }
    if b.contains(upstream_wgpu::Backends::DX12) {
        out |= Backends::DX12;
    }
    if b.contains(upstream_wgpu::Backends::BROWSER_WEBGPU) {
        out |= Backends::BROWSER_WEBGPU;
    }
    out
}

pub(crate) const fn from_wgpu_backend(b: upstream_wgpu::Backend) -> Backend {
    match b {
        upstream_wgpu::Backend::Noop => Backend::Noop,
        upstream_wgpu::Backend::Vulkan => Backend::Vulkan,
        upstream_wgpu::Backend::Metal => Backend::Metal,
        upstream_wgpu::Backend::Dx12 => Backend::Dx12,
        upstream_wgpu::Backend::Gl => Backend::Gl,
        upstream_wgpu::Backend::BrowserWebGpu => Backend::BrowserWebGpu,
    }
}

pub(crate) fn to_wgpu_instance_flags(f: InstanceFlags) -> upstream_wgpu::InstanceFlags {
    let mut out = upstream_wgpu::InstanceFlags::empty();
    if f.contains(InstanceFlags::DEBUG) {
        out |= upstream_wgpu::InstanceFlags::DEBUG;
    }
    if f.contains(InstanceFlags::VALIDATION) {
        out |= upstream_wgpu::InstanceFlags::VALIDATION;
    }
    if f.contains(InstanceFlags::DISCARD_HAL_LABELS) {
        out |= upstream_wgpu::InstanceFlags::DISCARD_HAL_LABELS;
    }
    out
}

pub(crate) fn to_wgpu_power_preference(p: PowerPreference) -> upstream_wgpu::PowerPreference {
    match p {
        PowerPreference::None => upstream_wgpu::PowerPreference::None,
        PowerPreference::LowPower => upstream_wgpu::PowerPreference::LowPower,
        PowerPreference::HighPerformance => upstream_wgpu::PowerPreference::HighPerformance,
    }
}

const FEATURE_PAIRS: &[(Features, upstream_wgpu::Features)] = &[
    (
        Features::TIMESTAMP_QUERY,
        upstream_wgpu::Features::TIMESTAMP_QUERY,
    ),
    (
        Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
        upstream_wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
    ),
    (
        Features::TIMESTAMP_QUERY_INSIDE_PASSES,
        upstream_wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES,
    ),
    (
        Features::MAPPABLE_PRIMARY_BUFFERS,
        upstream_wgpu::Features::MAPPABLE_PRIMARY_BUFFERS,
    ),
    (
        Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        upstream_wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
    ),
    (
        Features::MULTI_DRAW_INDIRECT_COUNT,
        upstream_wgpu::Features::MULTI_DRAW_INDIRECT_COUNT,
    ),
    (Features::SUBGROUP, upstream_wgpu::Features::SUBGROUP),
    (Features::SHADER_F16, upstream_wgpu::Features::SHADER_F16),
    (
        Features::FLOAT32_FILTERABLE,
        upstream_wgpu::Features::FLOAT32_FILTERABLE,
    ),
    (
        Features::IMMEDIATE_DATA,
        upstream_wgpu::Features::IMMEDIATES,
    ),
    (
        Features::EXPERIMENTAL_RAY_QUERY,
        upstream_wgpu::Features::EXPERIMENTAL_RAY_QUERY,
    ),
    (
        Features::EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE,
        upstream_wgpu::Features::EXPERIMENTAL_RAY_TRACING_PIPELINES,
    ),
    (
        Features::RAY_TRACING_PIPELINE,
        upstream_wgpu::Features::EXPERIMENTAL_RAY_TRACING_PIPELINES,
    ),
];

pub(crate) fn to_wgpu_features(f: Features) -> upstream_wgpu::Features {
    let mut out = upstream_wgpu::Features::empty();
    for &(local, upstream) in FEATURE_PAIRS {
        if f.contains(local) {
            out |= upstream;
        }
    }
    out
}

pub(crate) fn from_wgpu_features(f: upstream_wgpu::Features) -> Features {
    let mut out = Features::empty();
    for &(local, upstream) in FEATURE_PAIRS {
        if f.contains(upstream) {
            out |= local;
        }
    }
    out
}

pub(crate) fn to_wgpu_limits(
    l: &Limits,
    required_features: Features,
    backend: Backend,
    supported: &upstream_wgpu::Limits,
) -> upstream_wgpu::Limits {
    let max_immediate_size = if required_features.contains(Features::IMMEDIATE_DATA) {
        if l.max_immediate_size > 0 {
            l.max_immediate_size
        } else {
            supported.max_immediate_size.min(256)
        }
    } else {
        0
    };
    if backend == Backend::Gl && *l == Limits::default() {
        let mut downlevel = upstream_wgpu::Limits::downlevel_defaults()
            .using_resolution(supported.clone())
            .using_alignment(supported.clone());
        downlevel.max_immediate_size = max_immediate_size;
        return downlevel;
    }
    upstream_wgpu::Limits {
        max_texture_dimension_1d: l.max_texture_dimension_1d,
        max_texture_dimension_2d: l.max_texture_dimension_2d,
        max_texture_dimension_3d: l.max_texture_dimension_3d,
        max_texture_array_layers: l.max_texture_array_layers,
        max_bind_groups: l.max_bind_groups,
        max_bindings_per_bind_group: l.max_bindings_per_bind_group,
        max_dynamic_uniform_buffers_per_pipeline_layout: l
            .max_dynamic_uniform_buffers_per_pipeline_layout,
        max_dynamic_storage_buffers_per_pipeline_layout: l
            .max_dynamic_storage_buffers_per_pipeline_layout,
        max_sampled_textures_per_shader_stage: l.max_sampled_textures_per_shader_stage,
        max_samplers_per_shader_stage: l.max_samplers_per_shader_stage,
        max_storage_buffers_per_shader_stage: l.max_storage_buffers_per_shader_stage,
        max_storage_textures_per_shader_stage: l.max_storage_textures_per_shader_stage,
        max_uniform_buffers_per_shader_stage: l.max_uniform_buffers_per_shader_stage,
        max_uniform_buffer_binding_size: l.max_uniform_buffer_binding_size,
        max_storage_buffer_binding_size: l.max_storage_buffer_binding_size,
        max_buffer_size: l.max_buffer_size,
        max_vertex_buffers: l.max_vertex_buffers,
        max_vertex_attributes: l.max_vertex_attributes,
        max_vertex_buffer_array_stride: l.max_vertex_buffer_array_stride,
        max_inter_stage_shader_variables: (l.max_inter_stage_shader_components / 4).max(1),
        min_uniform_buffer_offset_alignment: l.min_uniform_buffer_offset_alignment,
        min_storage_buffer_offset_alignment: l.min_storage_buffer_offset_alignment,
        max_color_attachments: l.max_color_attachments,
        max_color_attachment_bytes_per_sample: l.max_color_attachment_bytes_per_sample,
        max_compute_workgroup_storage_size: l.max_compute_workgroup_storage_size,
        max_compute_invocations_per_workgroup: l.max_compute_invocations_per_workgroup,
        max_compute_workgroup_size_x: l.max_compute_workgroup_size_x,
        max_compute_workgroup_size_y: l.max_compute_workgroup_size_y,
        max_compute_workgroup_size_z: l.max_compute_workgroup_size_z,
        max_compute_workgroups_per_dimension: l.max_compute_workgroups_per_dimension,
        max_immediate_size,
        max_non_sampler_bindings: l.max_non_sampler_bindings,
        ..upstream_wgpu::Limits::default()
    }
}

pub(crate) fn from_wgpu_limits(l: &upstream_wgpu::Limits) -> Limits {
    Limits {
        max_texture_dimension_1d: l.max_texture_dimension_1d,
        max_texture_dimension_2d: l.max_texture_dimension_2d,
        max_texture_dimension_3d: l.max_texture_dimension_3d,
        max_texture_array_layers: l.max_texture_array_layers,
        max_bind_groups: l.max_bind_groups,
        max_bindings_per_bind_group: l.max_bindings_per_bind_group,
        max_dynamic_uniform_buffers_per_pipeline_layout: l
            .max_dynamic_uniform_buffers_per_pipeline_layout,
        max_dynamic_storage_buffers_per_pipeline_layout: l
            .max_dynamic_storage_buffers_per_pipeline_layout,
        max_sampled_textures_per_shader_stage: l.max_sampled_textures_per_shader_stage,
        max_samplers_per_shader_stage: l.max_samplers_per_shader_stage,
        max_storage_buffers_per_shader_stage: l.max_storage_buffers_per_shader_stage,
        max_storage_textures_per_shader_stage: l.max_storage_textures_per_shader_stage,
        max_uniform_buffers_per_shader_stage: l.max_uniform_buffers_per_shader_stage,
        max_uniform_buffer_binding_size: l.max_uniform_buffer_binding_size,
        max_storage_buffer_binding_size: l.max_storage_buffer_binding_size,
        max_buffer_size: l.max_buffer_size,
        max_vertex_buffers: l.max_vertex_buffers,
        max_vertex_attributes: l.max_vertex_attributes,
        max_vertex_buffer_array_stride: l.max_vertex_buffer_array_stride,
        min_uniform_buffer_offset_alignment: l.min_uniform_buffer_offset_alignment,
        min_storage_buffer_offset_alignment: l.min_storage_buffer_offset_alignment,
        max_inter_stage_shader_components: l.max_inter_stage_shader_variables.saturating_mul(4),
        max_color_attachments: l.max_color_attachments,
        max_color_attachment_bytes_per_sample: l.max_color_attachment_bytes_per_sample,
        max_compute_workgroup_storage_size: l.max_compute_workgroup_storage_size,
        max_compute_invocations_per_workgroup: l.max_compute_invocations_per_workgroup,
        max_compute_workgroup_size_x: l.max_compute_workgroup_size_x,
        max_compute_workgroup_size_y: l.max_compute_workgroup_size_y,
        max_compute_workgroup_size_z: l.max_compute_workgroup_size_z,
        max_compute_workgroups_per_dimension: l.max_compute_workgroups_per_dimension,
        max_immediate_size: l.max_immediate_size,
        max_non_sampler_bindings: l.max_non_sampler_bindings,
    }
}

pub(crate) fn from_wgpu_adapter_info(info: upstream_wgpu::AdapterInfo) -> AdapterInfo {
    let device_type = match info.device_type {
        upstream_wgpu::DeviceType::Other => DeviceType::Other,
        upstream_wgpu::DeviceType::IntegratedGpu => DeviceType::IntegratedGpu,
        upstream_wgpu::DeviceType::DiscreteGpu => DeviceType::DiscreteGpu,
        upstream_wgpu::DeviceType::VirtualGpu => DeviceType::VirtualGpu,
        upstream_wgpu::DeviceType::Cpu => DeviceType::Cpu,
    };
    AdapterInfo {
        name: info.name,
        vendor: info.vendor,
        device: info.device,
        device_type,
        device_pci_bus_id: info.device_pci_bus_id,
        driver: info.driver,
        driver_info: info.driver_info,
        backend: from_wgpu_backend(info.backend),
        subgroup_min_size: info.subgroup_min_size,
        subgroup_max_size: info.subgroup_max_size,
        transient_saves_memory: info.transient_saves_memory.unwrap_or(false),
    }
}

pub(crate) fn to_wgpu_buffer_usages(u: BufferUsages) -> upstream_wgpu::BufferUsages {
    let mut out = upstream_wgpu::BufferUsages::empty();
    if u.contains(BufferUsages::MAP_READ) {
        out |= upstream_wgpu::BufferUsages::MAP_READ;
    }
    if u.contains(BufferUsages::MAP_WRITE) {
        out |= upstream_wgpu::BufferUsages::MAP_WRITE;
    }
    if u.contains(BufferUsages::COPY_SRC) {
        out |= upstream_wgpu::BufferUsages::COPY_SRC;
    }
    if u.contains(BufferUsages::COPY_DST) {
        out |= upstream_wgpu::BufferUsages::COPY_DST;
    }
    if u.contains(BufferUsages::INDEX) {
        out |= upstream_wgpu::BufferUsages::INDEX;
    }
    if u.contains(BufferUsages::VERTEX) {
        out |= upstream_wgpu::BufferUsages::VERTEX;
    }
    if u.contains(BufferUsages::UNIFORM) {
        out |= upstream_wgpu::BufferUsages::UNIFORM;
    }
    if u.contains(BufferUsages::STORAGE) {
        out |= upstream_wgpu::BufferUsages::STORAGE;
    }
    if u.contains(BufferUsages::INDIRECT) {
        out |= upstream_wgpu::BufferUsages::INDIRECT;
    }
    if u.contains(BufferUsages::QUERY_RESOLVE) {
        out |= upstream_wgpu::BufferUsages::QUERY_RESOLVE;
    }
    if u.contains(BufferUsages::BLAS_INPUT) {
        out |= upstream_wgpu::BufferUsages::BLAS_INPUT;
    }
    if u.contains(BufferUsages::TLAS_INPUT) {
        out |= upstream_wgpu::BufferUsages::TLAS_INPUT;
    }
    out
}

pub(crate) fn to_wgpu_texture_usages(u: TextureUsages) -> upstream_wgpu::TextureUsages {
    let mut out = upstream_wgpu::TextureUsages::empty();
    if u.contains(TextureUsages::COPY_SRC) {
        out |= upstream_wgpu::TextureUsages::COPY_SRC;
    }
    if u.contains(TextureUsages::COPY_DST) {
        out |= upstream_wgpu::TextureUsages::COPY_DST;
    }
    if u.contains(TextureUsages::TEXTURE_BINDING) {
        out |= upstream_wgpu::TextureUsages::TEXTURE_BINDING;
    }
    if u.contains(TextureUsages::STORAGE_BINDING) {
        out |= upstream_wgpu::TextureUsages::STORAGE_BINDING;
    }
    if u.contains(TextureUsages::RENDER_ATTACHMENT) {
        out |= upstream_wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    out
}

pub(crate) fn to_wgpu_texture_dimension(d: TextureDimension) -> upstream_wgpu::TextureDimension {
    match d {
        TextureDimension::D1 => upstream_wgpu::TextureDimension::D1,
        TextureDimension::D2 => upstream_wgpu::TextureDimension::D2,
        TextureDimension::D3 => upstream_wgpu::TextureDimension::D3,
    }
}

pub(crate) fn to_wgpu_texture_format(f: TextureFormat) -> upstream_wgpu::TextureFormat {
    match f {
        TextureFormat::R8Unorm => upstream_wgpu::TextureFormat::R8Unorm,
        TextureFormat::R16Uint => upstream_wgpu::TextureFormat::R16Uint,
        TextureFormat::Rgba8Unorm => upstream_wgpu::TextureFormat::Rgba8Unorm,
        TextureFormat::Rgba8UnormSrgb => upstream_wgpu::TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Rgba8Snorm => upstream_wgpu::TextureFormat::Rgba8Snorm,
        TextureFormat::Rgba8Uint => upstream_wgpu::TextureFormat::Rgba8Uint,
        TextureFormat::Rgba8Sint => upstream_wgpu::TextureFormat::Rgba8Sint,
        TextureFormat::Bgra8Unorm => upstream_wgpu::TextureFormat::Bgra8Unorm,
        TextureFormat::Bgra8UnormSrgb => upstream_wgpu::TextureFormat::Bgra8UnormSrgb,
        TextureFormat::R32Uint => upstream_wgpu::TextureFormat::R32Uint,
        TextureFormat::R32Sint => upstream_wgpu::TextureFormat::R32Sint,
        TextureFormat::R32Float => upstream_wgpu::TextureFormat::R32Float,
        TextureFormat::Rg32Uint => upstream_wgpu::TextureFormat::Rg32Uint,
        TextureFormat::Rg32Sint => upstream_wgpu::TextureFormat::Rg32Sint,
        TextureFormat::Rg32Float => upstream_wgpu::TextureFormat::Rg32Float,
        TextureFormat::Rgba16Uint => upstream_wgpu::TextureFormat::Rgba16Uint,
        TextureFormat::Rgba16Sint => upstream_wgpu::TextureFormat::Rgba16Sint,
        TextureFormat::Rgba16Float => upstream_wgpu::TextureFormat::Rgba16Float,
        TextureFormat::Rgba32Uint => upstream_wgpu::TextureFormat::Rgba32Uint,
        TextureFormat::Rgba32Sint => upstream_wgpu::TextureFormat::Rgba32Sint,
        TextureFormat::Rgba32Float => upstream_wgpu::TextureFormat::Rgba32Float,
        TextureFormat::Bc1RgbaUnormSrgb => upstream_wgpu::TextureFormat::Bc1RgbaUnormSrgb,
        TextureFormat::Depth24Plus => upstream_wgpu::TextureFormat::Depth24Plus,
        TextureFormat::Depth32Float => upstream_wgpu::TextureFormat::Depth32Float,
    }
}

pub(crate) fn from_wgpu_texture_format(f: upstream_wgpu::TextureFormat) -> Option<TextureFormat> {
    Some(match f {
        upstream_wgpu::TextureFormat::R8Unorm => TextureFormat::R8Unorm,
        upstream_wgpu::TextureFormat::R16Uint => TextureFormat::R16Uint,
        upstream_wgpu::TextureFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
        upstream_wgpu::TextureFormat::Rgba8UnormSrgb => TextureFormat::Rgba8UnormSrgb,
        upstream_wgpu::TextureFormat::Rgba8Snorm => TextureFormat::Rgba8Snorm,
        upstream_wgpu::TextureFormat::Rgba8Uint => TextureFormat::Rgba8Uint,
        upstream_wgpu::TextureFormat::Rgba8Sint => TextureFormat::Rgba8Sint,
        upstream_wgpu::TextureFormat::Bgra8Unorm => TextureFormat::Bgra8Unorm,
        upstream_wgpu::TextureFormat::Bgra8UnormSrgb => TextureFormat::Bgra8UnormSrgb,
        upstream_wgpu::TextureFormat::R32Uint => TextureFormat::R32Uint,
        upstream_wgpu::TextureFormat::R32Sint => TextureFormat::R32Sint,
        upstream_wgpu::TextureFormat::R32Float => TextureFormat::R32Float,
        upstream_wgpu::TextureFormat::Rg32Uint => TextureFormat::Rg32Uint,
        upstream_wgpu::TextureFormat::Rg32Sint => TextureFormat::Rg32Sint,
        upstream_wgpu::TextureFormat::Rg32Float => TextureFormat::Rg32Float,
        upstream_wgpu::TextureFormat::Rgba16Uint => TextureFormat::Rgba16Uint,
        upstream_wgpu::TextureFormat::Rgba16Sint => TextureFormat::Rgba16Sint,
        upstream_wgpu::TextureFormat::Rgba16Float => TextureFormat::Rgba16Float,
        upstream_wgpu::TextureFormat::Rgba32Uint => TextureFormat::Rgba32Uint,
        upstream_wgpu::TextureFormat::Rgba32Sint => TextureFormat::Rgba32Sint,
        upstream_wgpu::TextureFormat::Rgba32Float => TextureFormat::Rgba32Float,
        upstream_wgpu::TextureFormat::Bc1RgbaUnormSrgb => TextureFormat::Bc1RgbaUnormSrgb,
        upstream_wgpu::TextureFormat::Depth24Plus => TextureFormat::Depth24Plus,
        upstream_wgpu::TextureFormat::Depth32Float => TextureFormat::Depth32Float,
        _ => return None,
    })
}

pub(crate) const fn to_wgpu_texture_aspect(a: TextureAspect) -> upstream_wgpu::TextureAspect {
    match a {
        TextureAspect::All => upstream_wgpu::TextureAspect::All,
        TextureAspect::StencilOnly => upstream_wgpu::TextureAspect::StencilOnly,
        TextureAspect::DepthOnly => upstream_wgpu::TextureAspect::DepthOnly,
    }
}

pub(crate) const fn to_wgpu_view_dimension(
    d: TextureViewDimension,
) -> upstream_wgpu::TextureViewDimension {
    match d {
        TextureViewDimension::D1 => upstream_wgpu::TextureViewDimension::D1,
        TextureViewDimension::D2 => upstream_wgpu::TextureViewDimension::D2,
        TextureViewDimension::D2Array => upstream_wgpu::TextureViewDimension::D2Array,
        TextureViewDimension::Cube => upstream_wgpu::TextureViewDimension::Cube,
        TextureViewDimension::CubeArray => upstream_wgpu::TextureViewDimension::CubeArray,
        TextureViewDimension::D3 => upstream_wgpu::TextureViewDimension::D3,
    }
}

pub(crate) fn to_wgpu_shader_stages(s: ShaderStages) -> upstream_wgpu::ShaderStages {
    let mut out = upstream_wgpu::ShaderStages::empty();
    if s.contains(ShaderStages::VERTEX) {
        out |= upstream_wgpu::ShaderStages::VERTEX;
    }
    if s.contains(ShaderStages::FRAGMENT) {
        out |= upstream_wgpu::ShaderStages::FRAGMENT;
    }
    if s.contains(ShaderStages::COMPUTE) {
        out |= upstream_wgpu::ShaderStages::COMPUTE;
    }
    out
}

pub(crate) fn to_wgpu_color_writes(c: ColorWrites) -> upstream_wgpu::ColorWrites {
    let mut out = upstream_wgpu::ColorWrites::empty();
    if c.contains(ColorWrites::RED) {
        out |= upstream_wgpu::ColorWrites::RED;
    }
    if c.contains(ColorWrites::GREEN) {
        out |= upstream_wgpu::ColorWrites::GREEN;
    }
    if c.contains(ColorWrites::BLUE) {
        out |= upstream_wgpu::ColorWrites::BLUE;
    }
    if c.contains(ColorWrites::ALPHA) {
        out |= upstream_wgpu::ColorWrites::ALPHA;
    }
    out
}

pub(crate) fn to_wgpu_bind_group_layout_entry(
    e: &BindGroupLayoutEntry,
) -> upstream_wgpu::BindGroupLayoutEntry {
    let ty = match e.ty {
        BindingType::Buffer {
            ty,
            has_dynamic_offset,
            min_binding_size,
        } => upstream_wgpu::BindingType::Buffer {
            ty: match ty {
                BufferBindingType::Uniform => upstream_wgpu::BufferBindingType::Uniform,
                BufferBindingType::Storage { read_only } => {
                    upstream_wgpu::BufferBindingType::Storage { read_only }
                }
            },
            has_dynamic_offset,
            min_binding_size,
        },
        BindingType::Sampler(st) => upstream_wgpu::BindingType::Sampler(match st {
            SamplerBindingType::Filtering => upstream_wgpu::SamplerBindingType::Filtering,
            SamplerBindingType::NonFiltering => upstream_wgpu::SamplerBindingType::NonFiltering,
            SamplerBindingType::Comparison => upstream_wgpu::SamplerBindingType::Comparison,
        }),
        BindingType::Texture {
            sample_type,
            view_dimension,
            multisampled,
        } => upstream_wgpu::BindingType::Texture {
            sample_type: match sample_type {
                TextureSampleType::Float { filterable } => {
                    upstream_wgpu::TextureSampleType::Float { filterable }
                }
                TextureSampleType::Depth => upstream_wgpu::TextureSampleType::Depth,
                TextureSampleType::Sint => upstream_wgpu::TextureSampleType::Sint,
                TextureSampleType::Uint => upstream_wgpu::TextureSampleType::Uint,
            },
            view_dimension: to_wgpu_view_dimension(view_dimension),
            multisampled,
        },
        BindingType::StorageTexture {
            access,
            format,
            view_dimension,
        } => upstream_wgpu::BindingType::StorageTexture {
            access: match access {
                StorageTextureAccess::WriteOnly => upstream_wgpu::StorageTextureAccess::WriteOnly,
                StorageTextureAccess::ReadOnly => upstream_wgpu::StorageTextureAccess::ReadOnly,
                StorageTextureAccess::ReadWrite => upstream_wgpu::StorageTextureAccess::ReadWrite,
            },
            format: to_wgpu_texture_format(format),
            view_dimension: to_wgpu_view_dimension(view_dimension),
        },
    };
    upstream_wgpu::BindGroupLayoutEntry {
        binding: e.binding,
        visibility: to_wgpu_shader_stages(e.visibility),
        ty,
        count: e.count,
    }
}

pub(crate) const fn to_wgpu_address_mode(m: AddressMode) -> upstream_wgpu::AddressMode {
    match m {
        AddressMode::ClampToEdge => upstream_wgpu::AddressMode::ClampToEdge,
        AddressMode::Repeat => upstream_wgpu::AddressMode::Repeat,
        AddressMode::MirrorRepeat => upstream_wgpu::AddressMode::MirrorRepeat,
        AddressMode::ClampToBorder => upstream_wgpu::AddressMode::ClampToBorder,
    }
}

pub(crate) const fn to_wgpu_filter_mode(m: FilterMode) -> upstream_wgpu::FilterMode {
    match m {
        FilterMode::Nearest => upstream_wgpu::FilterMode::Nearest,
        FilterMode::Linear => upstream_wgpu::FilterMode::Linear,
    }
}

pub(crate) const fn to_wgpu_mipmap_filter_mode(
    m: MipmapFilterMode,
) -> upstream_wgpu::MipmapFilterMode {
    match m {
        MipmapFilterMode::Nearest => upstream_wgpu::MipmapFilterMode::Nearest,
        MipmapFilterMode::Linear => upstream_wgpu::MipmapFilterMode::Linear,
    }
}

pub(crate) const fn to_wgpu_compare_function(c: CompareFunction) -> upstream_wgpu::CompareFunction {
    match c {
        CompareFunction::Never => upstream_wgpu::CompareFunction::Never,
        CompareFunction::Less => upstream_wgpu::CompareFunction::Less,
        CompareFunction::Equal => upstream_wgpu::CompareFunction::Equal,
        CompareFunction::LessEqual => upstream_wgpu::CompareFunction::LessEqual,
        CompareFunction::Greater => upstream_wgpu::CompareFunction::Greater,
        CompareFunction::NotEqual => upstream_wgpu::CompareFunction::NotEqual,
        CompareFunction::GreaterEqual => upstream_wgpu::CompareFunction::GreaterEqual,
        CompareFunction::Always => upstream_wgpu::CompareFunction::Always,
    }
}

pub(crate) const fn to_wgpu_primitive_state(p: PrimitiveState) -> upstream_wgpu::PrimitiveState {
    upstream_wgpu::PrimitiveState {
        topology: match p.topology {
            PrimitiveTopology::PointList => upstream_wgpu::PrimitiveTopology::PointList,
            PrimitiveTopology::LineList => upstream_wgpu::PrimitiveTopology::LineList,
            PrimitiveTopology::LineStrip => upstream_wgpu::PrimitiveTopology::LineStrip,
            PrimitiveTopology::TriangleList => upstream_wgpu::PrimitiveTopology::TriangleList,
            PrimitiveTopology::TriangleStrip => upstream_wgpu::PrimitiveTopology::TriangleStrip,
        },
        strip_index_format: match p.strip_index_format {
            Some(crate::IndexFormat::Uint16) => Some(upstream_wgpu::IndexFormat::Uint16),
            Some(crate::IndexFormat::Uint32) => Some(upstream_wgpu::IndexFormat::Uint32),
            None => None,
        },
        front_face: match p.front_face {
            FrontFace::Ccw => upstream_wgpu::FrontFace::Ccw,
            FrontFace::Cw => upstream_wgpu::FrontFace::Cw,
        },
        cull_mode: match p.cull_mode {
            Some(Face::Front) => Some(upstream_wgpu::Face::Front),
            Some(Face::Back) => Some(upstream_wgpu::Face::Back),
            None => None,
        },
        unclipped_depth: p.unclipped_depth,
        polygon_mode: match p.polygon_mode {
            PolygonMode::Fill => upstream_wgpu::PolygonMode::Fill,
            PolygonMode::Line => upstream_wgpu::PolygonMode::Line,
            PolygonMode::Point => upstream_wgpu::PolygonMode::Point,
        },
        conservative: p.conservative,
    }
}

const fn to_wgpu_blend_factor(f: BlendFactor) -> upstream_wgpu::BlendFactor {
    match f {
        BlendFactor::Zero => upstream_wgpu::BlendFactor::Zero,
        BlendFactor::One => upstream_wgpu::BlendFactor::One,
        BlendFactor::Src => upstream_wgpu::BlendFactor::Src,
        BlendFactor::OneMinusSrc => upstream_wgpu::BlendFactor::OneMinusSrc,
        BlendFactor::SrcAlpha => upstream_wgpu::BlendFactor::SrcAlpha,
        BlendFactor::OneMinusSrcAlpha => upstream_wgpu::BlendFactor::OneMinusSrcAlpha,
        BlendFactor::Dst => upstream_wgpu::BlendFactor::Dst,
        BlendFactor::OneMinusDst => upstream_wgpu::BlendFactor::OneMinusDst,
        BlendFactor::DstAlpha => upstream_wgpu::BlendFactor::DstAlpha,
        BlendFactor::OneMinusDstAlpha => upstream_wgpu::BlendFactor::OneMinusDstAlpha,
    }
}

const fn to_wgpu_blend_component(c: BlendComponent) -> upstream_wgpu::BlendComponent {
    upstream_wgpu::BlendComponent {
        src_factor: to_wgpu_blend_factor(c.src_factor),
        dst_factor: to_wgpu_blend_factor(c.dst_factor),
        operation: match c.operation {
            BlendOperation::Add => upstream_wgpu::BlendOperation::Add,
            BlendOperation::Subtract => upstream_wgpu::BlendOperation::Subtract,
            BlendOperation::ReverseSubtract => upstream_wgpu::BlendOperation::ReverseSubtract,
            BlendOperation::Min => upstream_wgpu::BlendOperation::Min,
            BlendOperation::Max => upstream_wgpu::BlendOperation::Max,
        },
    }
}

pub(crate) const fn to_wgpu_blend_state(b: BlendState) -> upstream_wgpu::BlendState {
    upstream_wgpu::BlendState {
        color: to_wgpu_blend_component(b.color),
        alpha: to_wgpu_blend_component(b.alpha),
    }
}

pub(crate) fn to_wgpu_depth_stencil_state(
    d: DepthStencilState,
) -> upstream_wgpu::DepthStencilState {
    upstream_wgpu::DepthStencilState {
        format: to_wgpu_texture_format(d.format),
        depth_write_enabled: d.depth_write_enabled,
        depth_compare: d.depth_compare.map(to_wgpu_compare_function),
        stencil: upstream_wgpu::StencilState::default(),
        bias: to_wgpu_depth_bias(d.bias),
    }
}

const fn to_wgpu_depth_bias(b: DepthBiasState) -> upstream_wgpu::DepthBiasState {
    upstream_wgpu::DepthBiasState {
        constant: b.constant,
        slope_scale: b.slope_scale,
        clamp: b.clamp,
    }
}

pub(crate) fn to_wgpu_vertex_attribute(a: VertexAttribute) -> upstream_wgpu::VertexAttribute {
    upstream_wgpu::VertexAttribute {
        format: match a.format {
            VertexFormat::Float32 => upstream_wgpu::VertexFormat::Float32,
            VertexFormat::Float32x2 => upstream_wgpu::VertexFormat::Float32x2,
            VertexFormat::Float32x3 => upstream_wgpu::VertexFormat::Float32x3,
            VertexFormat::Float32x4 => upstream_wgpu::VertexFormat::Float32x4,
            VertexFormat::Uint32 => upstream_wgpu::VertexFormat::Uint32,
            VertexFormat::Uint32x2 => upstream_wgpu::VertexFormat::Uint32x2,
            VertexFormat::Uint32x3 => upstream_wgpu::VertexFormat::Uint32x3,
            VertexFormat::Uint32x4 => upstream_wgpu::VertexFormat::Uint32x4,
            VertexFormat::Sint32 => upstream_wgpu::VertexFormat::Sint32,
            VertexFormat::Sint32x2 => upstream_wgpu::VertexFormat::Sint32x2,
            VertexFormat::Sint32x3 => upstream_wgpu::VertexFormat::Sint32x3,
            VertexFormat::Sint32x4 => upstream_wgpu::VertexFormat::Sint32x4,
        },
        offset: a.offset,
        shader_location: a.shader_location,
    }
}

pub(crate) const fn to_wgpu_step_mode(s: VertexStepMode) -> upstream_wgpu::VertexStepMode {
    match s {
        VertexStepMode::Vertex => upstream_wgpu::VertexStepMode::Vertex,
        VertexStepMode::Instance => upstream_wgpu::VertexStepMode::Instance,
    }
}

pub(crate) const fn to_wgpu_query_type(t: QueryType) -> upstream_wgpu::QueryType {
    match t {
        QueryType::Occlusion => upstream_wgpu::QueryType::Occlusion,
        QueryType::Timestamp => upstream_wgpu::QueryType::Timestamp,
    }
}

pub(crate) const fn to_wgpu_map_mode(m: MapMode) -> upstream_wgpu::MapMode {
    match m {
        MapMode::Read => upstream_wgpu::MapMode::Read,
        MapMode::Write => upstream_wgpu::MapMode::Write,
    }
}

pub(crate) const fn to_wgpu_color(c: Color) -> upstream_wgpu::Color {
    upstream_wgpu::Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

pub(crate) fn from_wgpu_error(err: upstream_wgpu::Error) -> Error {
    match err {
        upstream_wgpu::Error::OutOfMemory { source } => Error::OutOfMemory {
            source: Arc::from(source),
        },
        upstream_wgpu::Error::Validation {
            source,
            description,
        } => Error::Validation {
            source: Arc::from(source),
            description,
        },
        upstream_wgpu::Error::Internal {
            source,
            description,
        } => Error::Internal {
            source: Arc::from(source),
            description,
        },
    }
}

pub(crate) const fn from_wgpu_device_lost_reason(
    r: upstream_wgpu::DeviceLostReason,
) -> DeviceLostReason {
    match r {
        upstream_wgpu::DeviceLostReason::Unknown => DeviceLostReason::Unknown,
        upstream_wgpu::DeviceLostReason::Destroyed => DeviceLostReason::Destroyed,
    }
}

pub(crate) const fn from_wgpu_poll_status(s: upstream_wgpu::PollStatus) -> PollStatus {
    match s {
        upstream_wgpu::PollStatus::QueueEmpty => PollStatus::QueueEmpty,
        upstream_wgpu::PollStatus::WaitSucceeded => PollStatus::WaitSucceeded,
        upstream_wgpu::PollStatus::Poll => PollStatus::Poll,
    }
}

pub(crate) const fn from_wgpu_poll_error(e: upstream_wgpu::PollError) -> PollError {
    match e {
        upstream_wgpu::PollError::Timeout => PollError::Timeout,
        upstream_wgpu::PollError::WrongSubmissionIndex(a, b) => {
            PollError::WrongSubmissionIndex(SubmissionIndex(a), SubmissionIndex(b))
        }
    }
}

pub(crate) fn to_wgpu_poll_type(
    p: PollType,
    submission: Option<upstream_wgpu::SubmissionIndex>,
) -> upstream_wgpu::PollType {
    match p {
        PollType::Poll => upstream_wgpu::PollType::Poll,
        PollType::Wait { timeout, .. } => upstream_wgpu::PollType::Wait {
            submission_index: submission,
            timeout,
        },
    }
}

pub(crate) fn from_wgpu_surface_capabilities(
    caps: upstream_wgpu::SurfaceCapabilities,
) -> SurfaceCapabilities {
    let formats: Vec<TextureFormat> = caps
        .formats
        .into_iter()
        .filter_map(from_wgpu_texture_format)
        .collect();
    let format_capabilities = formats
        .iter()
        .map(|&format| SurfaceFormatCapabilities {
            format,
            color_spaces: SurfaceColorSpaces::SRGB,
        })
        .collect();
    let present_modes = caps
        .present_modes
        .into_iter()
        .map(|m| match m {
            upstream_wgpu::PresentMode::AutoVsync => PresentMode::AutoVsync,
            upstream_wgpu::PresentMode::AutoNoVsync => PresentMode::AutoNoVsync,
            upstream_wgpu::PresentMode::Fifo => PresentMode::Fifo,
            upstream_wgpu::PresentMode::FifoRelaxed => PresentMode::FifoRelaxed,
            upstream_wgpu::PresentMode::Immediate => PresentMode::Immediate,
            upstream_wgpu::PresentMode::Mailbox => PresentMode::Mailbox,
        })
        .collect();
    let alpha_modes = caps
        .alpha_modes
        .into_iter()
        .map(|a| match a {
            upstream_wgpu::CompositeAlphaMode::Auto => CompositeAlphaMode::Auto,
            upstream_wgpu::CompositeAlphaMode::Opaque => CompositeAlphaMode::Opaque,
            upstream_wgpu::CompositeAlphaMode::PreMultiplied => CompositeAlphaMode::PreMultiplied,
            upstream_wgpu::CompositeAlphaMode::PostMultiplied => CompositeAlphaMode::PostMultiplied,
            upstream_wgpu::CompositeAlphaMode::Inherit => CompositeAlphaMode::Inherit,
        })
        .collect();
    SurfaceCapabilities {
        formats,
        format_capabilities,
        present_modes,
        alpha_modes,
        usages: to_local_texture_usages(caps.usages),
    }
}

fn to_local_texture_usages(u: upstream_wgpu::TextureUsages) -> TextureUsages {
    let mut out = TextureUsages::empty();
    if u.contains(upstream_wgpu::TextureUsages::COPY_SRC) {
        out |= TextureUsages::COPY_SRC;
    }
    if u.contains(upstream_wgpu::TextureUsages::COPY_DST) {
        out |= TextureUsages::COPY_DST;
    }
    if u.contains(upstream_wgpu::TextureUsages::TEXTURE_BINDING) {
        out |= TextureUsages::TEXTURE_BINDING;
    }
    if u.contains(upstream_wgpu::TextureUsages::STORAGE_BINDING) {
        out |= TextureUsages::STORAGE_BINDING;
    }
    if u.contains(upstream_wgpu::TextureUsages::RENDER_ATTACHMENT) {
        out |= TextureUsages::RENDER_ATTACHMENT;
    }
    out
}

pub(crate) fn to_wgpu_surface_color_space(
    c: SurfaceColorSpace,
) -> upstream_wgpu::wgt::SurfaceColorSpace {
    use upstream_wgpu::wgt::SurfaceColorSpace as S;
    match c {
        SurfaceColorSpace::Auto => S::Auto,
        SurfaceColorSpace::Srgb => S::Srgb,
        SurfaceColorSpace::ExtendedSrgbLinear => S::ExtendedSrgbLinear,
        SurfaceColorSpace::DisplayP3 => S::DisplayP3,
        SurfaceColorSpace::Bt2100Pq => S::Bt2100Pq,
        SurfaceColorSpace::Bt2100Hlg => S::Bt2100Hlg,
        SurfaceColorSpace::ExtendedSrgb => S::ExtendedSrgb,
        SurfaceColorSpace::ExtendedDisplayP3 => S::ExtendedDisplayP3,
    }
}

pub(crate) fn to_wgpu_surface_config(
    cfg: &SurfaceConfiguration,
) -> upstream_wgpu::SurfaceConfiguration {
    let view_formats: Vec<upstream_wgpu::TextureFormat> = cfg
        .view_formats
        .iter()
        .copied()
        .map(to_wgpu_texture_format)
        .collect();
    upstream_wgpu::SurfaceConfiguration {
        usage: to_wgpu_texture_usages(cfg.usage),
        format: to_wgpu_texture_format(cfg.format),
        color_space: to_wgpu_surface_color_space(cfg.color_space),
        width: cfg.width.max(1),
        height: cfg.height.max(1),
        present_mode: match cfg.present_mode {
            PresentMode::AutoVsync => upstream_wgpu::PresentMode::AutoVsync,
            PresentMode::AutoNoVsync => upstream_wgpu::PresentMode::AutoNoVsync,
            PresentMode::Fifo => upstream_wgpu::PresentMode::Fifo,
            PresentMode::FifoRelaxed => upstream_wgpu::PresentMode::FifoRelaxed,
            PresentMode::Immediate => upstream_wgpu::PresentMode::Immediate,
            PresentMode::Mailbox => upstream_wgpu::PresentMode::Mailbox,
        },
        desired_maximum_frame_latency: cfg.desired_maximum_frame_latency,
        alpha_mode: match cfg.alpha_mode {
            CompositeAlphaMode::Auto => upstream_wgpu::CompositeAlphaMode::Auto,
            CompositeAlphaMode::Opaque => upstream_wgpu::CompositeAlphaMode::Opaque,
            CompositeAlphaMode::PreMultiplied => upstream_wgpu::CompositeAlphaMode::PreMultiplied,
            CompositeAlphaMode::PostMultiplied => upstream_wgpu::CompositeAlphaMode::PostMultiplied,
            CompositeAlphaMode::Inherit => upstream_wgpu::CompositeAlphaMode::Inherit,
        },
        view_formats,
    }
}
