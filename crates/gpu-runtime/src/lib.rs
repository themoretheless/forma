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
pub mod cuda;
pub mod device;
pub mod duma;
pub mod dx12;
mod error_sink;
pub mod flags;
pub mod metal;
pub mod opengl;
pub mod pipeline;
pub mod platform;
pub(crate) mod portable;
pub mod queue;
pub mod texture;
pub mod transfer;
pub mod types;
pub mod uma;
pub mod util;
pub mod vulkan;
pub mod webgpu;

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

use device::BackendDevice;
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

/// Entry point for enumerating GPUs across Metal (Apple native), Vulkan, DirectX 12,
/// OpenGL/GLES, and WebGPU, while keeping CUDA host-gated outside `wgpu` adapters.
#[derive(Clone)]
pub struct Instance {
    backends: Backends,
    portable: upstream_wgpu::Instance,
    portable_backends: upstream_wgpu::Backends,
    include_metal: bool,
}

impl Default for Instance {
    fn default() -> Self {
        Self::new(InstanceDescriptor::default())
    }
}

impl Instance {
    pub fn new(mut desc: InstanceDescriptor) -> Self {
        if desc.backends == Backends::PRIMARY || desc.backends == Backends::all() {
            if let Some(env_backends) = Backends::from_env() {
                desc.backends = env_backends;
            }
        }
        let include_metal = cfg!(all(
            feature = "metal",
            any(target_os = "macos", target_os = "ios")
        )) && desc.backends.contains(Backends::METAL);
        let portable_effective = if include_metal {
            desc.backends - Backends::METAL
        } else {
            desc.backends
        };
        let portable = portable::create_wgpu_instance(&desc, portable_effective, false);
        let portable_backends = portable::convert::to_wgpu_backends(portable_effective);
        Self {
            backends: desc.backends,
            portable,
            portable_backends,
            include_metal,
        }
    }

    #[doc(hidden)]
    pub fn new_portable_for_testing(enable_noop: bool) -> Self {
        let backends = if enable_noop {
            Backends::empty()
        } else {
            Backends::from_env().unwrap_or(Backends::PRIMARY)
        };
        let desc = InstanceDescriptor {
            backends,
            ..Default::default()
        };
        let portable = portable::create_wgpu_instance(&desc, backends, enable_noop);
        let portable_backends = if enable_noop {
            upstream_wgpu::Backends::NOOP
        } else {
            portable::convert::to_wgpu_backends(backends)
        };
        Self {
            backends,
            portable,
            portable_backends,
            include_metal: false,
        }
    }

    /// Backends compiled into this build (`Metal` on Apple + compiled portable backends).
    pub fn enabled_backend_features() -> Backends {
        let metal = if cfg!(all(
            feature = "metal",
            any(target_os = "macos", target_os = "ios")
        )) {
            Backends::METAL
        } else {
            Backends::empty()
        };
        metal | portable::enabled_backend_features()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn enumerate_adapters(&self, backends: Backends) -> impl Future<Output = Vec<Adapter>> {
        let mut list = Vec::new();
        let requested = backends & self.backends;
        if self.include_metal && requested.contains(Backends::METAL) {
            for metal in MetalDevice::enumerate_all() {
                if !metal.is_lost() {
                    list.push(Adapter::from_metal(metal));
                }
            }
        }
        let mut portable_mask =
            portable::convert::to_wgpu_backends(backends) & self.portable_backends;
        if self.include_metal {
            portable_mask.remove(upstream_wgpu::Backends::METAL);
        }
        if !portable_mask.is_empty() {
            for adapter in portable::enumerate_portable_adapters_sync(&self.portable, portable_mask)
            {
                list.push(Adapter::from_portable(adapter));
            }
        }
        std::future::ready(list)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn enumerate_adapters(&self, backends: Backends) -> impl Future<Output = Vec<Adapter>> {
        let mut prefix = Vec::new();
        let requested = backends & self.backends;
        if self.include_metal && requested.contains(Backends::METAL) {
            for metal in MetalDevice::enumerate_all() {
                if !metal.is_lost() {
                    prefix.push(Adapter::from_metal(metal));
                }
            }
        }
        let portable = self.portable.clone();
        let portable_mask = portable::convert::to_wgpu_backends(requested) & self.portable_backends;
        async move {
            if !portable_mask.is_empty() {
                for adapter in portable.enumerate_adapters(portable_mask).await {
                    prefix.push(Adapter::from_portable(adapter));
                }
            }
            prefix
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_adapter(
        &self,
        options: &RequestAdapterOptions<'_, '_>,
    ) -> impl Future<Output = Result<Adapter, RequestAdapterError>> {
        let surface_allows_metal = options
            .compatible_surface
            .is_none_or(|surface| surface.metal_layer.is_some());
        if self.include_metal && surface_allows_metal && !options.force_fallback_adapter {
            let candidates: Vec<Adapter> = MetalDevice::enumerate_all()
                .into_iter()
                .map(Adapter::from_metal)
                .collect();
            if let Ok(adapter) = select_adapter(candidates, options) {
                return std::future::ready(Ok(adapter));
            }
        }

        if !self.portable_backends.is_empty() {
            let compatible_surface = options
                .compatible_surface
                .and_then(|surface| surface.portable.as_deref());
            if options.compatible_surface.is_some() && compatible_surface.is_none() {
                return std::future::ready(Err(RequestAdapterError(
                    "Compatible surface belongs to another backend".to_string(),
                )));
            }
            return std::future::ready(
                portable::request_portable_adapter_sync(
                    &self.portable,
                    options,
                    compatible_surface,
                )
                .map(Adapter::from_portable)
                .map_err(RequestAdapterError),
            );
        }

        std::future::ready(Err(RequestAdapterError(
            "No supported GPU adapter found".to_string(),
        )))
    }

    #[cfg(target_arch = "wasm32")]
    pub fn request_adapter(
        &self,
        options: &RequestAdapterOptions<'_, '_>,
    ) -> impl Future<Output = Result<Adapter, RequestAdapterError>> {
        let metal_candidate = if self.include_metal
            && options
                .compatible_surface
                .is_none_or(|surface| surface.metal_layer.is_some())
            && !options.force_fallback_adapter
        {
            let candidates: Vec<Adapter> = MetalDevice::enumerate_all()
                .into_iter()
                .map(Adapter::from_metal)
                .collect();
            select_adapter(candidates, options).ok()
        } else {
            None
        };
        let wgpu_opts = upstream_wgpu::RequestAdapterOptions {
            power_preference: portable::convert::to_wgpu_power_preference(options.power_preference),
            force_fallback_adapter: options.force_fallback_adapter,
            compatible_surface: options
                .compatible_surface
                .and_then(|surface| surface.portable.as_deref()),
            apply_limit_buckets: false,
        };
        let fut = self.portable.request_adapter(&wgpu_opts);
        async move {
            if let Some(adapter) = metal_candidate {
                return Ok(adapter);
            }
            fut.await
                .map(Adapter::from_portable)
                .map_err(|err| RequestAdapterError(err.to_string()))
        }
    }

    pub fn create_surface<'a, W>(&self, target: W) -> Result<Surface<'a>, RequestDeviceError>
    where
        W: raw_window_handle::HasWindowHandle
            + raw_window_handle::HasDisplayHandle
            + Send
            + Sync
            + 'a,
    {
        // Only AppKit and UIKit views carry a Metal layer; any other window has none.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let (metal_layer, metal_view) = if self.include_metal {
            match target.window_handle().map(|handle| handle.as_raw()) {
                Ok(raw_window_handle::RawWindowHandle::AppKit(appkit)) => {
                    let view_ptr = appkit.ns_view.as_ptr();
                    (
                        unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, true) },
                        unsafe { crate::metal::ObjCObject::from_unretained(view_ptr) }
                            .map(Arc::new),
                    )
                }
                Ok(raw_window_handle::RawWindowHandle::UiKit(uikit)) => {
                    let view_ptr = uikit.ui_view.as_ptr();
                    (
                        unsafe { MetalDevice::ensure_metal_layer_for_view(view_ptr, false) },
                        unsafe { crate::metal::ObjCObject::from_unretained(view_ptr) }
                            .map(Arc::new),
                    )
                }
                _ => (None, None),
            }
        } else {
            (None, None)
        };
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        let metal_layer = {
            let _ = target.window_handle();
            None
        };
        let portable = if !self.include_metal {
            Some(Arc::new(portable::surface::create_surface(
                &self.portable,
                target,
            )?))
        } else {
            None
        };
        Ok(Surface {
            config: Arc::new(Mutex::new(None)),
            metal_layer,
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            metal_view,
            portable,
            _marker: std::marker::PhantomData,
        })
    }
}

#[derive(Clone)]
pub(crate) enum AdapterInner {
    Metal(MetalDevice),
    Portable(upstream_wgpu::Adapter),
}

#[derive(Clone)]
pub struct Adapter {
    pub(crate) inner: AdapterInner,
    pub(crate) info: AdapterInfo,
    pub(crate) features: Features,
    pub(crate) limits: Limits,
}

impl Adapter {
    #[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
    pub(crate) fn expect_metal(&self) -> &MetalDevice {
        match &self.inner {
            AdapterInner::Metal(metal) => metal,
            AdapterInner::Portable(_) => panic!("expected Metal adapter"),
        }
    }

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
            max_texture_dimension_1d: 16384,
            max_texture_dimension_2d: 16384,
            max_texture_array_layers: 2048,
            max_storage_buffers_per_shader_stage: 16,
            max_storage_textures_per_shader_stage: 8,
            max_inter_stage_shader_components: 124,
            max_color_attachment_bytes_per_sample: 64,
            max_compute_workgroup_storage_size: 32768,
            max_compute_invocations_per_workgroup: 1024,
            max_compute_workgroup_size_x: 1024,
            max_compute_workgroup_size_y: 1024,
            max_immediate_size: 4096,
            max_buffer_size: metal.max_buffer_length,
            max_storage_buffer_binding_size: metal.max_buffer_length,
            ..Limits::default()
        };
        Self {
            inner: AdapterInner::Metal(metal),
            info,
            features,
            limits,
        }
    }

    pub(crate) fn from_portable(adapter: upstream_wgpu::Adapter) -> Self {
        let info = portable::adapter_info(&adapter);
        let features = portable::adapter_features(&adapter);
        let limits = portable::adapter_limits(&adapter);
        Self {
            inner: AdapterInner::Portable(adapter),
            info,
            features,
            limits,
        }
    }

    pub(crate) fn is_lost(&self) -> bool {
        match &self.inner {
            AdapterInner::Metal(metal) => metal.is_lost(),
            AdapterInner::Portable(_) => false,
        }
    }

    pub(crate) fn is_low_power(&self) -> bool {
        match &self.inner {
            AdapterInner::Metal(metal) => metal.is_low_power,
            AdapterInner::Portable(_) => self.info.device_type == DeviceType::IntegratedGpu,
        }
    }

    pub(crate) fn is_removable(&self) -> bool {
        match &self.inner {
            AdapterInner::Metal(metal) => metal.is_removable,
            AdapterInner::Portable(_) => false,
        }
    }

    pub fn get_info(&self) -> AdapterInfo {
        self.info.clone()
    }

    /// A surface is presentable when backed by a Metal layer or portable surface supported by the adapter.
    pub fn is_surface_supported(&self, surface: &Surface<'_>) -> bool {
        match &self.inner {
            AdapterInner::Metal(_) => surface.metal_layer.is_some(),
            AdapterInner::Portable(raw) => surface
                .portable
                .as_ref()
                .is_some_and(|s| raw.is_surface_supported(s)),
        }
    }

    pub fn features(&self) -> Features {
        self.features
    }

    pub fn limits(&self) -> Limits {
        self.limits.clone()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn request_device_sync(
        &self,
        desc: &DeviceDescriptor<'_>,
    ) -> Result<(Device, Queue), RequestDeviceError> {
        let destroyed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let error_stack = Arc::new(Mutex::new(Vec::new()));
        let uncaptured_error_handler = Arc::new(Mutex::new(None));
        let device_loss = Arc::new(DeviceLossState::default());
        let granted_features = desc.required_features & self.features;

        let backend = match &self.inner {
            AdapterInner::Metal(metal) => {
                metal.register_loss_observer(&device_loss);
                BackendDevice::Metal(metal.clone())
            }
            AdapterInner::Portable(raw) => BackendDevice::Portable(portable::PortableDevice::new(
                raw,
                &self.info,
                desc,
                error_stack.clone(),
                uncaptured_error_handler.clone(),
                device_loss.clone(),
            )?),
        };

        let dev = Device {
            backend: backend.clone(),
            info: Arc::new(self.info.clone()),
            features: granted_features,
            limits: desc.required_limits.clone(),
            error_stack: error_stack.clone(),
            uncaptured_error_handler: uncaptured_error_handler.clone(),
            device_loss,
            destroyed: destroyed.clone(),
        };
        let queue = Queue {
            backend,
            identity: Arc::new(()),
            error_sink: CommandErrorSink {
                stack: error_stack,
                handler: uncaptured_error_handler,
                scopes: None,
                captured: false,
            },
            device_destroyed: destroyed,
        };
        Ok((dev, queue))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_device(
        &self,
        desc: &DeviceDescriptor<'_>,
    ) -> impl Future<Output = Result<(Device, Queue), RequestDeviceError>> {
        std::future::ready(self.request_device_sync(desc))
    }

    #[cfg(target_arch = "wasm32")]
    pub fn request_device(
        &self,
        desc: &DeviceDescriptor<'_>,
    ) -> impl Future<Output = Result<(Device, Queue), RequestDeviceError>> {
        let inner = self.inner.clone();
        let info = self.info.clone();
        let granted_features = desc.required_features & self.features;
        let required_limits = desc.required_limits.clone();
        let label = desc.label.map(str::to_owned);
        async move {
            let error_stack = Arc::new(Mutex::new(Vec::new()));
            let uncaptured_error_handler = Arc::new(Mutex::new(None));
            let device_loss = Arc::new(DeviceLossState::default());
            let destroyed = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let backend = match inner {
                AdapterInner::Metal(metal) => {
                    metal.register_loss_observer(&device_loss);
                    BackendDevice::Metal(metal)
                }
                AdapterInner::Portable(adapter) => BackendDevice::Portable(
                    portable::PortableDevice::new_async(
                        adapter,
                        info.clone(),
                        granted_features,
                        required_limits.clone(),
                        label,
                        error_stack.clone(),
                        uncaptured_error_handler.clone(),
                        device_loss.clone(),
                    )
                    .await?,
                ),
            };
            let dev = Device {
                backend: backend.clone(),
                info: Arc::new(info),
                features: granted_features,
                limits: required_limits,
                error_stack: error_stack.clone(),
                uncaptured_error_handler: uncaptured_error_handler.clone(),
                device_loss,
                destroyed: destroyed.clone(),
            };
            let queue = Queue {
                backend,
                identity: Arc::new(()),
                error_sink: CommandErrorSink {
                    stack: error_stack,
                    handler: uncaptured_error_handler,
                    scopes: None,
                    captured: false,
                },
                device_destroyed: destroyed,
            };
            Ok((dev, queue))
        }
    }
}

pub(crate) fn select_adapter(
    candidates: Vec<Adapter>,
    options: &RequestAdapterOptions<'_, '_>,
) -> Result<Adapter, RequestAdapterError> {
    let mut healthy: Vec<Adapter> = candidates
        .into_iter()
        .filter(|adapter| {
            !adapter.is_lost()
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
            .position(|a| !a.is_low_power() && !a.is_removable())
            .or_else(|| healthy.iter().position(|a| !a.is_low_power())),
        PowerPreference::LowPower => healthy.iter().position(|a| a.is_low_power()),
        PowerPreference::None => healthy.iter().position(|a| !a.is_removable()),
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
