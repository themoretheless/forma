//! Browser WebGPU backend metadata and compile-time gating (`WGSL` + `upstream-wgpu` WebGPU).
//!
//! Browser WebGPU execution is driven on `wasm32-unknown-unknown` by [`crate::portable`]
//! whenever [`crate::Backends::BROWSER_WEBGPU`] is requested and `navigator.gpu` is available.

use gpu_shader::ComputeShaderTarget;

/// Shader representation consumed directly by Browser WebGPU.
pub const SHADER_TARGET: ComputeShaderTarget = ComputeShaderTarget::BrowserWgsl;

/// Whether this build includes Browser WebGPU runtime support on the current target platform.
#[inline]
pub const fn is_compiled() -> bool {
    cfg!(all(feature = "webgpu", target_arch = "wasm32"))
}
