//! Cross-backend lowering tests across MSL, SPIR-V, HLSL, GLSL, and WGSL.

use gpu_shader::back::glsl;
use gpu_shader::back::spv::validate_spirv_words;
use gpu_shader::{
    CompiledShader, ComputeShaderTarget, ShaderStage, compile_wgsl, compile_wgsl_for_target,
};

#[test]
fn compute_kernels_with_atomics_storage_textures_and_overrides_lower_cleanly() {
    let atomic_and_length = r#"
@group(0) @binding(0) var<storage, read_write> data: array<atomic<u32>>;
@group(0) @binding(1) var<storage, read> src: array<u32>;
override TILE: u32 = 8u;

@compute @workgroup_size(TILE)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = arrayLength(&src);
    if (gid.x < n) {
        atomicAdd(&data[0], src[gid.x]);
    }
}
"#;
    for target in [
        ComputeShaderTarget::MetalMsl,
        ComputeShaderTarget::VulkanSpirv,
        ComputeShaderTarget::Dx12Hlsl,
        ComputeShaderTarget::OpenGlGlsl,
        ComputeShaderTarget::BrowserWgsl,
    ] {
        let compiled = compile_wgsl(atomic_and_length, target)
            .unwrap_or_else(|err| panic!("{target:?} failed: {err}"));
        match (target, compiled) {
            (ComputeShaderTarget::MetalMsl, CompiledShader::Msl(s)) => {
                assert!(s.contains("kernel void"), "{s}");
            }
            (ComputeShaderTarget::VulkanSpirv, CompiledShader::Spirv(words)) => {
                let info = validate_spirv_words(&words).unwrap();
                assert_eq!(info.entry_points, vec!["main".to_string()]);
                let asm = gpu_shader::back::spv::disassemble_words(&words).unwrap();
                assert!(asm.contains("OpAtomicIAdd"), "{asm}");
            }
            (ComputeShaderTarget::Dx12Hlsl, CompiledShader::Hlsl(s)) => {
                assert!(s.contains("InterlockedAdd"), "{s}");
                assert!(s.contains("[numthreads(8, 1, 1)]"), "{s}");
            }
            (ComputeShaderTarget::OpenGlGlsl, CompiledShader::Glsl(s)) => {
                assert!(s.contains("atomicAdd"), "{s}");
                assert!(s.contains("local_size_x = 8"), "{s}");
            }
            (ComputeShaderTarget::BrowserWgsl, CompiledShader::Wgsl(s)) => {
                assert!(s.contains("atomicAdd"), "{s}");
            }
            other => panic!("unexpected variant for {target:?}: {other:?}"),
        }
    }

    let storage_tex = r#"
@group(0) @binding(0) var img: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(4, 4, 1)
fn write_texel(@builtin(global_invocation_id) gid: vec3<u32>) {
    textureStore(img, vec2<i32>(i32(gid.x), i32(gid.y)), vec4<f32>(0.25, 0.5, 0.75, 1.0));
}
"#;
    for target in [
        ComputeShaderTarget::MetalMsl,
        ComputeShaderTarget::VulkanSpirv,
        ComputeShaderTarget::Dx12Hlsl,
        ComputeShaderTarget::OpenGlGlsl,
        ComputeShaderTarget::BrowserWgsl,
    ] {
        assert!(
            compile_wgsl_for_target(storage_tex, target).is_ok(),
            "storage texture failed for {target:?}"
        );
    }
}

#[test]
fn forma_production_vector_shader_compiles_across_all_backends() {
    let vector_wgsl = include_str!("../../forma/src/vector.wgsl");

    let msl = compile_wgsl(vector_wgsl, ComputeShaderTarget::MetalMsl).unwrap();
    assert!(
        matches!(msl, CompiledShader::Msl(ref s) if s.contains("vertex ") && s.contains("fragment "))
    );

    let spv = match compile_wgsl(vector_wgsl, ComputeShaderTarget::VulkanSpirv).unwrap() {
        CompiledShader::Spirv(words) => words,
        _ => unreachable!(),
    };
    let header = validate_spirv_words(&spv).unwrap();
    assert!(header.entry_points.contains(&"vs".to_string()));
    assert!(header.entry_points.contains(&"fs".to_string()));

    let hlsl = match compile_wgsl(vector_wgsl, ComputeShaderTarget::Dx12Hlsl).unwrap() {
        CompiledShader::Hlsl(s) => s,
        _ => unreachable!(),
    };
    assert!(hlsl.contains("vs(") && hlsl.contains("fs("), "{hlsl}");

    let (module, info) = {
        let m = gpu_shader::front::wgsl::parse_str(vector_wgsl).unwrap();
        let mut v = gpu_shader::valid::Validator::new(
            gpu_shader::valid::ValidationFlags::all(),
            gpu_shader::valid::Capabilities::all(),
        );
        let i = v.validate(&m).unwrap();
        (m, i)
    };
    for (stage, name) in [(ShaderStage::Vertex, "vs"), (ShaderStage::Fragment, "fs")] {
        let out = glsl::write_string(
            &module,
            &info,
            &glsl::Options::default(),
            &glsl::PipelineOptions {
                shader_stage: stage,
                entry_point: name.to_string(),
            },
        )
        .unwrap();
        assert!(out.contains("#version 310 es"), "{out}");
        assert!(out.contains("void main()"), "{out}");
    }
    let glsl_all = compile_wgsl_for_target(vector_wgsl, ComputeShaderTarget::OpenGlGlsl).unwrap();
    assert!(
        glsl_all.contains("// entry point: vs (vertex)"),
        "{glsl_all}"
    );
    assert!(
        glsl_all.contains("// entry point: fs (fragment)"),
        "{glsl_all}"
    );
}

#[test]
fn spirv_validator_rejects_corrupted_binary_headers_and_opcodes() {
    assert!(validate_spirv_words(&[0, 1, 2]).is_err());
    assert!(validate_spirv_words(&[0xDEAD_BEEF, 0x0001_0300, 0, 4, 0]).is_err());
    // Magic ok, zero id bound -> rejected.
    assert!(validate_spirv_words(&[0x0723_0203, 0x0001_0300, 0, 0, 0]).is_err());
    // Unknown opcode 0xffff with length 1 -> rejected.
    assert!(
        validate_spirv_words(&[0x0723_0203, 0x0001_0300, 0, 4, 0, (1 << 16) | 0xffff]).is_err()
    );
}

#[test]
fn ill_typed_wgsl_is_rejected_uniformly_across_all_six_targets() {
    let invalid_shaders = [
        "@compute @workgroup_size(1) fn main() { let x: u32 = 1.5; }",
        "@group(0) @binding(0) var<storage, read> buf: array<u32>;
         @compute @workgroup_size(1) fn main() { buf[0] = 7u; }",
        "@vertex fn vs(@builtin(global_invocation_id) id: vec3<u32>) -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }",
    ];
    for src in invalid_shaders {
        for target in [
            ComputeShaderTarget::MetalMsl,
            ComputeShaderTarget::VulkanSpirv,
            ComputeShaderTarget::Dx12Hlsl,
            ComputeShaderTarget::OpenGlGlsl,
            ComputeShaderTarget::BrowserWgsl,
            ComputeShaderTarget::CudaPtx,
        ] {
            assert!(
                compile_wgsl_for_target(src, target).is_err(),
                "{target:?} accepted invalid WGSL: {src}"
            );
        }
    }
}
