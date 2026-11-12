//! A target without a back end is refused instead of answered with look-alike text.
use gpu_shader::{ComputeShaderTarget, compile_wgsl_for_target};

const SHADER: &str = "@compute @workgroup_size(4) fn run() { let x = 1u; }";

#[test]
fn metal_and_wgsl_are_produced() {
    let msl = compile_wgsl_for_target(SHADER, ComputeShaderTarget::MetalMsl).unwrap();
    assert!(msl.contains("kernel void"), "{msl}");
    assert_eq!(
        compile_wgsl_for_target(SHADER, ComputeShaderTarget::BrowserWgsl).unwrap(),
        SHADER
    );
}

#[test]
fn targets_without_a_back_end_are_errors() {
    for target in [
        ComputeShaderTarget::VulkanSpirv,
        ComputeShaderTarget::Dx12Hlsl,
        ComputeShaderTarget::CudaPtx,
        ComputeShaderTarget::OpenGlGlsl,
    ] {
        assert!(!target.is_implemented());
        let error = compile_wgsl_for_target(SHADER, target).unwrap_err();
        assert_eq!(
            error,
            format!("no shader back end for {target:?} is implemented")
        );
    }
}

#[test]
fn a_shader_that_does_not_parse_is_an_error_for_every_target() {
    for target in [
        ComputeShaderTarget::MetalMsl,
        ComputeShaderTarget::BrowserWgsl,
    ] {
        assert!(target.is_implemented());
        assert!(compile_wgsl_for_target("fn (", target).is_err());
    }
}
