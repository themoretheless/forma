//! WGSL translation by target, and portable compute kernel generators.

use crate::back::lower::parse_and_validate;

/// A shading language a WGSL module may be asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComputeShaderTarget {
    /// Apple Metal Shading Language (macOS, iOS)
    MetalMsl,
    /// Khronos SPIR-V (Vulkan)
    VulkanSpirv,
    /// Microsoft DirectX 12 HLSL
    Dx12Hlsl,
    /// NVIDIA CUDA PTX (host-gated in `gpu-compute::cuda`; not synthesized from WGSL).
    CudaPtx,
    /// OpenGL / OpenGL ES GLSL
    OpenGlGlsl,
    /// W3C WebGPU WGSL: the source itself, once it parses and validates.
    BrowserWgsl,
}

/// Binary or textual shader artifact emitted for a supported target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledShader {
    Msl(String),
    Spirv(Vec<u32>),
    Hlsl(String),
    Glsl(String),
    Wgsl(String),
}

impl ComputeShaderTarget {
    /// Whether `compile_wgsl_for_target` and `compile_wgsl` can produce this target.
    pub const fn is_implemented(self) -> bool {
        matches!(
            self,
            Self::MetalMsl
                | Self::VulkanSpirv
                | Self::Dx12Hlsl
                | Self::OpenGlGlsl
                | Self::BrowserWgsl
        )
    }
}

/// Translates a WGSL shader into validated binary SPIR-V words for Vulkan.
pub fn compile_wgsl_to_spirv(wgsl_source: &str) -> Result<Vec<u32>, String> {
    let (module, info) = parse_and_validate(wgsl_source)?;
    crate::back::spv::write_vec(
        &module,
        &info,
        &crate::back::spv::Options::default(),
        &crate::back::spv::PipelineOptions::default(),
    )
}

/// Translates a WGSL shader into the structured artifact form for `target`.
pub fn compile_wgsl(
    wgsl_source: &str,
    target: ComputeShaderTarget,
) -> Result<CompiledShader, String> {
    let (module, info) = parse_and_validate(wgsl_source)?;
    match target {
        ComputeShaderTarget::MetalMsl => {
            let (msl, _) = crate::back::msl::write_string(
                &module,
                &info,
                &crate::back::msl::Options::default(),
                &crate::back::msl::PipelineOptions::default(),
            )
            .map_err(|e| e.to_string())?;
            Ok(CompiledShader::Msl(msl))
        }
        ComputeShaderTarget::VulkanSpirv => crate::back::spv::write_vec(
            &module,
            &info,
            &crate::back::spv::Options::default(),
            &crate::back::spv::PipelineOptions::default(),
        )
        .map(CompiledShader::Spirv),
        ComputeShaderTarget::Dx12Hlsl => crate::back::hlsl::write_string(
            &module,
            &info,
            &crate::back::hlsl::Options::default(),
            &crate::back::hlsl::PipelineOptions::default(),
        )
        .map(CompiledShader::Hlsl),
        ComputeShaderTarget::OpenGlGlsl => crate::back::glsl::write_all_entry_points(
            &module,
            &info,
            &crate::back::glsl::Options::default(),
        )
        .map(CompiledShader::Glsl),
        ComputeShaderTarget::BrowserWgsl => Ok(CompiledShader::Wgsl(module.raw_source)),
        ComputeShaderTarget::CudaPtx => {
            Err(format!("no shader back end for {target:?} is implemented"))
        }
    }
}

/// Translates a WGSL shader into printable target output for `target`.
///
/// # Errors
/// The source does not parse or validate, or `target` has no WGSL compiler back end (`CudaPtx`
/// compiles CUDA kernels on NVIDIA hosts through `gpu-compute::cuda` rather than pretending to
/// emit PTX from arbitrary WGSL).
pub fn compile_wgsl_for_target(
    wgsl_source: &str,
    target: ComputeShaderTarget,
) -> Result<String, String> {
    match compile_wgsl(wgsl_source, target)? {
        CompiledShader::Msl(source)
        | CompiledShader::Hlsl(source)
        | CompiledShader::Glsl(source)
        | CompiledShader::Wgsl(source) => Ok(source),
        CompiledShader::Spirv(words) => crate::back::spv::disassemble_words(&words),
    }
}
