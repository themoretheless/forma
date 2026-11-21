//! Portable multi-backend GPU implementation backed by `upstream-wgpu` (`Vulkan`, `DX12`,
//! `OpenGL / GLES`, `Browser WebGPU`, and test-only `Noop`).

pub(crate) mod buffer;
pub(crate) mod command;
pub(crate) mod convert;
pub(crate) mod pipeline;
pub(crate) mod surface;

use crate::{
    AdapterInfo, Backends, DeviceLossState, Error, Features, InstanceDescriptor, Limits, PollError,
    PollStatus, PollType, RequestDeviceError, SubmissionIndex, UncapturedErrorHandler,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{DeviceDescriptor, RequestAdapterOptions};
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Runtime handle wrapping an `upstream_wgpu` device/queue for portable backends.
pub(crate) struct PortableDevice {
    pub(crate) backend: crate::Backend,
    pub(crate) name: Arc<str>,
    pub(crate) device: upstream_wgpu::Device,
    pub(crate) queue: upstream_wgpu::Queue,
    pub(crate) injected_loss: Mutex<Option<String>>,
    pub(crate) portable_shader_compilations: Arc<AtomicUsize>,
    pub(crate) next_submission: AtomicU64,
    pub(crate) submissions: Mutex<BTreeMap<u64, upstream_wgpu::SubmissionIndex>>,
}

impl PortableDevice {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn new(
        adapter: &upstream_wgpu::Adapter,
        adapter_info: &AdapterInfo,
        desc: &DeviceDescriptor<'_>,
        error_stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
        uncaptured_error_handler: Arc<Mutex<Option<UncapturedErrorHandler>>>,
        device_loss: Arc<DeviceLossState>,
    ) -> Result<Arc<Self>, RequestDeviceError> {
        let supported = adapter.limits();
        let wgpu_desc = upstream_wgpu::DeviceDescriptor {
            label: desc.label,
            required_features: convert::to_wgpu_features(desc.required_features),
            required_limits: convert::to_wgpu_limits(
                &desc.required_limits,
                desc.required_features,
                adapter_info.backend,
                &supported,
            ),
            experimental_features: upstream_wgpu::ExperimentalFeatures::disabled(),
            memory_hints: upstream_wgpu::MemoryHints::Performance,
            trace: upstream_wgpu::Trace::Off,
        };

        let (device, queue) = block_on_native(adapter.request_device(&wgpu_desc))
            .map_err(|err| RequestDeviceError(err.to_string()))?;

        Self::finish_init(
            adapter_info,
            device,
            queue,
            error_stack,
            uncaptured_error_handler,
            device_loss,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn new_async(
        adapter: upstream_wgpu::Adapter,
        adapter_info: AdapterInfo,
        required_features: Features,
        required_limits: Limits,
        label: Option<String>,
        error_stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
        uncaptured_error_handler: Arc<Mutex<Option<UncapturedErrorHandler>>>,
        device_loss: Arc<DeviceLossState>,
    ) -> Result<Arc<Self>, RequestDeviceError> {
        let supported = adapter.limits();
        let wgpu_desc = upstream_wgpu::DeviceDescriptor {
            label: label.as_deref(),
            required_features: convert::to_wgpu_features(required_features),
            required_limits: convert::to_wgpu_limits(
                &required_limits,
                required_features,
                adapter_info.backend,
                &supported,
            ),
            experimental_features: upstream_wgpu::ExperimentalFeatures::disabled(),
            memory_hints: upstream_wgpu::MemoryHints::Performance,
            trace: upstream_wgpu::Trace::Off,
        };
        let (device, queue) = adapter
            .request_device(&wgpu_desc)
            .await
            .map_err(|err| RequestDeviceError(err.to_string()))?;

        Self::finish_init(
            &adapter_info,
            device,
            queue,
            error_stack,
            uncaptured_error_handler,
            device_loss,
        )
    }

    fn finish_init(
        adapter_info: &AdapterInfo,
        device: upstream_wgpu::Device,
        queue: upstream_wgpu::Queue,
        error_stack: Arc<Mutex<Vec<crate::types::ErrorScopeEntry>>>,
        uncaptured_error_handler: Arc<Mutex<Option<UncapturedErrorHandler>>>,
        device_loss: Arc<DeviceLossState>,
    ) -> Result<Arc<Self>, RequestDeviceError> {
        device.on_uncaptured_error(Arc::new(move |err| {
            crate::dispatch_error(
                &error_stack,
                &uncaptured_error_handler,
                convert::from_wgpu_error(err),
            );
        }));
        let loss_state = device_loss.clone();
        device.set_device_lost_callback(move |reason, message| {
            loss_state.notify(convert::from_wgpu_device_lost_reason(reason), message);
        });

        Ok(Arc::new(Self {
            backend: adapter_info.backend,
            name: Arc::from(adapter_info.name.as_str()),
            device,
            queue,
            injected_loss: Mutex::new(None),
            portable_shader_compilations: Arc::new(AtomicUsize::new(0)),
            next_submission: AtomicU64::new(1),
            submissions: Mutex::new(BTreeMap::new()),
        }))
    }

    pub(crate) fn record_submission(
        &self,
        wgpu_idx: upstream_wgpu::SubmissionIndex,
    ) -> SubmissionIndex {
        let idx = self.next_submission.fetch_add(1, Ordering::Relaxed);
        let mut map = self.submissions.lock().unwrap_or_else(|e| e.into_inner());
        map.insert(idx, wgpu_idx);
        // Keep only the most recent 64 submissions so long sessions stay bounded.
        while map.len() > 64 {
            if let Some(&oldest) = map.keys().next() {
                map.remove(&oldest);
            }
        }
        SubmissionIndex(idx)
    }

    pub(crate) fn poll(&self, poll_type: PollType) -> Result<PollStatus, PollError> {
        let target = match &poll_type {
            PollType::Poll => None,
            PollType::Wait {
                submission_index, ..
            } => submission_index.as_ref().and_then(|s| {
                self.submissions
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&s.0)
                    .cloned()
            }),
        };
        let mapped = convert::to_wgpu_poll_type(poll_type, target);
        let status = self
            .device
            .poll(mapped)
            .map_err(convert::from_wgpu_poll_error)?;
        Ok(convert::from_wgpu_poll_status(status))
    }

    pub(crate) fn is_queue_idle(&self) -> bool {
        matches!(
            self.device.poll(upstream_wgpu::PollType::Poll),
            Ok(upstream_wgpu::PollStatus::QueueEmpty)
        )
    }

    pub(crate) fn check_loss_message(&self) -> Option<String> {
        self.injected_loss
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    #[cfg(feature = "test-hooks")]
    pub(crate) fn inject_loss_for_test(&self, message: String) {
        *self.injected_loss.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
    }

    pub(crate) fn destroy(&self) {
        self.device.destroy();
    }

    pub(crate) fn push_inner_scope(
        &self,
        filter: crate::ErrorFilter,
    ) -> upstream_wgpu::ErrorScopeGuard {
        let wgpu_filter = match filter {
            crate::ErrorFilter::OutOfMemory => upstream_wgpu::ErrorFilter::OutOfMemory,
            crate::ErrorFilter::Validation => upstream_wgpu::ErrorFilter::Validation,
            crate::ErrorFilter::Internal => upstream_wgpu::ErrorFilter::Internal,
        };
        self.device.push_error_scope(wgpu_filter)
    }
}

/// Portable backends built into this configuration (never overrides Apple Metal).
pub(crate) fn enabled_backend_features() -> Backends {
    let mut mask = Backends::empty();
    if crate::vulkan::is_compiled() {
        mask |= Backends::VULKAN;
    }
    if crate::dx12::is_compiled() {
        mask |= Backends::DX12;
    }
    if crate::opengl::is_compiled() {
        mask |= Backends::GL;
    }
    if crate::webgpu::is_compiled() {
        mask |= Backends::BROWSER_WEBGPU;
    }
    convert::from_wgpu_backends(upstream_wgpu::Instance::enabled_backend_features()) & mask
}

pub(crate) fn create_wgpu_instance(
    desc: &InstanceDescriptor,
    effective_backends: Backends,
    enable_noop: bool,
) -> upstream_wgpu::Instance {
    let mut wgpu_backends = convert::to_wgpu_backends(effective_backends);
    // Avoid touching Apple's Metal backend unless test mode requests a portable device or a
    // portable backend was explicitly requested.
    #[cfg(all(feature = "metal", any(target_os = "macos", target_os = "ios")))]
    if !enable_noop && !effective_backends.contains(Backends::METAL) {
        wgpu_backends.remove(upstream_wgpu::Backends::METAL);
    }
    if enable_noop {
        wgpu_backends |= upstream_wgpu::Backends::NOOP;
    }
    let noop_opts = upstream_wgpu::NoopBackendOptions {
        enable: enable_noop,
        ..Default::default()
    };
    upstream_wgpu::Instance::new(upstream_wgpu::InstanceDescriptor {
        backends: wgpu_backends,
        flags: convert::to_wgpu_instance_flags(desc.flags),
        memory_budget_thresholds: upstream_wgpu::MemoryBudgetThresholds::default(),
        backend_options: upstream_wgpu::BackendOptions {
            dx12: upstream_wgpu::Dx12BackendOptions::from_env_or_default(),
            gl: upstream_wgpu::GlBackendOptions::from_env_or_default(),
            noop: noop_opts,
        },
        display: None,
    })
}

pub(crate) fn adapter_features(adapter: &upstream_wgpu::Adapter) -> Features {
    convert::from_wgpu_features(adapter.features())
}

pub(crate) fn adapter_limits(adapter: &upstream_wgpu::Adapter) -> Limits {
    convert::from_wgpu_limits(&adapter.limits())
}

pub(crate) fn adapter_info(adapter: &upstream_wgpu::Adapter) -> AdapterInfo {
    convert::from_wgpu_adapter_info(adapter.get_info())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn block_on_native<F: Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Wake, Waker};
    struct Unparker(std::thread::Thread);
    impl Wake for Unparker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unparker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn enumerate_portable_adapters_sync(
    instance: &upstream_wgpu::Instance,
    backends: upstream_wgpu::Backends,
) -> Vec<upstream_wgpu::Adapter> {
    block_on_native(instance.enumerate_adapters(backends))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn request_portable_adapter_sync(
    instance: &upstream_wgpu::Instance,
    options: &RequestAdapterOptions<'_, '_>,
    compatible_surface: Option<&upstream_wgpu::Surface<'_>>,
) -> Result<upstream_wgpu::Adapter, String> {
    let wgpu_opts = upstream_wgpu::RequestAdapterOptions {
        power_preference: convert::to_wgpu_power_preference(options.power_preference),
        force_fallback_adapter: options.force_fallback_adapter,
        compatible_surface,
        apply_limit_buckets: false,
    };
    block_on_native(instance.request_adapter(&wgpu_opts)).map_err(|e| e.to_string())
}

pub(crate) fn pop_scope_error(guard: upstream_wgpu::ErrorScopeGuard) -> Option<Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        block_on_native(guard.pop()).map(convert::from_wgpu_error)
    }
    #[cfg(target_arch = "wasm32")]
    {
        drop(guard);
        None
    }
}
