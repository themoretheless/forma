#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]
// Off Apple targets the Metal half of the crate is stubs, which leaves most of it unused.
#![cfg_attr(
    not(any(target_os = "macos", target_os = "ios")),
    allow(unused_variables, dead_code)
)]

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
//! - [`metal`], [`vulkan`], [`dx12`], [`cuda`], [`opengl`], [`webgpu`]: Hardware backend runtimes.

pub mod asd;
pub mod buffer;
pub mod command;
pub mod duma;
pub mod flags;
pub mod metal;
pub mod pipeline;
pub mod platform;
pub mod texture;
pub mod transfer;
pub mod types;
pub mod uma;
pub mod util;

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
pub use pipeline::*;
pub use platform::*;
pub use texture::*;
pub use transfer::*;
pub use types::*;
pub use uma::*;

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

/// The entry point for finding adapters. The descriptor selects nothing in this runtime: there
/// is one backend.
#[derive(Clone)]
pub struct Instance {}

impl Default for Instance {
    fn default() -> Self {
        Self::new(InstanceDescriptor::default())
    }
}

impl Instance {
    pub fn new(_desc: InstanceDescriptor) -> Self {
        Self {}
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
        // Only AppKit and UIKit views carry a Metal layer; any other window has none.
        let (metal_layer, metal_view) = match target.window_handle().map(|handle| handle.as_raw()) {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Ok(raw_window_handle::RawWindowHandle::AppKit(appkit)) => {
                let view_ptr = appkit.ns_view.as_ptr();
                (
                    unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, true) },
                    unsafe { crate::metal::ObjCObject::from_unretained(view_ptr) }.map(Arc::new),
                )
            }
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            Ok(raw_window_handle::RawWindowHandle::UiKit(uikit)) => {
                let view_ptr = uikit.ui_view.as_ptr();
                (
                    unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, false) },
                    unsafe { crate::metal::ObjCObject::from_unretained(view_ptr) }.map(Arc::new),
                )
            }
            _ => (None, None),
        };
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
            | Features::SUBGROUP;
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
) -> bool {
    let filter = match &error {
        Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
        Error::Validation { .. } => ErrorFilter::Validation,
        Error::Internal { .. } => ErrorFilter::Internal,
    };
    {
        let mut scopes = stack.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = scopes.iter_mut().rev().find(|entry| entry.filter == filter) {
            entry.record(error);
            return true;
        }
    }
    let callback = handler.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match callback {
        Some(callback) => {
            callback(error);
            true
        }
        None => false,
    }
}

/// Why a shader module or pipeline could not be built.
#[derive(Debug)]
struct ShaderError(String);

impl std::fmt::Display for ShaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShaderError {}

fn validation_error(description: String) -> Error {
    Error::Validation {
        source: Arc::new(ShaderError(description.clone())),
        description,
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

    /// Reports a pipeline that could not be created. The caller gets an invalid pipeline whose
    /// dispatches and draws are skipped.
    ///
    /// # Panics
    /// With no error scope and no uncaptured-error handler to receive it, the failure would
    /// otherwise go unnoticed.
    fn report_invalid_pipeline(&self, description: String) {
        let error = validation_error(description.clone());
        if !dispatch_error(&self.error_stack, &self.uncaptured_error_handler, error) {
            panic!("{description}");
        }
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
        let invalid_descriptor = desc.usage.is_empty()
            || desc.size > self.limits.max_buffer_size
            || missing_ray_query
            || missing_mappable_primary;
        if invalid_descriptor {
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
        if !invalid_descriptor && (desc.mapped_at_creation || desc.size >= 1_048_576) {
            self.metal.wait_idle();
        }
        let options = 0u64; // MTLResourceStorageModeShared: zero-copy UMA
        let allocated = if desc.size > self.limits.max_buffer_size {
            None
        } else {
            usize::try_from(desc.size)
                .ok()
                .and_then(|size| self.metal.alloc_buffer(size, options))
        };
        let (raw, ptr, stored_options) = match allocated {
            Some((raw, ptr)) => (raw, ptr, options),
            None => {
                if desc.size <= self.limits.max_buffer_size {
                    let msg = format!("Metal buffer allocation failed for size={}", desc.size);
                    self.record_error(Error::OutOfMemory {
                        source: Arc::new(std::io::Error::new(std::io::ErrorKind::OutOfMemory, msg)),
                    });
                }
                (
                    Arc::new(crate::metal::ObjCObject(std::ptr::null_mut())),
                    std::ptr::null_mut(),
                    0xFF,
                )
            }
        };
        Buffer {
            inner: Arc::new(BufferInner {
                device: self.metal.clone(),
                raw,
                ptr,
                size: desc.size,
                usage: desc.usage,
                options: stored_options,
                mapped_ready: AtomicBool::new(desc.mapped_at_creation),
                mapped_ranges: Default::default(),
            }),
        }
    }

    /// ASD (Accelerated Storage Direct / Apple Silicon Direct) zero-copy page-aligned buffer creation.
    ///
    /// # Safety
    ///
    /// `page_aligned_ptr` must be page-aligned for Metal `newBufferWithBytesNoCopy` and remain
    /// valid for `page_aligned_size` bytes until the returned `Buffer` is dropped on CPU and GPU.
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
                mapped_ranges: Default::default(),
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
        let info = validator.validate(&module).unwrap_or_else(|error| {
            self.record_error(validation_error(format!(
                "shader validation failed: {error}"
            )));
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
        .unwrap_or_else(|error| {
            self.record_error(validation_error(format!("MSL generation failed: {error}")));
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
            msl_source: Arc::new(msl_source),
            bindings: Arc::new(bindings),
        }
    }

    pub fn create_bind_group_layout(
        &self,
        _desc: &BindGroupLayoutDescriptor<'_>,
    ) -> BindGroupLayout {
        BindGroupLayout
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

    pub fn create_pipeline_layout(&self, _desc: &PipelineLayoutDescriptor<'_>) -> PipelineLayout {
        PipelineLayout
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
        let pso =
            metal::msl_for_pipeline(&desc.module.msl_source, desc.compilation_options.constants)
                .and_then(|msl| {
                    self.metal
                        .compile_compute_pipeline(&msl, msl_ep_name, wg_total)
                        .map_err(|error| {
                            format!("Metal compute pipeline compilation failed: {error}")
                        })
                })
                .map_err(|description| self.report_invalid_pipeline(description))
                .ok();

        ComputePipeline {
            pso,
            workgroup_size: wg_size,
            bindings: desc.module.bindings.clone(),
        }
    }

    #[inline]
    pub fn create_render_pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> RenderPipeline {
        self.metal
            .create_render_pipeline(desc)
            .unwrap_or_else(|description| {
                self.report_invalid_pipeline(description);
                RenderPipeline::invalid(desc)
            })
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
        if buffer.inner.ptr.is_null() {
            return None;
        }
        buffer.inner.write_view(offset, size.get())
    }

    #[inline]
    pub fn write_texture(
        &self,
        texture: TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) {
        if data.is_empty() || size.width == 0 || size.height == 0 {
            return;
        }
        let problem = texture
            .texture
            .region_error(texture.mip_level, texture.origin, size)
            .or_else(|| texture.texture.data_error(data.len(), &data_layout, size));
        if let Some(problem) = problem {
            self.error_sink
                .capture()
                .command_failure(validation_error(format!("write_texture: {problem}")));
            return;
        }
        if texture.texture.storage_mode != 0 {
            self.write_private_texture(texture, data, data_layout, size);
            return;
        }
        self.metal.write_texture(texture, data, data_layout, size);
    }

    /// Uploads to a texture the CPU cannot address (private storage, as every texture is on a
    /// device without unified memory): the data goes through a staging buffer and a blit.
    fn write_private_texture(
        &self,
        texture: TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) {
        // `write_texture` has checked that the layout fits `data`.
        let Some(payload) = usize::try_from(data_layout.offset)
            .ok()
            .and_then(|offset| data.get(offset..))
        else {
            return;
        };
        let sink = self.error_sink.capture();
        // MTLResourceStorageModeShared, so the bytes can be copied in from the CPU.
        let Some((raw, ptr)) = self.metal.alloc_buffer_with_bytes(payload, 0) else {
            sink.command_failure(Error::OutOfMemory {
                source: Arc::new(std::io::Error::new(
                    std::io::ErrorKind::OutOfMemory,
                    format!(
                        "write_texture: no staging buffer of {} bytes",
                        payload.len()
                    ),
                )),
            });
            return;
        };
        let staging = Buffer {
            inner: Arc::new(BufferInner {
                device: self.metal.clone(),
                raw,
                ptr,
                size: payload.len() as u64,
                usage: BufferUsages::COPY_SRC,
                options: 0,
                mapped_ready: AtomicBool::new(false),
                mapped_ranges: Default::default(),
            }),
        };
        let upload = CommandBuffer {
            ops: vec![RecordedOp::CopyBufferToTexture {
                src_buf: staging.clone(),
                src_offset: 0,
                bytes_per_row: data_layout.row_stride(size.width, texture.texture.format),
                rows_per_image: data_layout.rows_per_image,
                dst_tex: texture.texture.clone(),
                dst_mip: texture.mip_level,
                dst_origin: texture.origin,
                size,
            }],
            on_submit_callbacks: Vec::new(),
        };
        self.metal.submit_with_error_sink(Some(upload), Some(sink));
        // The staging buffer returns to the pool when dropped; the blit must have read it first.
        self.metal.wait_idle();
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
mod error_delivery_tests;

#[cfg(test)]
mod device_loss_delivery_tests;

#[cfg(test)]
mod asd_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod invalid_pipeline_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod layer_bounds_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod mapped_range_tests;

#[cfg(test)]
mod pipeline_cache_and_buffer_safety_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod copy_bounds_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod immediates_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod private_texture_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod query_resolve_tests;

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod storage_texture_tests;
