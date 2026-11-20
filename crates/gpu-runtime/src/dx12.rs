//! DirectX 12 backend metadata and compile-time gating (`HLSL` + `upstream-wgpu` DX12 HAL).
//!
//! DX12 execution is driven on Windows by [`crate::portable`] whenever [`crate::Backends::DX12`]
//! is enabled and requested (including Windows CI WARP software adapters).

use gpu_shader::ComputeShaderTarget;

/// Shader representation consumed by the DirectX 12 backend.
pub const SHADER_TARGET: ComputeShaderTarget = ComputeShaderTarget::Dx12Hlsl;

/// Whether this build includes DirectX 12 runtime support on the current target platform.
#[inline]
pub const fn is_compiled() -> bool {
    cfg!(all(feature = "dx12", target_os = "windows"))
}
