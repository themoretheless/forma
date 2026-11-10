#![allow(unsafe_code, unsafe_op_in_unsafe_fn, unused_variables, dead_code)]

//! Modular, zero-overhead cross-platform GPU abstraction layer (`wgpu` drop-in replacement).
//!
//! Organized according to **SOLID**, **DRY**, and **Divide & Conquer** principles:
//! - [`platform`]: Target OS detection (`Windows`, `MacOs`, `Linux`, `Ios`, `Android`, `BrowserWebGpu`),
//!   orthogonal memory architectures (`Uma`, `Duma`), storage direct capabilities (`StorageDirectCapability`),
//!   and 5-level `HardwareExecutionTopology`.
//! - [`flags`]: Bitflags (`Backends`, `Features`, `BufferUsages`, `TextureUsages`, `ShaderStages`, `ColorWrites`).
//! - [`types`]: Core descriptors, limits, adapter info, error scopes, and poll primitives.
//! - [`buffer`]: Zero-copy UMA/ASD `Buffer`, `BufferSlice`, `BufferView`, and `WriteOnly`.
//! - [`texture`]: `Texture`, `TextureView`, `Sampler`, `QuerySet`, and texel copy descriptors.
//! - [`pipeline`]: `ShaderModule`, `BindGroup`, `ComputePipeline`, `RenderPipeline`, and state descriptors.
//! - [`command`]: `CommandEncoder`, `CommandBuffer`, `ComputePass`, `RenderPass`, and Metal encoder passes.
//! - [`rtx`]: Hardware Ray Tracing (`Blas`, `Tlas`, `RtxCapabilities`, `RtxTier`).
//! - [`metal`], [`vulkan`], [`dx12`], [`cuda`], [`opengl`], [`webgpu`]: Hardware backend runtimes.

pub mod asd;
pub mod buffer;
pub mod command;
pub mod cuda;
pub mod duma;
pub mod dx12;
pub mod flags;
pub mod metal;
pub mod opengl;
pub mod pipeline;
pub mod platform;
pub mod rtx;
pub mod texture;
pub mod transfer;
pub mod types;
pub mod uma;
pub mod util;
pub mod vulkan;
pub mod webgpu;

/// `Debug` for GPU handles whose contents are opaque backend objects.
macro_rules! impl_opaque_debug {
    ($($ty:ty => $name:literal),+ $(,)?) => {
        $(impl std::fmt::Debug for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct($name).finish_non_exhaustive()
            }
        })+
    };
}

impl_opaque_debug!(
    TextureView => "TextureView",
    Sampler => "Sampler",
    QuerySet => "QuerySet",
    ShaderModule => "ShaderModule",
    ComputePipeline => "ComputePipeline",
    RenderPipeline => "RenderPipeline",
    BindGroup => "BindGroup",
    Adapter => "Adapter",
    Instance => "Instance",
    CommandBuffer => "CommandBuffer",
    SurfaceTexture => "SurfaceTexture",
);

pub use asd::*;
pub use buffer::*;
pub use command::*;
pub use duma::*;
pub use flags::*;
pub use opengl::*;
pub use pipeline::*;
pub use platform::*;
pub use rtx::*;
pub use texture::*;
pub use transfer::*;
pub use types::*;
pub use uma::*;
pub use webgpu::*;

use gpu_shader as naga;
use gpu_shader::back::msl::BindingMap;
use metal::MetalDevice;
use std::ffi::c_void;
use std::future::Future;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub type BufferAddress = u64;
pub type BufferSize = NonZeroU64;
pub type DynamicOffset = u32;
pub type Label<'a> = Option<&'a str>;

pub const COPY_BUFFER_ALIGNMENT: u64 = 4;
pub const COPY_BYTES_PER_ROW_ALIGNMENT: u32 = 256;
pub const MAP_ALIGNMENT: u64 = 8;

#[derive(Clone)]
pub struct Instance {
    desc: InstanceDescriptor,
}

impl Default for Instance {
    fn default() -> Self {
        Self::new(InstanceDescriptor::default())
    }
}

impl Instance {
    pub fn new(desc: InstanceDescriptor) -> Self {
        Self { desc }
    }

    /// Backends this build can drive. Only Metal has an implementation in this runtime.
    pub fn enabled_backend_features() -> Backends {
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            Backends::METAL
        } else {
            Backends::empty()
        }
    }

    pub fn enumerate_adapters(&self, backends: Backends) -> impl Future<Output = Vec<Adapter>> {
        let mut list = Vec::new();
        if backends.contains(Backends::METAL) {
            if let Some(m) = MetalDevice::create_system_default() {
                list.push(Adapter::from_metal(m));
            }
        }
        std::future::ready(list)
    }

    pub fn request_adapter(
        &self,
        _options: &RequestAdapterOptions<'_, '_>,
    ) -> impl Future<Output = Result<Adapter, RequestAdapterError>> {
        let res = if let Some(m) = MetalDevice::create_system_default() {
            Ok(Adapter::from_metal(m))
        } else {
            Err(RequestAdapterError(
                "No supported GPU adapter found".to_string(),
            ))
        };
        std::future::ready(res)
    }

    pub fn create_surface<'a, W>(&self, target: W) -> Result<Surface<'a>, RequestDeviceError>
    where
        W: raw_window_handle::HasWindowHandle,
    {
        let mut metal_layer = None;
        let mut metal_view = None;
        if let Ok(handle) = target.window_handle() {
            match handle.as_raw() {
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                raw_window_handle::RawWindowHandle::AppKit(appkit) => {
                    let view_ptr = appkit.ns_view.as_ptr();
                    metal_layer =
                        unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, true) };
                    metal_view = crate::metal::ObjCObject::from_unretained(view_ptr).map(Arc::new);
                }
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                raw_window_handle::RawWindowHandle::UiKit(uikit) => {
                    let view_ptr = uikit.ui_view.as_ptr();
                    metal_layer =
                        unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, false) };
                    metal_view = crate::metal::ObjCObject::from_unretained(view_ptr).map(Arc::new);
                }
                _ => {}
            }
        }
        Ok(Surface {
            config: Arc::new(Mutex::new(None)),
            metal_layer,
            metal_view,
            _marker: std::marker::PhantomData,
        })
    }
}

#[derive(Clone)]
pub struct Adapter {
    pub(crate) metal: MetalDevice,
    pub(crate) info: AdapterInfo,
    pub(crate) features: Features,
    pub(crate) limits: Limits,
}

impl Adapter {
    pub(crate) fn from_metal(metal: MetalDevice) -> Self {
        let device_type = if metal.has_unified_memory {
            DeviceType::IntegratedGpu
        } else {
            DeviceType::DiscreteGpu
        };
        let info = AdapterInfo {
            name: metal.name.to_string(),
            vendor: 0x106B, // Apple
            device: 1,
            device_type,
            device_pci_bus_id: String::new(),
            driver: "Metal Direct Native Runtime".to_string(),
            driver_info: "ASD + UMA + dUMA Accelerated".to_string(),
            backend: Backend::Metal,
            subgroup_min_size: 32,
            subgroup_max_size: 32,
            transient_saves_memory: true,
        };
        let mut features = Features::TIMESTAMP_QUERY
            | Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
            | Features::TIMESTAMP_QUERY_INSIDE_PASSES
            | Features::SHADER_F16
            | Features::FLOAT32_FILTERABLE
            | Features::IMMEDIATE_DATA
            | Features::SUBGROUP
            | Features::EXPERIMENTAL_RAY_QUERY
            | Features::EXPERIMENTAL_RAY_TRACING_ACCELERATION_STRUCTURE
            | Features::RAY_TRACING_PIPELINE;
        if metal.has_unified_memory {
            features |= Features::MAPPABLE_PRIMARY_BUFFERS;
        }
        let limits = Limits {
            max_buffer_size: metal.max_buffer_length,
            max_storage_buffer_binding_size: metal.max_buffer_length,
            ..Limits::default()
        };
        Self {
            metal,
            info,
            features,
            limits,
        }
    }

    pub fn get_info(&self) -> AdapterInfo {
        self.info.clone()
    }

    /// A surface is presentable when it is backed by a Metal layer.
    pub fn is_surface_supported(&self, surface: &Surface<'_>) -> bool {
        surface.metal_layer.is_some()
    }

    pub fn features(&self) -> Features {
        self.features
    }

    pub fn limits(&self) -> Limits {
        self.limits.clone()
    }

    pub fn request_device(
        &self,
        desc: &DeviceDescriptor<'_>,
    ) -> impl Future<Output = Result<(Device, Queue), RequestDeviceError>> {
        let dev = Device {
            metal: self.metal.clone(),
            info: Arc::new(self.info.clone()),
            features: desc.required_features & self.features,
            limits: desc.required_limits.clone(),
            error_stack: Arc::new(Mutex::new(Vec::new())),
            uncaptured_error_handler: Arc::new(Mutex::new(None)),
            device_loss: Arc::new(DeviceLossState::default()),
        };
        let queue = Queue {
            metal: self.metal.clone(),
            identity: Arc::new(()),
            error_sink: CommandErrorSink {
                stack: dev.error_stack.clone(),
                handler: dev.uncaptured_error_handler.clone(),
                scopes: None,
                captured: false,
            },
        };
        std::future::ready(Ok((dev, queue)))
    }
}

type DeviceLostHandler = Arc<dyn Fn(DeviceLostReason, String) + Send + Sync + 'static>;
#[derive(Default)]
struct DeviceLossInner {
    callback: Option<DeviceLostHandler>,
    loss: Option<(DeviceLostReason, String)>,
    delivered: bool,
}
#[derive(Default)]
struct DeviceLossState(Mutex<DeviceLossInner>);
impl DeviceLossState {
    fn take_notification(
        state: &mut DeviceLossInner,
    ) -> Option<(DeviceLostHandler, DeviceLostReason, String)> {
        if state.delivered {
            return None;
        }
        let callback = state.callback.as_ref()?.clone();
        let (reason, message) = state.loss.as_ref()?;
        let notification = (callback, *reason, message.clone());
        state.delivered = true;
        Some(notification)
    }
    fn set_callback(&self, callback: DeviceLostHandler) {
        let notification = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            state.callback = Some(callback);
            Self::take_notification(&mut state)
        };
        if let Some((callback, reason, message)) = notification {
            callback(reason, message);
        }
    }
    fn notify(&self, reason: DeviceLostReason, message: String) {
        let notification = {
            let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if state.loss.is_none() {
                state.loss = Some((reason, message));
            }
            Self::take_notification(&mut state)
        };
        if let Some((callback, reason, message)) = notification {
            callback(reason, message);
        }
    }
}

pub(crate) type UncapturedErrorHandler = Arc<dyn Fn(Error) + Send + Sync + 'static>;

// Matching scopes consume errors even after their first slot is occupied.
// Invoke unscoped handlers outside both mutexes to permit handler reentry.
pub(crate) fn dispatch_error(
    stack: &Mutex<Vec<crate::types::ErrorScopeEntry>>,
    handler: &Mutex<Option<UncapturedErrorHandler>>,
    error: Error,
) {
    let filter = match &error {
        Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
        Error::Validation { .. } => ErrorFilter::Validation,
        Error::Internal { .. } => ErrorFilter::Internal,
    };
    {
        let mut scopes = stack.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = scopes.iter_mut().rev().find(|entry| entry.filter == filter) {
            entry.record(error);
            return;
        }
    }
    let callback = handler.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(callback) = callback {
        callback(error);
    }
}

#[derive(Clone)]
pub(crate) struct CommandErrorSink {
    stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
    handler: Arc<Mutex<Option<UncapturedErrorHandler>>>,
    scopes: Option<Arc<crate::types::SubmittedScopes>>,
    captured: bool,
}
impl CommandErrorSink {
    pub(crate) fn has_scoped_work(&self) -> bool {
        self.scopes.is_some()
    }
    pub(crate) fn same_device(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.stack, &other.stack)
    }
    pub(crate) fn capture(&self) -> Self {
        if self.captured {
            return self.clone();
        }
        let active = self.stack.lock().unwrap_or_else(|e| e.into_inner());
        let scopes = if active.is_empty() {
            None
        } else {
            Some(Arc::new(crate::types::SubmittedScopes::capture(&active)))
        };
        Self {
            stack: self.stack.clone(),
            handler: self.handler.clone(),
            scopes,
            captured: true,
        }
    }
    pub(crate) fn same_submission(&self, other: &Self) -> bool {
        match (&self.scopes, &other.scopes) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => self.captured && other.captured && self.same_device(other),
            _ => false,
        }
    }
    pub(crate) fn command_failure(&self, error: Error) {
        if self.captured {
            let filter = match &error {
                Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
                Error::Validation { .. } => ErrorFilter::Validation,
                Error::Internal { .. } => ErrorFilter::Internal,
            };
            if let Some(entry) = self
                .scopes
                .as_ref()
                .and_then(|scopes| scopes.0.iter().rev().find(|entry| entry.filter == filter))
            {
                entry.record(error);
                return;
            }
            let handler = self
                .handler
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(handler) = handler {
                handler(error);
            }
        } else {
            dispatch_error(&self.stack, &self.handler, error);
        }
    }
}

#[derive(Clone)]
pub struct Device {
    pub(crate) metal: MetalDevice,
    pub(crate) info: Arc<AdapterInfo>,
    pub(crate) features: Features,
    pub(crate) limits: Limits,
    pub(crate) error_stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
    pub(crate) uncaptured_error_handler: Arc<Mutex<Option<UncapturedErrorHandler>>>,
    device_loss: Arc<DeviceLossState>,
}

impl PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.error_stack, &other.error_stack)
    }
}
impl Eq for Device {}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("name", &self.metal.name)
            .finish()
    }
}

#[derive(Clone)]
pub struct Queue {
    pub(crate) metal: MetalDevice,
    // One queue per logical device. Backend handles can be shared by separate
    // request_device calls; cloning the public queue must retain its identity.
    identity: Arc<()>,
    error_sink: CommandErrorSink,
}

impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }
}
impl Eq for Queue {}

impl std::fmt::Debug for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Queue")
            .field("name", &self.metal.name)
            .finish()
    }
}

impl Device {
    pub fn adapter_info(&self) -> AdapterInfo {
        (*self.info).clone()
    }

    pub fn features(&self) -> Features {
        self.features
    }

    pub fn limits(&self) -> Limits {
        self.limits.clone()
    }

    pub fn push_error_scope(&self, filter: ErrorFilter) -> ErrorScopeGuard {
        ErrorScopeGuard::push(self.error_stack.clone(), filter).with_completion(self.metal.clone())
    }

    pub fn on_uncaptured_error(&self, handler: Arc<dyn Fn(Error) + Send + Sync + 'static>) {
        *self
            .uncaptured_error_handler
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handler);
    }

    /// Deliver a native backend loss once per logical device, including late
    /// registration. Backend completion notifications are serviced by `poll`.
    pub fn set_device_lost_callback(
        &self,
        callback: impl Fn(DeviceLostReason, String) + Send + Sync + 'static,
    ) {
        self.device_loss.set_callback(Arc::new(callback));
        self.refresh_device_loss();
    }

    #[cfg(feature = "test-hooks")]
    #[doc(hidden)]
    pub fn inject_backend_device_loss_for_testing(&self, message: String) {
        self.metal.inject_device_loss_for_test(message);
    }

    fn refresh_device_loss(&self) {
        if let Some(message) = self.metal.device_loss_message() {
            self.device_loss.notify(DeviceLostReason::Unknown, message);
        }
    }

    /// Whether the backend queue has no outstanding work. This does not wait
    /// or deliver error callbacks; callers should poll before inspecting it.
    pub fn is_queue_idle(&self) -> bool {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            self.metal.is_last_cmd_completed()
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            true
        }
    }

    pub fn generate_allocator_report(&self) -> Option<AllocatorReport> {
        None
    }

    pub(crate) fn record_error(&self, err: Error) {
        dispatch_error(&self.error_stack, &self.uncaptured_error_handler, err);
    }

    pub fn poll(&self, poll_type: PollType) -> Result<PollStatus, PollError> {
        self.refresh_device_loss();
        match poll_type {
            PollType::Poll => {
                self.metal.fire_work_done_callbacks();
                Ok(PollStatus::Poll)
            }
            PollType::Wait {
                submission_index,
                timeout,
            } => {
                if let Some(sub_idx) = submission_index {
                    let current = self.metal.next_submission_index.load(Ordering::Relaxed);
                    if sub_idx.0 >= current {
                        return Err(PollError::Timeout);
                    }
                }
                if let Some(dur) = timeout {
                    if dur.is_zero() && !self.metal.is_last_cmd_completed() {
                        return Err(PollError::Timeout);
                    }
                }
                self.metal.wait_idle();
                self.refresh_device_loss();
                self.metal.fire_work_done_callbacks();
                Ok(PollStatus::WaitSucceeded)
            }
        }
    }

    pub fn create_buffer(&self, desc: &BufferDescriptor<'_>) -> Buffer {
        let missing_ray_query = desc
            .usage
            .intersects(BufferUsages::BLAS_INPUT | BufferUsages::TLAS_INPUT)
            && !self.features.contains(Features::EXPERIMENTAL_RAY_QUERY);
        let missing_mappable_primary = !self.features.contains(Features::MAPPABLE_PRIMARY_BUFFERS)
            && ((desc.usage.contains(BufferUsages::MAP_READ)
                && !(desc.usage - BufferUsages::MAP_READ - BufferUsages::COPY_DST).is_empty())
                || (desc.usage.contains(BufferUsages::MAP_WRITE)
                    && !(desc.usage - BufferUsages::MAP_WRITE - BufferUsages::COPY_SRC)
                        .is_empty()));
        if desc.usage.is_empty()
            || desc.size > self.limits.max_buffer_size
            || missing_ray_query
            || missing_mappable_primary
        {
            let msg = format!(
                "Invalid buffer descriptor: size={}, usage={:?}, enabled_features={:?}",
                desc.size, desc.usage, self.features
            );
            self.record_error(Error::Validation {
                source: Arc::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    msg.clone(),
                )),
                description: msg,
            });
        }
        if desc.mapped_at_creation || desc.size >= 1_048_576 {
            self.metal.wait_idle();
        }
        let options = 0u64; // MTLResourceStorageModeShared: zero-copy UMA
        let (raw, ptr) = self.metal.alloc_buffer(desc.size as usize, options);
        Buffer {
            inner: Arc::new(BufferInner {
                device: self.metal.clone(),
                raw,
                ptr,
                size: desc.size,
                usage: desc.usage,
                options,
                mapped_ready: AtomicBool::new(desc.mapped_at_creation),
            }),
        }
    }

    /// ASD (Accelerated Storage Direct / Apple Silicon Direct) zero-copy page-aligned buffer creation
    pub unsafe fn create_buffer_no_copy(
        &self,
        page_aligned_ptr: *mut u8,
        page_aligned_size: usize,
        logical_size: u64,
        usage: BufferUsages,
    ) -> Option<Buffer> {
        let options = 0u64; // MTLResourceStorageModeShared
        let (raw, ptr) = self.metal.alloc_buffer_no_copy(
            page_aligned_ptr as *mut c_void,
            page_aligned_size,
            options,
        )?;
        Some(Buffer {
            inner: Arc::new(BufferInner {
                device: self.metal.clone(),
                raw,
                ptr,
                size: logical_size,
                usage,
                options: 0xFF, // Do not recycle external no-copy memory into pool
                mapped_ready: AtomicBool::new(true),
            }),
        })
    }

    pub fn create_shader_module(&self, desc: ShaderModuleDescriptor<'_>) -> ShaderModule {
        let wgsl_src = match &desc.source {
            ShaderSource::Wgsl(s) => s.as_ref(),
        };
        let module = match naga::front::wgsl::parse_str(wgsl_src) {
            Ok(m) => m,
            Err(e) => {
                self.record_error(Error::Validation {
                    source: Arc::new(e.clone()),
                    description: e.to_string(),
                });
                naga::Module::default()
            }
        };
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        let info = validator.validate(&module).unwrap_or_else(|_| {
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::empty(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&naga::Module::default())
            .unwrap()
        });
        let (msl_source, bindings) = naga::back::msl::write_string(
            &module,
            &info,
            &naga::back::msl::Options::default(),
            &naga::back::msl::PipelineOptions::default(),
        )
        .unwrap_or_else(|_| {
            (
                String::new(),
                BindingMap {
                    buffers: Default::default(),
                    textures: Default::default(),
                    samplers: Default::default(),
                    immediate_slot: None,
                    array_length_slots: Default::default(),
                },
            )
        });

        ShaderModule {
            module: Arc::new(module),
            info: Arc::new(info),
            msl_source: Arc::new(msl_source),
            bindings: Arc::new(bindings),
        }
    }

    pub fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDescriptor<'_>,
    ) -> BindGroupLayout {
        BindGroupLayout {
            entries: desc.entries.to_vec(),
        }
    }

    pub fn create_bind_group(&self, desc: &BindGroupDescriptor<'_>) -> BindGroup {
        let mut buffers = Vec::new();
        let mut textures = Vec::new();
        let mut samplers = Vec::new();

        for entry in desc.entries {
            match &entry.resource {
                BindingResource::Buffer(b) => {
                    let byte_size = b
                        .size
                        .map(|s| s.get())
                        .unwrap_or_else(|| b.buffer.size().saturating_sub(b.offset));
                    buffers.push((entry.binding, b.buffer.clone(), b.offset, byte_size));
                }
                BindingResource::TextureView(tv) => {
                    textures.push((entry.binding, tv.texture.clone()));
                }
                BindingResource::Sampler(s) => {
                    samplers.push((entry.binding, (*s).clone()));
                }
            }
        }

        BindGroup {
            buffers: buffers.into(),
            textures: textures.into(),
            samplers: samplers.into(),
        }
    }

    pub fn create_pipeline_layout(&self, desc: &PipelineLayoutDescriptor<'_>) -> PipelineLayout {
        PipelineLayout {
            immediate_size: desc.immediate_size,
        }
    }

    pub fn create_compute_pipeline(&self, desc: &ComputePipelineDescriptor<'_>) -> ComputePipeline {
        let ep_name = desc
            .entry_point
            .or_else(|| {
                desc.module
                    .module
                    .entry_points
                    .iter()
                    .find(|ep| ep.stage == naga::ShaderStage::Compute)
                    .map(|ep| ep.name.as_str())
            })
            .unwrap_or("main");

        let wg_size = desc
            .module
            .module
            .entry_points
            .iter()
            .find(|ep| ep.name == ep_name)
            .map(|ep| ep.workgroup_size)
            .unwrap_or([64, 1, 1]);

        let msl_ep_name = if ep_name == "main" {
            "_wgpu_entry_main"
        } else {
            ep_name
        };

        let wg_total = wg_size[0].max(1) * wg_size[1].max(1) * wg_size[2].max(1);
        let pso = self
            .metal
            .compile_compute_pipeline(
                &metal::msl_for_pipeline(
                    &desc.module.msl_source,
                    desc.compilation_options.constants,
                ),
                msl_ep_name,
                wg_total,
            )
            .expect("Metal compute pipeline compilation failed");

        ComputePipeline {
            pso,
            workgroup_size: wg_size,
            bindings: desc.module.bindings.clone(),
        }
    }

    #[inline]
    pub fn create_render_pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> RenderPipeline {
        self.metal.create_render_pipeline(desc)
    }

    #[inline]
    pub fn create_texture(&self, desc: &TextureDescriptor<'_>) -> Texture {
        self.metal.create_texture(desc)
    }

    #[inline]
    pub fn create_sampler(&self, desc: &SamplerDescriptor<'_>) -> Sampler {
        self.metal.create_sampler(desc)
    }

    pub fn create_query_set(&self, desc: &QuerySetDescriptor<'_>) -> QuerySet {
        QuerySet {
            count: desc.count,
            timestamps_ns: Arc::new(Mutex::new(vec![0u64; desc.count as usize])),
        }
    }

    pub fn create_command_encoder(&self, _desc: &CommandEncoderDescriptor<'_>) -> CommandEncoder {
        let ops = self.metal.take_cmd_ops();
        CommandEncoder {
            device: self.metal.clone(),
            error_stack: self.error_stack.clone(),
            uncaptured_error_handler: self.uncaptured_error_handler.clone(),
            ops,
            on_submit_callbacks: Mutex::new(Vec::new()),
        }
    }

    /// Creates a Bottom-Level Acceleration Structure (`BLAS`) for hardware RTX / Ray Tracing.
    pub fn create_blas(&self, label: Label<'_>, geometries: Vec<BlasGeometryDescriptor>) -> Blas {
        Blas::new(label, geometries)
    }

    /// Creates a Top-Level Acceleration Structure (`TLAS`) over scene instances for hardware RTX / Ray Tracing.
    pub fn create_tlas(&self, label: Label<'_>, instances: Vec<TlasInstance>) -> Tlas {
        Tlas::new(label, instances)
    }

    /// Queries the hardware RTX & Ray Tracing capabilities of this GPU device.
    pub fn rtx_capabilities(&self) -> RtxCapabilities {
        RtxCapabilities::for_backend(self.info.backend)
    }
}

impl Queue {
    #[inline]
    pub fn write_buffer(&self, buffer: &Buffer, offset: BufferAddress, data: &[u8]) {
        self.metal.write_buffer_with_error_sink(
            buffer,
            offset,
            data,
            Some(self.error_sink.capture()),
        );
    }

    #[inline]
    pub fn write_buffer_with<'a>(
        &'a self,
        buffer: &'a Buffer,
        offset: BufferAddress,
        size: BufferSize,
    ) -> Option<WriteOnly<'a, [u8]>> {
        let len = size.get() as usize;
        if offset as usize + len > buffer.inner.size as usize || buffer.inner.ptr.is_null() {
            return None;
        }
        let slice =
            unsafe { std::slice::from_raw_parts_mut(buffer.inner.ptr.add(offset as usize), len) };
        Some(WriteOnly::new(slice))
    }

    #[inline]
    pub fn write_texture(
        &self,
        texture: TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) {
        self.metal.write_texture(texture, data, data_layout, size);
    }

    #[inline]
    pub fn submit<I: IntoIterator<Item = CommandBuffer>>(
        &self,
        command_buffers: I,
    ) -> SubmissionIndex {
        self.metal
            .submit_with_error_sink(command_buffers, Some(self.error_sink.capture()))
    }

    /// Presents a surface texture; same as [`SurfaceTexture::present`].
    pub fn present(&self, surface_texture: SurfaceTexture) {
        surface_texture.present();
    }

    pub fn on_submitted_work_done<F>(&self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.metal
            .work_done_callbacks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Box::new(callback));
    }

    pub fn get_timestamp_period(&self) -> f32 {
        1.0
    }
}

#[cfg(test)]
mod error_delivery_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn validation(message: &str) -> Error {
        Error::Validation {
            source: Arc::new(std::io::Error::other(message.to_owned())),
            description: message.into(),
        }
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    #[ignore = "requires a real Metal adapter"]
    fn distinct_logical_devices_have_distinct_queues_on_one_adapter() {
        use std::future::Future;
        let adapter =
            Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
        let descriptor = DeviceDescriptor::default();
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let mut first = std::pin::pin!(adapter.request_device(&descriptor));
        let std::task::Poll::Ready(Ok((device_a, queue_a))) = first.as_mut().poll(&mut context)
        else {
            panic!("request_device must complete")
        };
        let mut second = std::pin::pin!(adapter.request_device(&descriptor));
        let std::task::Poll::Ready(Ok((device_b, queue_b))) = second.as_mut().poll(&mut context)
        else {
            panic!("request_device must complete")
        };
        assert_ne!(device_a, device_b);
        assert_ne!(
            queue_a, queue_b,
            "separate logical devices must not share queue identity"
        );
        assert_eq!(queue_a, queue_a.clone());
        assert_eq!(queue_b, queue_b.clone());
        let seen_a = Arc::new(AtomicUsize::new(0));
        let out_a = seen_a.clone();
        let seen_b = Arc::new(AtomicUsize::new(0));
        let out_b = seen_b.clone();
        device_a.on_uncaptured_error(Arc::new(move |_| {
            out_a.fetch_add(1, Ordering::Relaxed);
        }));
        device_b.on_uncaptured_error(Arc::new(move |_| {
            out_b.fetch_add(1, Ordering::Relaxed);
        }));
        device_a.record_error(validation("device a only"));
        assert_eq!(seen_a.load(Ordering::Relaxed), 1);
        assert_eq!(seen_b.load(Ordering::Relaxed), 0);
        drop(device_a);
        assert_eq!(
            queue_a,
            queue_a.clone(),
            "queue identity survives device handle drop"
        );
        eprintln!(
            "QUEUE_IDENTITY_RESULT logical_devices=2 shared_adapter=true isolated_errors=true clone_stable=true"
        );
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    #[ignore = "requires Metal; injects completion failures, not a hardware fault"]
    fn command_completion_routes_to_submit_owner_with_scope_and_reentry() {
        use std::future::Future;
        let adapter =
            Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
        let descriptor = DeviceDescriptor::default();
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let mut first = std::pin::pin!(adapter.request_device(&descriptor));
        let std::task::Poll::Ready(Ok((device_a, queue_a))) = first.as_mut().poll(&mut context)
        else {
            panic!("request_device")
        };
        let mut second = std::pin::pin!(adapter.request_device(&descriptor));
        let std::task::Poll::Ready(Ok((device_b, queue_b))) = second.as_mut().poll(&mut context)
        else {
            panic!("request_device")
        };
        let seen = Arc::new(AtomicUsize::new(0));
        let output = seen.clone();
        let device_a = Arc::new(device_a);
        let weak = Arc::downgrade(&device_a);
        device_a.on_uncaptured_error(Arc::new(move |error| {
            assert!(matches!(error, Error::Internal { .. }));
            output.fetch_add(1, Ordering::Relaxed);
            weak.upgrade().unwrap().poll(PollType::Poll).unwrap();
        }));
        device_b.on_uncaptured_error(Arc::new(|_| panic!("wrong logical owner")));
        let buffer = device_a.create_buffer(&BufferDescriptor {
            label: Some("command owner probe"),
            size: 64,
            usage: BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device_a.create_command_encoder(&CommandEncoderDescriptor::default());
        encoder.clear_buffer(&buffer, 0, None);
        queue_a.submit([encoder.finish()]);
        assert!(
            device_a
                .metal
                .last_command_owned_by_for_test(&queue_a.error_sink)
        );
        let shader = device_a.create_shader_module(ShaderModuleDescriptor {
            label: Some("owner compute"),
            source: ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
        });
        let pipeline = device_a.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("owner compute"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: PipelineCompilationOptions::default(),
            cache: None,
        });
        for (device, queue) in [(&*device_a, &queue_a), (&device_b, &queue_b)] {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor::default());
                pass.set_pipeline(&pipeline);
                pass.dispatch_workgroups(1, 1, 1);
            }
            queue.submit([encoder.finish()]);
            assert!(
                device
                    .metal
                    .last_command_owned_by_for_test(&queue.error_sink),
                "pure compute lost logical owner"
            );
        }
        // A different logical device can reap shared backend completions.
        device_a
            .metal
            .inject_command_failure_for_test(queue_a.error_sink.clone(), "owner a failure");
        device_b.poll(PollType::Poll).unwrap();
        device_b.poll(PollType::Poll).unwrap();
        assert_eq!(seen.load(Ordering::Relaxed), 1);
        let guard = device_a.push_error_scope(ErrorFilter::Internal);
        device_a
            .metal
            .inject_command_failure_for_test(queue_a.error_sink.clone(), "captured completion");
        device_b.poll(PollType::Poll).unwrap();
        assert_eq!(seen.load(Ordering::Relaxed), 1);
        assert!(
            device_a
                .error_stack
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .error()
                .as_ref()
                .unwrap()
                .to_string()
                .contains("captured completion")
        );
        let mut popped = std::pin::pin!(guard.pop());
        assert!(matches!(
            popped.as_mut().poll(&mut context),
            std::task::Poll::Ready(Some(Error::Internal { .. }))
        ));
        let oom_guard = device_a.push_error_scope(ErrorFilter::OutOfMemory);
        device_a.metal.inject_classified_command_failure_for_test(
            queue_a.error_sink.clone(),
            true,
            8,
        );
        device_b.poll(PollType::Poll).unwrap();
        assert!(matches!(
            device_a.error_stack.lock().unwrap().last().unwrap().error(),
            Some(Error::OutOfMemory { .. })
        ));
        assert_eq!(seen.load(Ordering::Relaxed), 1);
        // Another NSError domain's code8 must bypass the OOM scope.
        device_a.metal.inject_classified_command_failure_for_test(
            queue_a.error_sink.clone(),
            false,
            8,
        );
        device_b.poll(PollType::Poll).unwrap();
        assert_eq!(seen.load(Ordering::Relaxed), 2);
        let mut popped_oom = std::pin::pin!(oom_guard.pop());
        assert!(matches!(
            popped_oom.as_mut().poll(&mut context),
            std::task::Poll::Ready(Some(Error::OutOfMemory { .. }))
        ));
        device_b.poll(PollType::Poll).unwrap();
        assert_eq!(seen.load(Ordering::Relaxed), 2);
        let guard = device_a.push_error_scope(ErrorFilter::Internal);
        let mut encoder = device_a.create_command_encoder(&CommandEncoderDescriptor::default());
        encoder.clear_buffer(&buffer, 0, None);
        queue_a.submit([encoder.finish()]);
        device_a
            .metal
            .inject_command_failure_for_test(queue_a.error_sink.clone(), "async captured error");
        let mut pending = std::pin::pin!(guard.pop());
        let newer = device_a.push_error_scope(ErrorFilter::Internal);
        struct ReentrantWake {
            device: Arc<Device>,
            thread: std::thread::Thread,
        }
        impl std::task::Wake for ReentrantWake {
            fn wake(self: Arc<Self>) {
                self.device.poll(PollType::Poll).unwrap();
                self.thread.unpark();
            }
        }
        let waker = std::task::Waker::from(Arc::new(ReentrantWake {
            device: device_a.clone(),
            thread: std::thread::current(),
        }));
        let mut async_context = std::task::Context::from_waker(&waker);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let std::task::Poll::Ready(error) = pending.as_mut().poll(&mut async_context) {
                assert!(error.unwrap().to_string().contains("async captured error"));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "scope future never completed"
            );
            std::thread::park_timeout(std::time::Duration::from_millis(100));
        }
        let mut newer = std::pin::pin!(newer.pop());
        assert!(matches!(
            newer.as_mut().poll(&mut context),
            std::task::Poll::Ready(None)
        ));
        let success = device_a.push_error_scope(ErrorFilter::Validation);
        let mut encoder = device_a.create_command_encoder(&CommandEncoderDescriptor::default());
        encoder.clear_buffer(&buffer, 0, None);
        queue_a.submit([encoder.finish()]);
        let mut success = std::pin::pin!(success.pop());
        loop {
            if let std::task::Poll::Ready(error) = success.as_mut().poll(&mut async_context) {
                assert!(error.is_none());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "successful scope never completed"
            );
            std::thread::park_timeout(std::time::Duration::from_millis(100));
        }
        while device_a
            .metal
            .scope_completion_service
            .native_notifications()
            == 0
        {
            assert!(
                std::time::Instant::now() < deadline,
                "native completion signal never arrived"
            );
            std::thread::park_timeout(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            device_a.metal.blocking_wait_calls.load(Ordering::Relaxed),
            0,
            "async completion entered a blocking backend wait"
        );
        eprintln!(
            "NATIVE_SCOPE_SIGNAL_RESULT callbacks=true blocking_wait_calls=0 successful_and_failed_scopes=true"
        );
        eprintln!(
            "ASYNC_SCOPE_RESULT captured_at_submit=true pop_waits=true completion_worker=true reentrant_waker=true newer_scope_clean=true physical_fault=false"
        );
        eprintln!(
            "COMMAND_COMPLETION_OOM_RESULT scoped_oom=true foreign_domain_internal=true owner_isolation=true duplicate=false physical_fault=false"
        );
        eprintln!(
            "COMMAND_COMPLETION_RESULT owner_isolation=true callback_reentry=true duplicate=false scope_capture=true physical_fault=false"
        );
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    #[ignore = "requires Metal; holds synthetic submit leases to test completion service bursts"]
    fn scope_completion_service_coalesces_bursts_and_retires_cancelled_requests() {
        use std::future::Future;
        let adapter =
            Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
        let descriptor = DeviceDescriptor::default();
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let mut request = std::pin::pin!(adapter.request_device(&descriptor));
        let std::task::Poll::Ready(Ok((device, queue))) = request.as_mut().poll(&mut context)
        else {
            panic!("request_device")
        };
        struct Notify(std::thread::Thread);
        impl std::task::Wake for Notify {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = std::task::Waker::from(Arc::new(Notify(std::thread::current())));
        let mut context = std::task::Context::from_waker(&waker);
        for round in 0..2 {
            let mut leases = Vec::new();
            let mut futures = Vec::new();
            for _ in 0..64 {
                let guard = device.push_error_scope(ErrorFilter::Validation);
                leases.push(queue.error_sink.capture());
                futures.push(Some(Box::pin(guard.pop())));
            }
            for future in &mut futures {
                assert!(
                    future
                        .as_mut()
                        .unwrap()
                        .as_mut()
                        .poll(&mut context)
                        .is_pending()
                );
            }
            assert_eq!(
                device.metal.scope_completion_service.stats().1,
                round + 1,
                "burst spawned more than one worker"
            );
            for (i, future) in futures.iter_mut().enumerate() {
                if i % 2 == 1 {
                    *future = None;
                }
            }
            for (i, lease) in leases.into_iter().enumerate() {
                lease.command_failure(validation(&format!("burst-{round}-{i}")));
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                for (i, future) in futures.iter_mut().enumerate() {
                    if let Some(pending) = future {
                        if let std::task::Poll::Ready(error) = pending.as_mut().poll(&mut context) {
                            assert!(
                                error
                                    .unwrap()
                                    .to_string()
                                    .contains(&format!("burst-{round}-{i}"))
                            );
                            *future = None;
                        }
                    }
                }
                let (running, starts, waiting, capacity) =
                    device.metal.scope_completion_service.stats();
                if futures.iter().all(Option::is_none) && !running {
                    assert_eq!(waiting, 0);
                    assert_eq!(capacity, 0, "idle service retained registration storage");
                    assert_eq!(starts, round + 1);
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "completion service did not retire"
                );
                std::thread::park_timeout(std::time::Duration::from_millis(1));
            }
        }
        let mut leases = Vec::new();
        let mut cancelled = Vec::new();
        for _ in 0..64 {
            let guard = device.push_error_scope(ErrorFilter::Validation);
            leases.push(queue.error_sink.capture());
            let mut future = Box::pin(guard.pop());
            assert!(future.as_mut().poll(&mut context).is_pending());
            cancelled.push(future);
        }
        drop(cancelled);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let (running, _, waiting, capacity) = device.metal.scope_completion_service.stats();
            if !running {
                assert_eq!(waiting, 0);
                assert_eq!(capacity, 0);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "cancelled registrations kept service running while leases remained live"
            );
            std::thread::park_timeout(std::time::Duration::from_millis(1));
        }
        assert_eq!(leases.len(), 64);
        drop(leases);
        let retired = device.metal.scope_completion_service.stats();
        device.metal.scope_completion_service.native_completed();
        assert_eq!(
            device.metal.scope_completion_service.stats(),
            retired,
            "late completion restarted an idle service"
        );
        eprintln!("LATE_SCOPE_SIGNAL_RESULT retired_service_unchanged=true no_new_worker=true");
        eprintln!(
            "SCOPE_CANCELLATION_RESULT cancelled=64 live_submission_leases=64 service_retired_before_lease_release=true"
        );
        eprintln!(
            "SCOPE_SERVICE_RESULT rounds=2 scopes_per_round=64 cancelled_per_round=32 workers_per_burst=1 retired=true idle_capacity=0 synthetic_leases=true"
        );
    }
    #[test]
    fn scope_guards_pop_their_own_errors_in_either_order() {
        use std::future::Future;
        for outer_first in [false, true] {
            let stack = Arc::new(Mutex::new(Vec::new()));
            let handler = Mutex::new(None);
            let outer = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
            dispatch_error(&stack, &handler, validation("outer"));
            let inner = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
            dispatch_error(&stack, &handler, validation("inner"));
            let (first, second, expected_first, expected_second) = if outer_first {
                (outer, inner, "outer", "inner")
            } else {
                (inner, outer, "inner", "outer")
            };
            let mut context = std::task::Context::from_waker(std::task::Waker::noop());
            let mut popped = std::pin::pin!(first.pop());
            let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context) else {
                panic!("scope error")
            };
            assert!(error.to_string().contains(expected_first));
            assert_eq!(stack.lock().unwrap().len(), 1);
            // An unrelated newly-pushed scope must survive the old guard pop.
            let unrelated = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Internal);
            let mut popped = std::pin::pin!(second.pop());
            let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context) else {
                panic!("scope error")
            };
            assert!(error.to_string().contains(expected_second));
            assert_eq!(stack.lock().unwrap().len(), 1);
            assert_eq!(stack.lock().unwrap()[0].filter, ErrorFilter::Internal);
            let mut popped = std::pin::pin!(unrelated.pop());
            assert!(matches!(
                popped.as_mut().poll(&mut context),
                std::task::Poll::Ready(None)
            ));
            assert!(stack.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn concurrent_scope_pops_preserve_each_guard_error() {
        use std::future::Future;
        let stack = Arc::new(Mutex::new(Vec::new()));
        let handler = Mutex::new(None);
        let guards: Vec<_> = (0..8)
            .map(|i| {
                let guard = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
                let message = format!("scope-{i}");
                dispatch_error(&stack, &handler, validation(&message));
                (guard, message)
            })
            .collect();
        let threads: Vec<_> = guards
            .into_iter()
            .map(|(guard, message)| {
                std::thread::spawn(move || {
                    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
                    let mut popped = std::pin::pin!(guard.pop());
                    let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context)
                    else {
                        panic!("scope error")
                    };
                    assert!(error.to_string().contains(&message));
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert!(stack.lock().unwrap().is_empty());
    }
    #[test]
    fn submitted_scope_survives_pop_and_new_scope_with_waker_notification() {
        use std::future::Future;
        struct WakeCount(AtomicUsize);
        impl std::task::Wake for WakeCount {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let stack = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(Mutex::new(None));
        let guard = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
        let sink = CommandErrorSink {
            stack: stack.clone(),
            handler,
            scopes: None,
            captured: false,
        }
        .capture();
        let part = sink.clone();
        let mut popped = std::pin::pin!(guard.pop());
        let newer = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
        let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(wakes.clone());
        let mut context = std::task::Context::from_waker(&waker);
        assert!(popped.as_mut().poll(&mut context).is_pending());
        sink.command_failure(validation("submitted before pop"));
        drop(sink);
        assert!(
            popped.as_mut().poll(&mut context).is_pending(),
            "another command part still owns this submission"
        );
        drop(part);
        assert!(wakes.0.load(Ordering::Relaxed) > 0);
        let std::task::Poll::Ready(Some(error)) = popped.as_mut().poll(&mut context) else {
            panic!("captured error")
        };
        assert!(error.to_string().contains("submitted before pop"));
        let mut newer = std::pin::pin!(newer.pop());
        assert!(matches!(
            newer.as_mut().poll(&mut context),
            std::task::Poll::Ready(None)
        ));
    }
    #[test]
    fn cancelled_scope_pop_releases_waker_before_submission_lease() {
        use std::future::Future;
        struct Resource {
            entry: crate::types::ErrorScopeEntry,
            dropped: Arc<AtomicUsize>,
        }
        impl Drop for Resource {
            fn drop(&mut self) {
                let _ = self.entry.error();
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
        struct TaskWaker {
            _resource: Arc<Resource>,
            calls: Arc<AtomicUsize>,
        }
        impl std::task::Wake for TaskWaker {
            fn wake(self: Arc<Self>) {
                self.calls.fetch_add(1, Ordering::Relaxed);
            }
        }
        let stack = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(Mutex::new(None));
        let guard = ErrorScopeGuard::push(stack.clone(), ErrorFilter::Validation);
        let sink = CommandErrorSink {
            stack,
            handler,
            scopes: None,
            captured: false,
        }
        .capture();
        let dropped = Arc::new(AtomicUsize::new(0));
        let resource = Arc::new(Resource {
            entry: sink.scopes.as_ref().unwrap().0[0].clone(),
            dropped: dropped.clone(),
        });
        let weak = Arc::downgrade(&resource);
        let calls = Arc::new(AtomicUsize::new(0));
        let waker = std::task::Waker::from(Arc::new(TaskWaker {
            _resource: resource,
            calls: calls.clone(),
        }));
        let mut future = Box::pin(guard.pop());
        {
            let mut context = std::task::Context::from_waker(&waker);
            assert!(future.as_mut().poll(&mut context).is_pending());
        }
        drop(waker);
        assert!(
            weak.upgrade().is_some(),
            "pending future must retain its task notification"
        );
        drop(future);
        assert!(
            weak.upgrade().is_none(),
            "cancelled future retained executor resource through live submission"
        );
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            1,
            "task destructor must reenter scope state without deadlock"
        );
        sink.command_failure(validation("late error after cancellation"));
        assert!(
            sink.scopes.as_ref().unwrap().0[0]
                .error()
                .unwrap()
                .to_string()
                .contains("late error after cancellation")
        );
        drop(sink);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "completion woke a cancelled task"
        );
    }
    #[test]
    fn submission_without_scopes_does_not_capture_a_later_scope() {
        use std::future::Future;
        let stack = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let output = calls.clone();
        let handler: Arc<Mutex<Option<UncapturedErrorHandler>>> =
            Arc::new(Mutex::new(Some(Arc::new(move |_| {
                output.fetch_add(1, Ordering::Relaxed);
            }))));
        let sink = CommandErrorSink {
            stack: stack.clone(),
            handler,
            scopes: None,
            captured: false,
        }
        .capture();
        assert!(
            sink.scopes.is_none(),
            "empty scope capture must not allocate a snapshot"
        );
        let newer = ErrorScopeGuard::push(stack, ErrorFilter::Validation);
        sink.command_failure(validation("unscoped submission"));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let mut popped = std::pin::pin!(newer.pop());
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(matches!(
            popped.as_mut().poll(&mut context),
            std::task::Poll::Ready(None)
        ));
    }
    #[test]
    fn matching_innermost_scope_captures_first_without_uncaptured_delivery() {
        let stack = Mutex::new(vec![
            crate::types::ErrorScopeEntry::new(ErrorFilter::Validation),
            crate::types::ErrorScopeEntry::new(ErrorFilter::OutOfMemory),
            crate::types::ErrorScopeEntry::new(ErrorFilter::Validation),
        ]);
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let handler: Mutex<Option<UncapturedErrorHandler>> =
            Mutex::new(Some(Arc::new(move |_| {
                seen.fetch_add(1, Ordering::Relaxed);
            })));
        dispatch_error(&stack, &handler, validation("first"));
        dispatch_error(&stack, &handler, validation("second"));
        let scopes = stack.lock().unwrap();
        assert!(scopes[0].error().is_none() && scopes[1].error().is_none());
        assert!(
            scopes[2]
                .error()
                .as_ref()
                .unwrap()
                .to_string()
                .contains("first")
        );
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn uncaptured_handler_can_replace_itself_and_reenter_error_scopes() {
        let stack = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Mutex<Option<UncapturedErrorHandler>>> = Arc::new(Mutex::new(None));
        let weak = Arc::downgrade(&handler);
        let nested_stack = stack.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        *handler.lock().unwrap() = Some(Arc::new(move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
            let holder = weak.upgrade().unwrap();
            *holder.lock().unwrap() = None;
            nested_stack
                .lock()
                .unwrap()
                .push(crate::types::ErrorScopeEntry::new(ErrorFilter::Validation));
            dispatch_error(&nested_stack, &holder, validation("nested"));
        }));
        dispatch_error(&stack, &handler, validation("outer"));
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(handler.lock().unwrap().is_none());
        assert!(
            stack.lock().unwrap()[0]
                .error()
                .as_ref()
                .unwrap()
                .to_string()
                .contains("nested")
        );
    }
}

#[cfg(test)]
mod device_loss_delivery_tests {
    use super::*;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    #[ignore = "requires Metal; injects a logical backend notification, not physical device removal"]
    fn poll_delivers_backend_loss_to_late_callback_once_with_reentry() {
        use std::future::Future;
        let adapter =
            Adapter::from_metal(MetalDevice::create_system_default().expect("Metal required"));
        let descriptor = DeviceDescriptor::default();
        let mut request = std::pin::pin!(adapter.request_device(&descriptor));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let std::task::Poll::Ready(Ok((device, _queue))) = request.as_mut().poll(&mut context)
        else {
            panic!("native request_device must complete")
        };
        let device = Arc::new(device);
        device
            .metal
            .inject_device_loss_for_test("injected Metal backend removal".into());
        device.poll(PollType::Poll).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let output = seen.clone();
        let weak = Arc::downgrade(&device);
        device.set_device_lost_callback(move |reason, message| {
            output.lock().unwrap().push((reason, message));
            weak.upgrade().unwrap().poll(PollType::Poll).unwrap();
        });
        for _ in 0..5 {
            device.poll(PollType::Poll).unwrap();
        }
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(
                DeviceLostReason::Unknown,
                "injected Metal backend removal".into()
            )]
        );
        eprintln!(
            "DEVICE_LOSS_POLL_RESULT callback_once=true late_registration=true reentry=true physical_removal=false"
        );
    }

    #[test]
    fn late_registration_reentry_and_duplicate_loss_deliver_once() {
        let state = Arc::new(DeviceLossState::default());
        state.notify(DeviceLostReason::Unknown, "first backend loss".into());
        state.notify(DeviceLostReason::Destroyed, "later".into());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let output = seen.clone();
        let weak = Arc::downgrade(&state);
        state.set_callback(Arc::new(move |reason, message| {
            output.lock().unwrap().push((reason, message));
            // Reentry must not deadlock or issue a second callback.
            weak.upgrade()
                .unwrap()
                .notify(DeviceLostReason::Unknown, "reentrant".into());
        }));
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(DeviceLostReason::Unknown, "first backend loss".into())]
        );
        state.set_callback(Arc::new(|_, _| panic!("loss already delivered")));
        state.notify(DeviceLostReason::Destroyed, "duplicate".into());
    }
    #[test]
    fn registration_before_loss_observes_first_message_once() {
        let state = DeviceLossState::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let output = seen.clone();
        state.set_callback(Arc::new(move |reason, message| {
            output.lock().unwrap().push((reason, message))
        }));
        assert!(seen.lock().unwrap().is_empty());
        state.notify(DeviceLostReason::Unknown, "removed".into());
        state.notify(DeviceLostReason::Unknown, "revoked".into());
        assert_eq!(
            *seen.lock().unwrap(),
            vec![(DeviceLostReason::Unknown, "removed".into())]
        );
    }
}
