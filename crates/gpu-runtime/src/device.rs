//! Logical GPU device creation and resource factory methods.

#[cfg(any(target_os = "macos", target_os = "ios"))]
use crate::metal;
use crate::metal::MetalDevice;
use crate::{
    AdapterInfo, AllocatorReport, BindGroup, BindGroupDescriptor, BindGroupLayout,
    BindGroupLayoutDescriptor, Buffer, BufferDescriptor, BufferInner, BufferUsages, CommandEncoder,
    CommandEncoderDescriptor, ComputePipeline, ComputePipelineDescriptor, DeviceLossState,
    DeviceLostReason, Error, ErrorFilter, ErrorScopeGuard, Features, Limits, PipelineLayout,
    PipelineLayoutDescriptor, PollError, PollStatus, PollType, QuerySet, QuerySetDescriptor,
    RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerDescriptor, ShaderModule,
    ShaderModuleDescriptor, ShaderSource, Texture, TextureDescriptor, UncapturedErrorHandler,
    dispatch_error, validation_error,
};
use gpu_shader as naga;
#[cfg(any(target_os = "macos", target_os = "ios"))]
use gpu_shader::back::msl::BindingMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Device {
    pub(crate) metal: MetalDevice,
    pub(crate) info: Arc<AdapterInfo>,
    pub(crate) features: Features,
    pub(crate) limits: Limits,
    pub(crate) error_stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
    pub(crate) uncaptured_error_handler: Arc<Mutex<Option<UncapturedErrorHandler>>>,
    pub(crate) device_loss: Arc<DeviceLossState>,
    pub(crate) destroyed: Arc<AtomicBool>,
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

    /// Marks this logical device destroyed and delivers [`DeviceLostReason::Destroyed`] once
    /// without disturbing sibling logical devices created from the same adapter.
    pub fn destroy(&self) {
        if !self.destroyed.swap(true, Ordering::AcqRel) {
            self.device_loss.notify(
                DeviceLostReason::Destroyed,
                "Device was destroyed".to_string(),
            );
        }
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
                self.metal
                    .wait_for_submission(submission_index.map(|s| s.0), timeout)?;
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
            let _ = self.metal.wait_for_submission(None, None);
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
        // SAFETY: Caller upholds alignment, size, and lifetime invariants for `page_aligned_ptr`.
        let (raw, ptr) = unsafe {
            self.metal.alloc_buffer_no_copy(
                page_aligned_ptr as *mut c_void,
                page_aligned_size,
                options,
            )
        }?;
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
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        fn empty_binding_map() -> BindingMap {
            BindingMap {
                buffers: Default::default(),
                textures: Default::default(),
                samplers: Default::default(),
                immediate_slot: None,
                array_length_slots: Default::default(),
            }
        }

        let invalid_module = |_module: naga::Module, reason: String| ShaderModule {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            module: Arc::new(_module),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            msl_source: Arc::new(String::new()),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            bindings: Arc::new(empty_binding_map()),
            invalid_reason: Some(Arc::from(reason)),
        };

        let wgsl_src = match &desc.source {
            ShaderSource::Wgsl(s) => s.as_ref(),
        };
        let module = match naga::front::wgsl::parse_str(wgsl_src) {
            Ok(m) => m,
            Err(e) => {
                let description = e.to_string();
                self.record_error(Error::Validation {
                    source: Arc::new(e),
                    description: description.clone(),
                });
                return invalid_module(naga::Module::default(), description);
            }
        };
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        let info = match validator.validate(&module) {
            Ok(info) => info,
            Err(error) => {
                let description = format!("shader validation failed: {error}");
                self.record_error(validation_error(description.clone()));
                return invalid_module(module, description);
            }
        };
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let (msl_source, bindings) = match naga::back::msl::write_string(
            &module,
            &info,
            &naga::back::msl::Options::default(),
            &naga::back::msl::PipelineOptions::default(),
        ) {
            Ok(generated) => generated,
            Err(error) => {
                let description = format!("MSL generation failed: {error}");
                self.record_error(validation_error(description.clone()));
                return invalid_module(module, description);
            }
        };
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let _ = (module, info);

        ShaderModule {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            module: Arc::new(module),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            msl_source: Arc::new(msl_source),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            bindings: Arc::new(bindings),
            invalid_reason: None,
        }
    }

    pub fn create_bind_group_layout(
        &self,
        _desc: &BindGroupLayoutDescriptor<'_>,
    ) -> BindGroupLayout {
        BindGroupLayout
    }

    pub fn create_bind_group(&self, desc: &BindGroupDescriptor<'_>) -> BindGroup {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            use crate::BindingResource;
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
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = desc;
            BindGroup {}
        }
    }

    pub fn create_pipeline_layout(&self, _desc: &PipelineLayoutDescriptor<'_>) -> PipelineLayout {
        PipelineLayout
    }

    pub fn create_compute_pipeline(&self, desc: &ComputePipelineDescriptor<'_>) -> ComputePipeline {
        if let Some(reason) = &desc.module.invalid_reason {
            self.report_invalid_pipeline(format!(
                "compute pipeline uses invalid shader module: {reason}"
            ));
            return ComputePipeline {
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                pso: None,
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                workgroup_size: [64, 1, 1],
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                bindings: desc.module.bindings.clone(),
            };
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
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
            let pso = metal::msl_for_pipeline(
                &desc.module.msl_source,
                desc.compilation_options.constants,
            )
            .and_then(|msl| {
                self.metal
                    .compile_compute_pipeline(&msl, msl_ep_name, wg_total)
                    .map_err(|error| format!("Metal compute pipeline compilation failed: {error}"))
            })
            .map_err(|description| self.report_invalid_pipeline(description))
            .ok();

            ComputePipeline {
                pso,
                workgroup_size: wg_size,
                bindings: desc.module.bindings.clone(),
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            ComputePipeline {}
        }
    }

    #[inline]
    pub fn create_render_pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> RenderPipeline {
        if let Some(reason) = &desc.vertex.module.invalid_reason {
            self.report_invalid_pipeline(format!(
                "render pipeline uses invalid vertex shader module: {reason}"
            ));
            return RenderPipeline::invalid(desc);
        }
        if let Some(reason) = desc
            .fragment
            .as_ref()
            .and_then(|fs| fs.module.invalid_reason.as_ref())
        {
            self.report_invalid_pipeline(format!(
                "render pipeline uses invalid fragment shader module: {reason}"
            ));
            return RenderPipeline::invalid(desc);
        }
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
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            QuerySet {
                timestamps_ns: Arc::new(Mutex::new(vec![0u64; desc.count as usize])),
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            let _ = desc;
            QuerySet {}
        }
    }

    pub fn create_command_encoder(&self, _desc: &CommandEncoderDescriptor<'_>) -> CommandEncoder {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let ops = self.metal.take_cmd_ops();
        CommandEncoder {
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            device: self.metal.clone(),
            error_stack: self.error_stack.clone(),
            uncaptured_error_handler: self.uncaptured_error_handler.clone(),
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            ops,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            on_submit_callbacks: Mutex::new(Vec::new()),
        }
    }
}
