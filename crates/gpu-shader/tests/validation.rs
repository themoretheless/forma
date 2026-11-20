//! Real semantic validation tests for `gpu_shader::valid::Validator`.

use gpu_shader::front::wgsl::parse_str;
use gpu_shader::valid::{Capabilities, ValidationFlags, Validator};

fn validate(source: &str) -> Result<(), String> {
    let module = parse_str(source).map_err(|e| format!("parse error: {e}"))?;
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn assert_invalid(source: &str, expected_substr: &str) {
    let Err(err) = validate(source) else {
        panic!("expected validation failure for:\n{source}");
    };
    assert!(
        err.contains(expected_substr),
        "expected `{expected_substr}` in validation error `{err}` for:\n{source}"
    );
}

#[test]
fn rejects_ill_typed_binary_operations() {
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let x = 1.0f + 2u; }",
        "invalid",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let x = vec2<f32>(1.0) + vec3<f32>(1.0); }",
        "invalid",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let x = true && 1u; }",
        "bool",
    );
}

#[test]
fn rejects_non_bool_branch_and_loop_conditions() {
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { if 1u { return; } }",
        "bool",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { while 1i { break; } }",
        "bool",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { for (var i = 0u; 1u; i++) { break; } }",
        "bool",
    );
}

#[test]
fn rejects_invalid_returns_and_missing_return_paths() {
    assert_invalid(
        "fn helper() -> u32 { return 1.0f; } @compute @workgroup_size(1) fn main() { let v = helper(); }",
        "return type mismatch",
    );
    assert_invalid(
        "fn helper() -> u32 { let a = 1u; } @compute @workgroup_size(1) fn main() { let v = helper(); }",
        "does not return a value",
    );
    assert_invalid(
        "fn helper() -> u32 { return; } @compute @workgroup_size(1) fn main() { let v = helper(); }",
        "missing return value",
    );
    assert_invalid(
        "fn helper() { return 1u; } @compute @workgroup_size(1) fn main() { helper(); }",
        "void function",
    );
    validate(
        "fn helper(flag: bool) -> u32 { if flag { return 1u; } else { return 2u; } } @compute @workgroup_size(1) fn main() { let v = helper(true); }",
    )
    .unwrap();
}

#[test]
fn rejects_writes_to_immutable_bindings_and_read_only_buffers() {
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let x = 1u; x = 2u; }",
        "immutable",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { const x = 1u; x = 2u; }",
        "immutable",
    );
    assert_invalid(
        "fn helper(p: u32) { p = 2u; } @compute @workgroup_size(1) fn main() { helper(1u); }",
        "immutable",
    );
    assert_invalid(
        "@group(0) @binding(0) var<uniform> u: u32; @compute @workgroup_size(1) fn main() { u = 2u; }",
        "immutable",
    );
    assert_invalid(
        "@group(0) @binding(0) var<storage, read> buf: array<u32>; @compute @workgroup_size(1) fn main() { buf[0] = 2u; }",
        "immutable",
    );
}

#[test]
fn rejects_invalid_swizzles_members_and_calls() {
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let v = vec2<f32>(1.0, 2.0); let z = v.z; }",
        "out of bounds",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let v = vec3<f32>(1.0); let bad = v.xr; }",
        "swizzle",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let v = missing_var; }",
        "unknown identifier",
    );
    assert_invalid(
        "struct S { x: f32 } @compute @workgroup_size(1) fn main() { let s = S(1.0); let y = s.nope; }",
        "no member named `nope`",
    );
    assert_invalid(
        "fn f(a: u32, b: f32) -> u32 { return a; } @compute @workgroup_size(1) fn main() { let r = f(1u); }",
        "expected 2 argument(s), got 1",
    );
    assert_invalid(
        "fn f(a: u32) -> u32 { return a; } @compute @workgroup_size(1) fn main() { let r = f(1.5f); }",
        "argument 0",
    );
}

#[test]
fn rejects_invalid_texture_store_bitcast_and_stage_contracts() {
    assert_invalid(
        "@group(0) @binding(0) var t: texture_storage_2d<rgba8unorm, read>;\n@compute @workgroup_size(1) fn main() { textureStore(t, vec2<u32>(0u), vec4<f32>(1.0)); }",
        "read-only storage texture",
    );
    assert_invalid(
        "@group(0) @binding(0) var t: texture_2d<f32>;\n@compute @workgroup_size(1) fn main() { textureStore(t, vec2<u32>(0u), vec4<f32>(1.0)); }",
        "storage texture",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() { let x = bitcast<vec4<u32>>(1.0f); }",
        "source has 32 bits, target has 128 bits",
    );
    assert_invalid(
        "@vertex fn vs_main() -> vec4<f32> { return vec4<f32>(0.0); }",
        "@builtin(position)",
    );
    assert_invalid(
        "@compute @workgroup_size(1) fn main() -> u32 { return 0u; }",
        "compute entry point",
    );
}
