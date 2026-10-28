//! GPU (wgpu), CUDA and tensor backends for the `osv-math` point kernels.
//!
//! `osv-math` is CPU-only and exposes the [`math_core::DeviceKernels`] port;
//! this crate implements it. Call [`install`] once at startup so every
//! `math_core::*_accelerated` entry point can reach the device kernels.

// The moved device modules address the math types as `crate::…`.
#[allow(unused_imports)]
pub(crate) use math_core::*;

mod shaders;
pub use shaders::*;

#[cfg(feature = "cuda")]
pub mod cuda;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "gpu")]
mod kernels;
#[cfg(feature = "tensor")]
pub mod tensor;

/// Installs this crate's device kernels into `osv-math`. Idempotent; returns
/// `true` when a device backend is available after the call. Without the
/// `gpu` feature there is nothing to install and the CPU reference is used.
pub fn install() -> bool {
    #[cfg(feature = "gpu")]
    {
        math_core::install_device_kernels(&kernels::Device);
        math_core::device_kernels().is_some()
    }
    #[cfg(not(feature = "gpu"))]
    false
}
