#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

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
pub mod device;
pub mod duma;
mod error_sink;
pub mod flags;
pub mod metal;
pub mod pipeline;
pub mod platform;
pub mod queue;
pub mod texture;
pub mod transfer;
pub mod types;
pub mod uma;
pub mod util;

pub use device::Device;
pub(crate) use error_sink::*;
pub use queue::Queue;

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

use metal::MetalDevice;
use std::future::Future;
use std::num::NonZeroU64;
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
            for metal in MetalDevice::enumerate_all() {
                if !metal.is_lost() {
                    list.push(Adapter::from_metal(metal));
                }
            }
        }
        std::future::ready(list)
    }

    pub fn request_adapter(
        &self,
        options: &RequestAdapterOptions<'_, '_>,
    ) -> impl Future<Output = Result<Adapter, RequestAdapterError>> {
        let candidates = MetalDevice::enumerate_all()
            .into_iter()
            .map(Adapter::from_metal)
            .collect();
        std::future::ready(select_adapter(candidates, options))
    }

    pub fn create_surface<'a, W>(&self, target: W) -> Result<Surface<'a>, RequestDeviceError>
    where
        W: raw_window_handle::HasWindowHandle,
    {
        // Only AppKit and UIKit views carry a Metal layer; any other window has none.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let (metal_layer, metal_view) = match target.window_handle().map(|handle| handle.as_raw()) {
            Ok(raw_window_handle::RawWindowHandle::AppKit(appkit)) => {
                let view_ptr = appkit.ns_view.as_ptr();
                (
                    unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, true) },
                    unsafe { crate::metal::ObjCObject::from_unretained(view_ptr) }.map(Arc::new),
                )
            }
            Ok(raw_window_handle::RawWindowHandle::UiKit(uikit)) => {
                let view_ptr = uikit.ui_view.as_ptr();
                (
                    unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, false) },
                    unsafe { crate::metal::ObjCObject::from_unretained(view_ptr) }.map(Arc::new),
                )
            }
            _ => (None, None),
        };
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let metal_layer = {
            let _ = target.window_handle();
            None
        };
        Ok(Surface {
            config: Arc::new(Mutex::new(None)),
            metal_layer,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
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
            device: ((metal.registry_id & 0xffff_ffff) as u32).max(1),
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
        let destroyed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dev = Device {
            metal: self.metal.clone(),
            info: Arc::new(self.info.clone()),
            features: desc.required_features & self.features,
            limits: desc.required_limits.clone(),
            error_stack: Arc::new(Mutex::new(Vec::new())),
            uncaptured_error_handler: Arc::new(Mutex::new(None)),
            device_loss: Arc::new(DeviceLossState::default()),
            destroyed: destroyed.clone(),
        };
        self.metal.register_loss_observer(&dev.device_loss);
        let queue = Queue {
            metal: self.metal.clone(),
            identity: Arc::new(()),
            error_sink: CommandErrorSink {
                stack: dev.error_stack.clone(),
                handler: dev.uncaptured_error_handler.clone(),
                scopes: None,
                captured: false,
            },
            device_destroyed: destroyed,
        };
        std::future::ready(Ok((dev, queue)))
    }
}

pub(crate) fn select_adapter(
    candidates: Vec<Adapter>,
    options: &RequestAdapterOptions<'_, '_>,
) -> Result<Adapter, RequestAdapterError> {
    let mut healthy: Vec<Adapter> = candidates
        .into_iter()
        .filter(|adapter| {
            !adapter.metal.is_lost()
                && options
                    .compatible_surface
                    .is_none_or(|surface| adapter.is_surface_supported(surface))
        })
        .collect();
    if healthy.is_empty() {
        return Err(RequestAdapterError(
            "No supported GPU adapter found".to_string(),
        ));
    }
    let preferred = match options.power_preference {
        PowerPreference::HighPerformance => healthy
            .iter()
            .position(|a| !a.metal.is_low_power && !a.metal.is_removable)
            .or_else(|| healthy.iter().position(|a| !a.metal.is_low_power)),
        PowerPreference::LowPower => healthy.iter().position(|a| a.metal.is_low_power),
        PowerPreference::None => healthy.iter().position(|a| !a.metal.is_removable),
    };
    Ok(healthy.swap_remove(preferred.unwrap_or(0)))
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
