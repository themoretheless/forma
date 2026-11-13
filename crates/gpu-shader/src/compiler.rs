//! WGSL translation by target, and portable compute kernel generators.

/// A shading language a WGSL module may be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComputeShaderTarget {
    /// Apple Metal Shading Language (macOS, iOS)
    MetalMsl,
    /// Khronos SPIR-V (Vulkan). Not implemented.
    VulkanSpirv,
    /// Microsoft DirectX 12 HLSL / DXIL. Not implemented.
    Dx12Hlsl,
    /// NVIDIA CUDA PTX. Not implemented.
    CudaPtx,
    /// OpenGL / OpenGL ES GLSL. Not implemented.
    OpenGlGlsl,
    /// W3C WebGPU WGSL: the source itself, once it parses and validates.
    BrowserWgsl,
}

impl ComputeShaderTarget {
    /// Whether `compile_wgsl_for_target` can produce this target.
    pub const fn is_implemented(self) -> bool {
        matches!(self, Self::MetalMsl | Self::BrowserWgsl)
    }
}

/// Translates a WGSL shader for `target`.
///
/// # Errors
/// The source does not parse or validate, or `target` has no back end: only Metal MSL and
/// WGSL itself are produced. The other targets are named so callers can ask, and say so
/// instead of returning text that only looks like the target language.
pub fn compile_wgsl_for_target(
    wgsl_source: &str,
    target: ComputeShaderTarget,
) -> Result<String, String> {
    let module = crate::front::wgsl::parse_str(wgsl_source).map_err(|e| e.to_string())?;
    let mut validator = crate::valid::Validator::new(
        crate::valid::ValidationFlags::all(),
        crate::valid::Capabilities::all(),
    );
    let info = validator.validate(&module).map_err(|e| e.to_string())?;
    match target {
        ComputeShaderTarget::MetalMsl => {
            let (msl, _) = crate::back::msl::write_string(
                &module,
                &info,
                &crate::back::msl::Options::default(),
                &crate::back::msl::PipelineOptions::default(),
            )
            .map_err(|e| e.to_string())?;
            Ok(msl)
        }
        ComputeShaderTarget::BrowserWgsl => Ok(module.raw_source),
        ComputeShaderTarget::VulkanSpirv
        | ComputeShaderTarget::Dx12Hlsl
        | ComputeShaderTarget::CudaPtx
        | ComputeShaderTarget::OpenGlGlsl => {
            Err(format!("no shader back end for {target:?} is implemented"))
        }
    }
}
