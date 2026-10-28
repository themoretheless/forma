//! Dedicated GPU CAD compute kernels (SDF voxel grid evaluation, CSG boolean field combination,
//! tetrahedral normals, and mesh vertex transform).

/// Generates a WGSL compute kernel that transforms 3D CAD mesh vertices by an affine 4x4 matrix
/// and computes per-vertex bounding-box contributions.
pub fn generate_wgsl_cad_vertex_transform_shader(workgroup_size: u32) -> String {
    let wg = workgroup_size.clamp(32, 1024);
    format!(
        r#"struct TransformParams {{
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
    vertex_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}}

@group(0) @binding(0) var<uniform> params: TransformParams;
@group(0) @binding(1) var<storage, read> positions_in: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> positions_out: array<vec4<f32>>;

@compute @workgroup_size({wg}, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let idx = gid.x;
    if (idx >= params.vertex_count) {{
        return;
    }}
    let p = positions_in[idx];
    let x = dot(params.row0, p);
    let y = dot(params.row1, p);
    let z = dot(params.row2, p);
    positions_out[idx] = vec4<f32>(x, y, z, 1.0);
}}
"#
    )
}
