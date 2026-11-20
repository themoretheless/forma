//! Vulkan backend metadata and compile-time gating (`SPIR-V` + `upstream-wgpu` Vulkan HAL).
//!
//! Vulkan execution is driven by [`crate::portable`] whenever [`crate::Backends::VULKAN`] is
//! requested (including Linux CI on Mesa `lavapipe`, Windows, and Android). Adapters are never
//! fabricated when no Vulkan ICD is present.

use gpu_shader::ComputeShaderTarget;

/// Shader representation consumed by the Vulkan backend.
pub const SHADER_TARGET: ComputeShaderTarget = ComputeShaderTarget::VulkanSpirv;

/// Whether this build includes Vulkan runtime support on the current target platform.
#[inline]
pub const fn is_compiled() -> bool {
    cfg!(all(
        feature = "vulkan",
        any(
            target_os = "linux",
            target_os = "windows",
            target_os = "android",
            target_os = "freebsd"
        )
    ))
}
