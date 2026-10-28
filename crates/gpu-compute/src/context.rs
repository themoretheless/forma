use crate::{BackendReport, SubgroupReport, block_on};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GpuContextError {
    Adapter(String),
    UnsupportedFeatures {
        required: wgpu::Features,
        available: wgpu::Features,
    },
    Device(String),
}
impl std::fmt::Display for GpuContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adapter(error) => write!(f, "GPU adapter unavailable: {error}"),
            Self::UnsupportedFeatures {
                required,
                available,
            } => write!(
                f,
                "GPU adapter does not support required features {required:?}; available: {available:?}"
            ),
            Self::Device(error) => write!(f, "GPU device creation failed: {error}"),
        }
    }
}
impl std::error::Error for GpuContextError {}

/// Shared device/queue for GPU-accelerated stages; `None` falls back to CPU.
#[derive(Clone)]
pub struct GpuContext {
    inner: std::sync::Arc<GpuContextHandles>,
}

/// Immutable handles shared by clones of a context.
pub struct GpuContextHandles {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The wgpu backend actually bound (Metal on macOS, Vulkan/DX12 elsewhere —
    /// including NVIDIA/"CUDA-class" hardware, which wgpu drives through
    /// Vulkan or DX12; the dedicated CUDA path lives in `crate::cuda`). Kernels
    /// that template their workgroup size read this to pick a per-backend
    /// tuning.
    pub backend: wgpu::Backend,
    /// Features advertised by the adapter, not necessarily enabled on the device.
    pub features: wgpu::Features,
    /// Features enabled on the device.
    pub enabled_features: wgpu::Features,
    /// Device limits cached at creation.
    pub limits: wgpu::Limits,
    pub subgroup_min_size: u32,
    pub subgroup_max_size: u32,
}

impl std::ops::Deref for GpuContext {
    type Target = GpuContextHandles;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl GpuContext {
    pub fn same_device(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.inner, &other.inner)
    }
    pub fn new() -> Option<Self> {
        Self::with_features(wgpu::Features::empty()).ok()
    }

    /// Creates a device with pass-boundary timestamp queries enabled. An
    /// unsupported adapter is an explicit error; host timing is never used as
    /// a substitute. The default `new` does not enable timestamps or subgroups.
    pub fn with_timestamps() -> Result<Self, GpuContextError> {
        Self::with_features(wgpu::Features::TIMESTAMP_QUERY)
    }

    /// Creates a device with explicitly requested optional features. Missing
    /// adapter support is an error; features are never silently dropped or
    /// enabled implicitly, except MAPPABLE_PRIMARY_BUFFERS on Apple Silicon
    /// Metal, where it permits initialization in shared CPU/GPU memory.
    /// For example, native subgroup timing requests `SUBGROUP | TIMESTAMP_QUERY`.
    pub fn with_features(required_features: wgpu::Features) -> Result<Self, GpuContextError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            // Honor wgpu's explicit host/backend selector without a fallback to
            // another backend when that requested adapter is unavailable.
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|error| GpuContextError::Adapter(error.to_string()))?;
        let info = adapter.get_info();
        let backend = info.backend;
        let features = adapter.features();
        validate_features(features, required_features)?;
        let required_features = required_features
            | if apple_shared_memory(backend, features) {
                wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
            } else {
                wgpu::Features::empty()
            };
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu-compute"),
            required_features,
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| GpuContextError::Device(error.to_string()))?;
        let enabled_features = device.features();
        let limits = device.limits();
        Ok(Self {
            inner: std::sync::Arc::new(GpuContextHandles {
                device,
                queue,
                backend,
                features,
                enabled_features,
                limits,
                subgroup_min_size: info.subgroup_min_size,
                subgroup_max_size: info.subgroup_max_size,
            }),
        })
    }

    pub fn backend_label(&self) -> &'static str {
        self.backend_report().label
    }

    pub fn backend_report(&self) -> BackendReport {
        BackendReport::from_wgpu(self.backend)
    }

    /// Returns the memory architecture (`MemoryArchitecture::Uma` vs `MemoryArchitecture::Duma`)
    /// active on this device.
    pub fn memory_architecture(&self) -> crate::MemoryArchitecture {
        crate::device_memory_architecture(&self.device)
    }

    /// Returns the detailed UMA / dUMA memory architecture profile for this device.
    pub fn memory_profile(&self) -> crate::MemoryArchitectureProfile {
        self.memory_architecture().profile()
    }

    pub fn enabled_features(&self) -> wgpu::Features {
        self.enabled_features
    }

    /// Whether subgroups can be used on this device.
    pub fn subgroup_report(&self) -> SubgroupReport {
        SubgroupReport {
            supported: self.enabled_features().contains(wgpu::Features::SUBGROUP),
            min_size: self.subgroup_min_size,
            max_size: self.subgroup_max_size,
        }
    }

    /// Maximum single storage buffer allocation size in bytes permitted by this device.
    #[inline]
    pub fn max_storage_allocation_bytes(&self) -> u64 {
        self.limits
            .max_buffer_size
            .min(self.limits.max_storage_buffer_binding_size)
    }

    /// Validates whether a byte size fits within the device's storage buffer allocation limits.
    #[inline]
    pub fn can_allocate_storage(&self, bytes: u64) -> bool {
        bytes > 0 && bytes.is_multiple_of(4) && bytes <= self.max_storage_allocation_bytes()
    }
}

/// Restrict the shared upload policy to the unified-memory Metal backend.
/// Other native backends may advertise mapping but have discrete GPU memory.
pub(crate) fn apple_shared_memory(backend: wgpu::Backend, features: wgpu::Features) -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
        && backend == wgpu::Backend::Metal
        && features.contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS)
}

fn validate_features(
    available: wgpu::Features,
    required: wgpu::Features,
) -> Result<(), GpuContextError> {
    if !available.contains(required) {
        return Err(GpuContextError::UnsupportedFeatures {
            required,
            available,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_features_require_explicit_adapter_support() {
        assert!(validate_features(wgpu::Features::empty(), wgpu::Features::empty()).is_ok());
        assert!(
            validate_features(
                wgpu::Features::TIMESTAMP_QUERY,
                wgpu::Features::TIMESTAMP_QUERY
            )
            .is_ok()
        );
        assert!(matches!(
            validate_features(wgpu::Features::empty(), wgpu::Features::TIMESTAMP_QUERY),
            Err(GpuContextError::UnsupportedFeatures {
                required: wgpu::Features::TIMESTAMP_QUERY,
                ..
            })
        ));
        let combined = wgpu::Features::SUBGROUP | wgpu::Features::TIMESTAMP_QUERY;
        assert!(validate_features(combined, combined).is_ok());
        assert!(matches!(
            validate_features(wgpu::Features::TIMESTAMP_QUERY, combined),
            Err(GpuContextError::UnsupportedFeatures { required, available })
                if required == combined && available == wgpu::Features::TIMESTAMP_QUERY
        ));
    }
}
