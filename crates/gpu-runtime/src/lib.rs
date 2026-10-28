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

use metal::MetalDevice;
use gpu_shader as naga;
use gpu_shader::back::msl::BindingMap;
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
            Err(RequestAdapterError("No supported GPU adapter found".to_string()))
        };
        std::future::ready(res)
    }

    pub fn create_surface<'a, W>(&self, _target: W) -> Result<Surface<'a>, RequestDeviceError> {
        Ok(Surface {
            config: Arc::new(Mutex::new(None)),
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
            name: metal.name.clone(),
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
        let mut limits = Limits::default();
        limits.max_buffer_size = metal.max_buffer_length;
        limits.max_storage_buffer_binding_size = metal.max_buffer_length;
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
            info: self.info.clone(),
            features: desc.required_features & self.features,
            limits: desc.required_limits.clone(),
            error_stack: Arc::new(Mutex::new(Vec::new())),
        };
        let queue = Queue {
            metal: self.metal.clone(),
        };
        std::future::ready(Ok((dev, queue)))
    }
}

#[derive(Clone)]
pub struct Device {
    pub(crate) metal: MetalDevice,
    pub(crate) info: AdapterInfo,
    pub(crate) features: Features,
    pub(crate) limits: Limits,
    pub(crate) error_stack: Arc<Mutex<Vec<(ErrorFilter, Option<Error>)>>>,
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
}

impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.metal.raw, &other.metal.raw)
            && Arc::ptr_eq(&self.metal.queue, &other.metal.queue)
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
        self.info.clone()
    }

    pub fn features(&self) -> Features {
        self.features
    }

    pub fn limits(&self) -> Limits {
        self.limits.clone()
    }

    pub fn push_error_scope(&self, filter: ErrorFilter) -> ErrorScopeGuard {
        if let Ok(mut stack) = self.error_stack.lock() {
            stack.push((filter, None));
        }
        ErrorScopeGuard {
            stack: self.error_stack.clone(),
        }
    }

    pub fn on_uncaptured_error(&self, _handler: Arc<dyn Fn(Error) + Send + Sync + 'static>) {}

    pub fn set_device_lost_callback(
        &self,
        _callback: impl Fn(DeviceLostReason, String) + Send + Sync + 'static,
    ) {
    }

    pub fn generate_allocator_report(&self) -> Option<AllocatorReport> {
        None
    }

    pub(crate) fn record_error(&self, err: Error) {
        let err_filter = match &err {
            Error::OutOfMemory { .. } => ErrorFilter::OutOfMemory,
            Error::Validation { .. } => ErrorFilter::Validation,
            Error::Internal { .. } => ErrorFilter::Internal,
        };
        if let Ok(mut stack) = self.error_stack.lock() {
            if let Some((_, slot)) = stack.iter_mut().rev().find(|(f, _)| *f == err_filter) {
                if slot.is_none() {
                    *slot = Some(err);
                }
            }
        }
    }

    pub fn poll(&self, poll_type: PollType) -> Result<PollStatus, PollError> {
        match poll_type {
            PollType::Poll => Ok(PollStatus::Poll),
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
                    && !(desc.usage - BufferUsages::MAP_WRITE - BufferUsages::COPY_SRC).is_empty()));
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
            buffers,
            textures,
            samplers,
        }
    }

    pub fn create_pipeline_layout(&self, desc: &PipelineLayoutDescriptor<'_>) -> PipelineLayout {
        PipelineLayout {
            immediate_size: desc.immediate_size,
        }
    }

    pub fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDescriptor<'_>,
    ) -> ComputePipeline {
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
            .compile_compute_pipeline(&desc.module.msl_source, msl_ep_name, wg_total)
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
        CommandEncoder {
            device: self.metal.clone(),
            error_stack: self.error_stack.clone(),
            ops: Vec::with_capacity(16),
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
        self.metal.write_buffer(buffer, offset, data);
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
        let slice = unsafe {
            std::slice::from_raw_parts_mut(buffer.inner.ptr.add(offset as usize), len)
        };
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
        self.metal.submit(command_buffers)
    }

    pub fn on_submitted_work_done<F>(&self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.metal.wait_idle();
        callback();
    }

    pub fn get_timestamp_period(&self) -> f32 {
        1.0
    }
}
