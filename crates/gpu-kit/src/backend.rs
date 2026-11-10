//! Explicit backend selection; device availability is checked by request_adapter.
use crate::wgpu;
use std::{fmt, str::FromStr};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GpuBackend {
    #[default]
    Auto,
    Metal,
    Vulkan,
    Dx12,
    Gl,
}
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("unknown GPU backend {0}; expected auto, metal, vulkan, dx12 or gl")]
    Unknown(String),
    #[error("unknown GPU vendor {0}; expected any or nvidia")]
    UnknownVendor(String),
    #[error("no compatible GPU adapter for vendor {0}")]
    VendorMissing(GpuVendor),
    #[error(transparent)]
    Adapter(wgpu::RequestAdapterError),
    #[error("GPU backend {0} is not enabled in this build/platform")]
    Unavailable(GpuBackend),
}
impl fmt::Display for GpuBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Metal => "metal",
            Self::Vulkan => "vulkan",
            Self::Dx12 => "dx12",
            Self::Gl => "gl",
        })
    }
}
impl FromStr for GpuBackend {
    type Err = BackendError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "auto" => Self::Auto,
            "metal" => Self::Metal,
            "vulkan" => Self::Vulkan,
            "dx12" => Self::Dx12,
            "gl" => Self::Gl,
            _ => return Err(BackendError::Unknown(s.into())),
        })
    }
}
impl GpuBackend {
    fn resolve(self, available: wgpu::Backends) -> Result<wgpu::Backends, BackendError> {
        let requested = match self {
            Self::Auto => available,
            Self::Metal => wgpu::Backends::METAL,
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Dx12 => wgpu::Backends::DX12,
            Self::Gl => wgpu::Backends::GL,
        };
        let enabled = requested & available;
        if enabled.is_empty() {
            Err(BackendError::Unavailable(self))
        } else {
            Ok(enabled)
        }
    }
    /// Restricts adapter discovery. An explicit request never falls back to another API.
    pub fn configure(self, descriptor: &mut wgpu::InstanceDescriptor) -> Result<(), BackendError> {
        descriptor.backends = self.resolve(wgpu::Instance::enabled_backend_features())?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_backend_masks_and_unavailable_requests_never_fallback() {
        let available = wgpu::Backends::METAL | wgpu::Backends::VULKAN;
        assert_eq!(GpuBackend::Auto.resolve(available).unwrap(), available);
        assert_eq!(
            GpuBackend::Metal.resolve(available).unwrap(),
            wgpu::Backends::METAL
        );
        assert_eq!(
            GpuBackend::Vulkan.resolve(available).unwrap(),
            wgpu::Backends::VULKAN
        );
        assert!(GpuBackend::Dx12.resolve(available).is_err());
        assert!(GpuBackend::Auto.resolve(wgpu::Backends::empty()).is_err());
        for backend in [
            GpuBackend::Auto,
            GpuBackend::Metal,
            GpuBackend::Vulkan,
            GpuBackend::Dx12,
            GpuBackend::Gl,
        ] {
            assert_eq!(backend.to_string().parse::<GpuBackend>().unwrap(), backend);
        }
        assert!("cuda".parse::<GpuBackend>().is_err());
    }
}

/// Hardware filter uses the reported numeric vendor ID, not adapter names.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GpuVendor {
    #[default]
    Any,
    Nvidia,
}
impl fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Any => "any",
            Self::Nvidia => "nvidia",
        })
    }
}
impl FromStr for GpuVendor {
    type Err = BackendError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "any" => Ok(Self::Any),
            "nvidia" => Ok(Self::Nvidia),
            _ => Err(BackendError::UnknownVendor(s.into())),
        }
    }
}
struct Candidate {
    vendor: u32,
    device_type: wgpu::DeviceType,
    compatible: bool,
}
impl GpuVendor {
    fn choose(self, candidates: &[Candidate], preference: wgpu::PowerPreference) -> Option<usize> {
        candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.compatible
                    && match self {
                        Self::Any => true,
                        Self::Nvidia => {
                            c.vendor == 0x10de && c.device_type != wgpu::DeviceType::Cpu
                        }
                    }
            })
            .min_by_key(|(_, c)| match (preference, c.device_type) {
                (wgpu::PowerPreference::LowPower, wgpu::DeviceType::IntegratedGpu) => 0,
                (_, wgpu::DeviceType::DiscreteGpu) => 1,
                (_, wgpu::DeviceType::IntegratedGpu) => 2,
                (_, wgpu::DeviceType::VirtualGpu) => 3,
                (_, wgpu::DeviceType::Other) => 4,
                (_, wgpu::DeviceType::Cpu) => 5,
            })
            .map(|(i, _)| i)
    }
    /// Enumerates matching hardware for an explicit vendor; surface compatibility
    /// and power preference are preserved. Absence never falls back to another vendor.
    pub async fn request_adapter(
        &self,
        instance: &wgpu::Instance,
        options: &wgpu::RequestAdapterOptions<'_, '_>,
    ) -> Result<wgpu::Adapter, BackendError> {
        if *self == Self::Any {
            return instance
                .request_adapter(options)
                .await
                .map_err(BackendError::Adapter);
        }
        if options.force_fallback_adapter {
            return Err(BackendError::VendorMissing(*self));
        }
        let mut adapters = instance
            .enumerate_adapters(wgpu::Instance::enabled_backend_features())
            .await;
        let candidates: Vec<_> = adapters
            .iter()
            .map(|a| {
                let info = a.get_info();
                Candidate {
                    vendor: info.vendor,
                    device_type: info.device_type,
                    compatible: options
                        .compatible_surface
                        .is_none_or(|s| a.is_surface_supported(s)),
                }
            })
            .collect();
        let index = self
            .choose(&candidates, options.power_preference)
            .ok_or(BackendError::VendorMissing(*self))?;
        Ok(adapters.remove(index))
    }
}
#[cfg(test)]
mod vendor_tests {
    use super::*;
    #[test]
    fn hardware_vendor_filter_rejects_other_ids_and_incompatible_surfaces() {
        let mut candidates = vec![
            Candidate {
                vendor: 0x8086,
                device_type: wgpu::DeviceType::DiscreteGpu,
                compatible: true,
            },
            Candidate {
                vendor: 0x10de,
                device_type: wgpu::DeviceType::IntegratedGpu,
                compatible: true,
            },
            Candidate {
                vendor: 0x10de,
                device_type: wgpu::DeviceType::DiscreteGpu,
                compatible: true,
            },
        ];
        assert_eq!(
            GpuVendor::Nvidia.choose(&candidates, wgpu::PowerPreference::HighPerformance),
            Some(2)
        );
        assert_eq!(
            GpuVendor::Nvidia.choose(&candidates, wgpu::PowerPreference::LowPower),
            Some(1)
        );
        candidates[2].compatible = false;
        assert_eq!(
            GpuVendor::Nvidia.choose(&candidates, wgpu::PowerPreference::HighPerformance),
            Some(1)
        );
        candidates[1].compatible = false;
        assert_eq!(
            GpuVendor::Nvidia.choose(&candidates, wgpu::PowerPreference::HighPerformance),
            None
        );
        assert_eq!(
            GpuVendor::Any.choose(&candidates, wgpu::PowerPreference::HighPerformance),
            Some(0)
        );
        assert_eq!("nvidia".parse::<GpuVendor>().unwrap(), GpuVendor::Nvidia);
        assert!("NVIDIA GPU".parse::<GpuVendor>().is_err());
        candidates.push(Candidate {
            vendor: 0x10de,
            device_type: wgpu::DeviceType::Cpu,
            compatible: true,
        });
        assert_eq!(
            GpuVendor::Nvidia.choose(&candidates, wgpu::PowerPreference::HighPerformance),
            None
        );
    }
}
