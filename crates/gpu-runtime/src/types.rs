//! GPU Adapter, Device, Limits, Error Scope, Poll, and Descriptor types.

use crate::{Backends, Features, InstanceFlags, Label};
use std::future::Future;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backend {
    Noop,
    Vulkan,
    Metal,
    Dx12,
    Gl,
    BrowserWebGpu,
    Cuda,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Other,
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PowerPreference {
    #[default]
    None,
    LowPower,
    HighPerformance,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryBudgetThresholds {
    pub for_resource_creation: Option<u8>,
    pub for_device_loss: Option<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct BackendOptions {
    pub gl: (),
    pub dx12: (),
    pub noop: (),
}

#[derive(Debug, Clone)]
pub struct InstanceDescriptor {
    pub backends: Backends,
    pub flags: InstanceFlags,
    pub memory_budget_thresholds: MemoryBudgetThresholds,
    pub backend_options: BackendOptions,
    pub display: Option<()>,
}

impl Default for InstanceDescriptor {
    fn default() -> Self {
        Self {
            backends: Backends::PRIMARY,
            flags: InstanceFlags::empty(),
            memory_budget_thresholds: MemoryBudgetThresholds::default(),
            backend_options: BackendOptions::default(),
            display: None,
        }
    }
}

impl InstanceDescriptor {
    pub fn new_without_display_handle() -> Self {
        Self::default()
    }

    pub fn new_with_display_handle(_display: Box<dyn std::any::Any>) -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub device_type: DeviceType,
    pub device_pci_bus_id: String,
    pub driver: String,
    pub driver_info: String,
    pub backend: Backend,
    pub subgroup_min_size: u32,
    pub subgroup_max_size: u32,
    pub transient_saves_memory: bool,
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_texture_dimension_1d: u32,
    pub max_texture_dimension_2d: u32,
    pub max_texture_dimension_3d: u32,
    pub max_texture_array_layers: u32,
    pub max_bind_groups: u32,
    pub max_bindings_per_bind_group: u32,
    pub max_dynamic_uniform_buffers_per_pipeline_layout: u32,
    pub max_dynamic_storage_buffers_per_pipeline_layout: u32,
    pub max_sampled_textures_per_shader_stage: u32,
    pub max_samplers_per_shader_stage: u32,
    pub max_storage_buffers_per_shader_stage: u32,
    pub max_storage_textures_per_shader_stage: u32,
    pub max_uniform_buffers_per_shader_stage: u32,
    pub max_uniform_buffer_binding_size: u64,
    pub max_storage_buffer_binding_size: u64,
    pub max_buffer_size: u64,
    pub max_vertex_buffers: u32,
    pub max_vertex_attributes: u32,
    pub max_vertex_buffer_array_stride: u32,
    pub min_uniform_buffer_offset_alignment: u32,
    pub min_storage_buffer_offset_alignment: u32,
    pub max_inter_stage_shader_components: u32,
    pub max_color_attachments: u32,
    pub max_color_attachment_bytes_per_sample: u32,
    pub max_compute_workgroup_storage_size: u32,
    pub max_compute_invocations_per_workgroup: u32,
    pub max_compute_workgroup_size_x: u32,
    pub max_compute_workgroup_size_y: u32,
    pub max_compute_workgroup_size_z: u32,
    pub max_compute_workgroups_per_dimension: u32,
    pub max_immediate_size: u32,
    pub max_non_sampler_bindings: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_texture_dimension_1d: 16384,
            max_texture_dimension_2d: 16384,
            max_texture_dimension_3d: 2048,
            max_texture_array_layers: 2048,
            max_bind_groups: 4,
            max_bindings_per_bind_group: 1000,
            max_dynamic_uniform_buffers_per_pipeline_layout: 8,
            max_dynamic_storage_buffers_per_pipeline_layout: 4,
            max_sampled_textures_per_shader_stage: 16,
            max_samplers_per_shader_stage: 16,
            max_storage_buffers_per_shader_stage: 16,
            max_storage_textures_per_shader_stage: 8,
            max_uniform_buffers_per_shader_stage: 12,
            max_uniform_buffer_binding_size: 65536,
            max_storage_buffer_binding_size: 1 << 30,
            max_buffer_size: 1 << 31,
            max_vertex_buffers: 8,
            max_vertex_attributes: 16,
            max_vertex_buffer_array_stride: 2048,
            min_uniform_buffer_offset_alignment: 256,
            min_storage_buffer_offset_alignment: 32,
            max_inter_stage_shader_components: 60,
            max_color_attachments: 8,
            max_color_attachment_bytes_per_sample: 64,
            max_compute_workgroup_storage_size: 32768,
            max_compute_invocations_per_workgroup: 1024,
            max_compute_workgroup_size_x: 1024,
            max_compute_workgroup_size_y: 1024,
            max_compute_workgroup_size_z: 64,
            max_compute_workgroups_per_dimension: 65535,
            max_immediate_size: 256,
            max_non_sampler_bindings: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RequestAdapterOptions<'a, 'b> {
    pub power_preference: PowerPreference,
    pub force_fallback_adapter: bool,
    pub compatible_surface: Option<&'a Surface<'b>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresentMode {
    #[default]
    Fifo,
    FifoRelaxed,
    Immediate,
    Mailbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompositeAlphaMode {
    #[default]
    Auto,
    Opaque,
    PreMultiplied,
    PostMultiplied,
    Inherit,
}

#[derive(Debug, Clone)]
pub struct SurfaceCapabilities {
    pub formats: Vec<crate::TextureFormat>,
    pub present_modes: Vec<PresentMode>,
    pub alpha_modes: Vec<CompositeAlphaMode>,
    pub usages: crate::TextureUsages,
}

#[derive(Debug, Clone)]
pub struct SurfaceConfiguration {
    pub usage: crate::TextureUsages,
    pub format: crate::TextureFormat,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
    pub desired_maximum_frame_latency: u32,
    pub alpha_mode: CompositeAlphaMode,
    pub view_formats: Vec<crate::TextureFormat>,
}

pub struct SurfaceTexture {
    pub texture: crate::Texture,
    pub suboptimal: bool,
}

impl SurfaceTexture {
    pub fn present(self) {}
}

pub enum CurrentSurfaceTexture {
    Success(SurfaceTexture),
    Suboptimal(SurfaceTexture),
    Timeout,
    Occluded,
    Outdated,
    Lost,
    Validation,
}

impl std::fmt::Debug for CurrentSurfaceTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success(_) => write!(f, "Success"),
            Self::Suboptimal(_) => write!(f, "Suboptimal"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Occluded => write!(f, "Occluded"),
            Self::Outdated => write!(f, "Outdated"),
            Self::Lost => write!(f, "Lost"),
            Self::Validation => write!(f, "Validation"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Surface<'a> {
    pub(crate) config: Arc<Mutex<Option<(crate::Device, SurfaceConfiguration)>>>,
    pub(crate) _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Surface<'a> {
    pub fn get_capabilities(&self, _adapter: &crate::Adapter) -> SurfaceCapabilities {
        SurfaceCapabilities {
            formats: vec![
                crate::TextureFormat::Bgra8Unorm,
                crate::TextureFormat::Rgba8Unorm,
                crate::TextureFormat::Bgra8UnormSrgb,
                crate::TextureFormat::Rgba8UnormSrgb,
            ],
            present_modes: vec![PresentMode::Fifo, PresentMode::Immediate, PresentMode::Mailbox],
            alpha_modes: vec![
                CompositeAlphaMode::Auto,
                CompositeAlphaMode::Opaque,
                CompositeAlphaMode::PostMultiplied,
            ],
            usages: crate::TextureUsages::RENDER_ATTACHMENT | crate::TextureUsages::COPY_SRC | crate::TextureUsages::COPY_DST,
        }
    }

    pub fn get_default_config(
        &self,
        _adapter: &crate::Adapter,
        width: u32,
        height: u32,
    ) -> Option<SurfaceConfiguration> {
        Some(SurfaceConfiguration {
            usage: crate::TextureUsages::RENDER_ATTACHMENT,
            format: crate::TextureFormat::Bgra8Unorm,
            width: width.max(1),
            height: height.max(1),
            present_mode: PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: Vec::new(),
        })
    }

    pub fn configure(&self, device: &crate::Device, config: &SurfaceConfiguration) {
        if let Ok(mut guard) = self.config.lock() {
            *guard = Some((device.clone(), config.clone()));
        }
    }

    pub fn get_current_texture(&self) -> CurrentSurfaceTexture {
        let Some((device, cfg)) = self.config.lock().ok().and_then(|g| g.clone()) else {
            return CurrentSurfaceTexture::Outdated;
        };
        let texture = device.create_texture(&crate::TextureDescriptor {
            label: Some("Surface drawable texture"),
            size: crate::Extent3d {
                width: cfg.width.max(1),
                height: cfg.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: crate::TextureDimension::D2,
            format: cfg.format,
            usage: cfg.usage | crate::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        CurrentSurfaceTexture::Success(SurfaceTexture {
            texture,
            suboptimal: false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceLostReason {
    Unknown,
    Destroyed,
}

#[derive(Debug, Clone, Default)]
pub struct AllocatorReport {
    pub total_allocated_bytes: u64,
    pub total_reserved_bytes: u64,
    pub allocations: Vec<()>,
    pub blocks: Vec<()>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryHints;

impl MemoryHints {
    #[allow(non_upper_case_globals)]
    pub const Performance: Self = Self;
    #[allow(non_upper_case_globals)]
    pub const MemoryUsage: Self = Self;
}

#[derive(Debug, Clone, Default)]
pub struct Trace;

impl Trace {
    #[allow(non_upper_case_globals)]
    pub const Off: Self = Self;
}

#[derive(Debug, Clone, Default)]
pub struct ExperimentalFeatures;

impl ExperimentalFeatures {
    pub const fn disabled() -> Self {
        Self
    }
}

#[derive(Debug, Clone)]
pub struct DeviceDescriptor<'a> {
    pub label: Label<'a>,
    pub required_features: Features,
    pub required_limits: Limits,
    pub experimental_features: ExperimentalFeatures,
    pub memory_hints: MemoryHints,
    pub trace: Trace,
}

impl Default for DeviceDescriptor<'_> {
    fn default() -> Self {
        Self {
            label: None,
            required_features: Features::empty(),
            required_limits: Limits::default(),
            experimental_features: ExperimentalFeatures::disabled(),
            memory_hints: MemoryHints::Performance,
            trace: Trace::Off,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestDeviceError(pub String);
impl std::fmt::Display for RequestDeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for RequestDeviceError {}

#[derive(Debug, Clone)]
pub struct RequestAdapterError(pub String);
impl std::fmt::Display for RequestAdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for RequestAdapterError {}

#[derive(Debug, Clone)]
pub enum Error {
    OutOfMemory {
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    Validation {
        source: Arc<dyn std::error::Error + Send + Sync>,
        description: String,
    },
    Internal {
        source: Arc<dyn std::error::Error + Send + Sync>,
        description: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfMemory { source } => write!(f, "Out of memory: {source}"),
            Self::Validation { description, .. } => write!(f, "Validation error: {description}"),
            Self::Internal { description, .. } => write!(f, "Internal error: {description}"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorFilter {
    OutOfMemory,
    Validation,
    Internal,
}

pub struct ErrorScopeGuard {
    pub(crate) stack: Arc<Mutex<Vec<(ErrorFilter, Option<Error>)>>>,
}

impl ErrorScopeGuard {
    pub fn pop(self) -> impl Future<Output = Option<Error>> {
        let err = self
            .stack
            .lock()
            .ok()
            .and_then(|mut s| s.pop())
            .and_then(|(_, e)| e);
        std::future::ready(err)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubmissionIndex(pub u64);

#[derive(Debug, Clone)]
pub enum PollType {
    Poll,
    Wait {
        submission_index: Option<SubmissionIndex>,
        timeout: Option<std::time::Duration>,
    },
}

impl PollType {
    pub fn wait_indefinitely() -> Self {
        Self::Wait {
            submission_index: None,
            timeout: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollStatus {
    QueueEmpty,
    WaitSucceeded,
    Poll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError {
    Timeout,
    WrongSubmissionIndex(SubmissionIndex, SubmissionIndex),
}

impl std::fmt::Display for PollError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "Poll wait timed out"),
            Self::WrongSubmissionIndex(a, b) => {
                write!(f, "Wrong submission index: {:?} vs {:?}", a, b)
            }
        }
    }
}
impl std::error::Error for PollError {}
