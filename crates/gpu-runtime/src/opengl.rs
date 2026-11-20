//! OpenGL / OpenGL ES backend metadata and compile-time gating (`GLSL` + `upstream-wgpu` GLES HAL).
//!
//! OpenGL / WebGL2 execution is driven by [`crate::portable`] whenever [`crate::Backends::GL`] is
//! requested on desktop Linux/Windows/Android or browser WebGL2.

use gpu_shader::ComputeShaderTarget;

/// Shader representation consumed by the OpenGL / GLES backend.
pub const SHADER_TARGET: ComputeShaderTarget = ComputeShaderTarget::OpenGlGlsl;

/// Whether this build includes OpenGL / GLES runtime support on the current target platform.
#[inline]
pub const fn is_compiled() -> bool {
    cfg!(feature = "opengl")
}
