//! Storage textures: the front end accepts their types, and the MSL back end reads and writes
//! them with the access and component type the WGSL declares.

use gpu_shader::back::msl;
use gpu_shader::front::wgsl;
use gpu_shader::valid::{Capabilities, ValidationFlags, Validator};

fn translate(source: &str) -> Result<String, String> {
    let module = wgsl::parse_str(source).map_err(|error| error.to_string())?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|error| format!("{error:?}"))?;
    msl::write_string(
        &module,
        &info,
        &msl::Options::default(),
        &Default::default(),
    )
    .map(|(source, _)| source)
}

const COPY: &str = "
@group(0) @binding(0) var source: texture_storage_2d<r32uint, read>;
@group(0) @binding(1) var target: texture_storage_2d<rgba8unorm, write>;
@compute @workgroup_size(8, 8)
fn run(@builtin(global_invocation_id) id: vec3<u32>) {
    let texel = textureLoad(source, id.xy);
    let level = f32(texel.x) / 255.0;
    textureStore(target, id.xy, vec4<f32>(level, level, level, 1.0));
}
";

#[test]
fn a_storage_texture_is_bound_with_its_access_and_component_type() {
    let msl = translate(COPY).expect("storage textures translate");
    assert!(
        msl.contains("texture2d<uint, access::read> source [[texture(0)]]"),
        "{msl}"
    );
    assert!(
        msl.contains("texture2d<float, access::write> target [[texture(1)]]"),
        "{msl}"
    );
}

#[test]
fn load_and_store_become_read_and_write() {
    let msl = translate(COPY).expect("storage textures translate");
    assert!(msl.contains("source.read(uint2(id.xy))"), "{msl}");
    assert!(
        msl.contains("target.write(float4(level, level, level, 1.0), uint2(id.xy))"),
        "{msl}"
    );
}

#[test]
fn other_dimensions_index_with_their_own_coordinate_type() {
    let msl = translate(
        "
@group(0) @binding(0) var line: texture_storage_1d<r32float, write>;
@group(0) @binding(1) var volume: texture_storage_3d<rgba32float, read_write>;
@group(0) @binding(2) var layers: texture_storage_2d_array<rgba8unorm, write>;
@compute @workgroup_size(1)
fn run(@builtin(global_invocation_id) id: vec3<u32>) {
    textureStore(line, id.x, vec4<f32>(1.0));
    textureStore(volume, id, textureLoad(volume, id));
    textureStore(layers, id.xy, id.z, vec4<f32>(0.5));
}
",
    )
    .expect("storage textures translate");
    assert!(
        msl.contains("texture1d<float, access::write> line"),
        "{msl}"
    );
    assert!(
        msl.contains("texture3d<float, access::read_write> volume"),
        "{msl}"
    );
    assert!(
        msl.contains("texture2d_array<float, access::write> layers"),
        "{msl}"
    );
    assert!(msl.contains("line.write(float4(1.0), uint(id.x))"), "{msl}");
    assert!(
        msl.contains("volume.write(volume.read(uint3(id)), uint3(id))"),
        "{msl}"
    );
    assert!(
        msl.contains("layers.write(float4(0.5), uint2(id.xy), uint(id.z))"),
        "{msl}"
    );
}

#[test]
fn an_unknown_format_or_access_is_a_parse_error() {
    let unknown_format = translate(
        "@group(0) @binding(0) var t: texture_storage_2d<rgba16sint, write>;\n@compute @workgroup_size(1) fn run() {}",
    )
    .unwrap_err();
    assert!(unknown_format.contains("rgba16sint"), "{unknown_format}");
    let unknown_access = translate(
        "@group(0) @binding(0) var t: texture_storage_2d<rgba8unorm, append>;\n@compute @workgroup_size(1) fn run() {}",
    )
    .unwrap_err();
    assert!(unknown_access.contains("append"), "{unknown_access}");
}
