//! Each shading back end emits validated target artifacts; unimplemented targets error explicitly.
use gpu_shader::back::spv::validate_spirv_words;
use gpu_shader::{
    CompiledShader, ComputeShaderTarget, compile_wgsl, compile_wgsl_for_target,
    compile_wgsl_to_spirv,
};

const SHADER: &str = "@compute @workgroup_size(4) fn run() { let x = 1u; }";

#[test]
fn all_implemented_shader_targets_are_produced() {
    let msl = compile_wgsl_for_target(SHADER, ComputeShaderTarget::MetalMsl).unwrap();
    assert!(msl.contains("kernel void"), "{msl}");

    let spv_words = compile_wgsl_to_spirv(SHADER).unwrap();
    let header = validate_spirv_words(&spv_words).unwrap();
    assert_eq!(header.entry_points, vec!["run".to_string()]);
    let spv_asm = compile_wgsl_for_target(SHADER, ComputeShaderTarget::VulkanSpirv).unwrap();
    assert!(spv_asm.contains("OpEntryPoint"), "{spv_asm}");
    assert!(!spv_asm.contains("@workgroup_size"), "{spv_asm}");

    let hlsl = compile_wgsl_for_target(SHADER, ComputeShaderTarget::Dx12Hlsl).unwrap();
    assert!(hlsl.contains("[numthreads(4, 1, 1)]"), "{hlsl}");

    let glsl = compile_wgsl_for_target(SHADER, ComputeShaderTarget::OpenGlGlsl).unwrap();
    assert!(glsl.contains("#version 310 es"), "{glsl}");
    assert!(glsl.contains("void main()"), "{glsl}");

    assert_eq!(
        compile_wgsl_for_target(SHADER, ComputeShaderTarget::BrowserWgsl).unwrap(),
        SHADER
    );
    assert!(matches!(
        compile_wgsl(SHADER, ComputeShaderTarget::VulkanSpirv).unwrap(),
        CompiledShader::Spirv(_)
    ));
}

#[test]
fn targets_without_a_back_end_are_errors() {
    assert!(!ComputeShaderTarget::CudaPtx.is_implemented());
    let error = compile_wgsl_for_target(SHADER, ComputeShaderTarget::CudaPtx).unwrap_err();
    assert_eq!(error, "no shader back end for CudaPtx is implemented");
}

#[test]
fn a_shader_that_does_not_parse_or_typecheck_is_an_error_for_every_target() {
    for target in [
        ComputeShaderTarget::MetalMsl,
        ComputeShaderTarget::VulkanSpirv,
        ComputeShaderTarget::Dx12Hlsl,
        ComputeShaderTarget::OpenGlGlsl,
        ComputeShaderTarget::BrowserWgsl,
    ] {
        assert!(target.is_implemented(), "{target:?}");
        assert!(
            compile_wgsl_for_target("fn (", target).is_err(),
            "{target:?}"
        );
        assert!(
            compile_wgsl_for_target(
                "@compute @workgroup_size(1) fn run() { let x: u32 = 1.0; }",
                target
            )
            .is_err(),
            "{target:?}"
        );
    }
}
