// WebGPU Compute Marching Cubes: Voxel classification, edge interpolation, and triangle generation.

struct GridParams {
    nx: u32,
    ny: u32,
    nz: u32,
    total_cells: u32,
    min_x: f32,
    min_y: f32,
    min_z: f32,
    iso_level: f32,
    step_x: f32,
    step_y: f32,
    step_z: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> grid: GridParams;
@group(0) @binding(1) var<storage, read> field_values: array<f32>;
@group(0) @binding(2) var<storage, read> tri_table: array<i32>; // 256 * 16
@group(0) @binding(3) var<storage, read> num_triangles_table: array<u32>; // 256

// Outputs for classification phase
@group(1) @binding(0) var<storage, read_write> cell_cube_index: array<u32>;
@group(1) @binding(1) var<storage, read_write> cell_tri_count: array<u32>;

// Corner relative offsets in a cube
const CORNER_OFFSETS = array<vec3<u32>, 8>(
    vec3<u32>(0u, 0u, 0u),
    vec3<u32>(1u, 0u, 0u),
    vec3<u32>(1u, 1u, 0u),
    vec3<u32>(0u, 1u, 0u),
    vec3<u32>(0u, 0u, 1u),
    vec3<u32>(1u, 0u, 1u),
    vec3<u32>(1u, 1u, 1u),
    vec3<u32>(0u, 1u, 1u)
);

fn grid_point_index(x: u32, y: u32, z: u32) -> u32 {
    let row = grid.nx + 1u;
    let slice = row * (grid.ny + 1u);
    return x + y * row + z * slice;
}

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn classify_cells(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell_idx = id.x;
    if (cell_idx >= grid.total_cells) {
        return;
    }

    let x = cell_idx % grid.nx;
    let y = (cell_idx / grid.nx) % grid.ny;
    let z = cell_idx / (grid.nx * grid.ny);

    var cube_idx = 0u;
    for (var i = 0u; i < 8u; i++) {
        let corner = CORNER_OFFSETS[i];
        let p_idx = grid_point_index(x + corner.x, y + corner.y, z + corner.z);
        let val = field_values[p_idx];
        if (val < grid.iso_level) {
            cube_idx |= (1u << i);
        }
    }

    cell_cube_index[cell_idx] = cube_idx;
    cell_tri_count[cell_idx] = num_triangles_table[cube_idx];
}

// -------------------------------------------------------------
// Pass 2: Generate Vertices and Indices
// -------------------------------------------------------------

struct Vertex {
    position: vec3<f32>,
    normal: vec3<f32>,
}

@group(2) @binding(0) var<storage, read> cell_offsets: array<u32>; // from prefix scan
@group(2) @binding(1) var<storage, read_write> out_vertices: array<f32>; // x,y,z, nx,ny,nz per vertex (stride 6 floats)
@group(2) @binding(2) var<storage, read_write> out_indices: array<u32>; // 3 u32 per triangle

// 12 edges connecting the 8 corners: [cornerA, cornerB]
const EDGE_CORNERS = array<vec2<u32>, 12>(
    vec2<u32>(0u, 1u), vec2<u32>(1u, 2u), vec2<u32>(2u, 3u), vec2<u32>(3u, 0u),
    vec2<u32>(4u, 5u), vec2<u32>(5u, 6u), vec2<u32>(6u, 7u), vec2<u32>(7u, 4u),
    vec2<u32>(0u, 4u), vec2<u32>(1u, 5u), vec2<u32>(2u, 6u), vec2<u32>(3u, 7u)
);

fn corner_position(x: u32, y: u32, z: u32, corner_idx: u32) -> vec3<f32> {
    let c = CORNER_OFFSETS[corner_idx];
    return vec3<f32>(
        grid.min_x + f32(x + c.x) * grid.step_x,
        grid.min_y + f32(y + c.y) * grid.step_y,
        grid.min_z + f32(z + c.z) * grid.step_z
    );
}

fn interpolate_edge(
    p1: vec3<f32>, val1: f32,
    p2: vec3<f32>, val2: f32
) -> vec3<f32> {
    let diff = val2 - val1;
    if (abs(diff) < 1e-6) {
        return (p1 + p2) * 0.5;
    }
    let t = clamp((grid.iso_level - val1) / diff, 0.0, 1.0);
    return p1 + t * (p2 - p1);
}

@compute @workgroup_size(WG)
fn emit_triangles(@builtin(global_invocation_id) id: vec3<u32>) {
    let cell_idx = id.x;
    if (cell_idx >= grid.total_cells) {
        return;
    }

    let tri_cnt = cell_tri_count[cell_idx];
    if (tri_cnt == 0u) {
        return;
    }

    let cube_idx = cell_cube_index[cell_idx];
    let base_tri_idx = cell_offsets[cell_idx];

    let x = cell_idx % grid.nx;
    let y = (cell_idx / grid.nx) % grid.ny;
    let z = cell_idx / (grid.nx * grid.ny);

    // Read the 8 corner values and positions
    var vals: array<f32, 8>;
    var pos: array<vec3<f32>, 8>;
    for (var i = 0u; i < 8u; i++) {
        let corner = CORNER_OFFSETS[i];
        let p_idx = grid_point_index(x + corner.x, y + corner.y, z + corner.z);
        vals[i] = field_values[p_idx];
        pos[i] = corner_position(x, y, z, i);
    }

    // Precalculate vertices only on edges referenced by emitted triangles
    let table_offset = cube_idx * 16u;
    var edge_mask = 0u;
    for (var t = 0u; t < tri_cnt; t++) {
        let e0 = u32(tri_table[table_offset + t * 3u + 0u]);
        let e1 = u32(tri_table[table_offset + t * 3u + 1u]);
        let e2 = u32(tri_table[table_offset + t * 3u + 2u]);
        edge_mask |= (1u << e0) | (1u << e1) | (1u << e2);
    }

    var edge_verts: array<vec3<f32>, 12>;
    for (var e = 0u; e < 12u; e++) {
        if ((edge_mask & (1u << e)) != 0u) {
            let c = EDGE_CORNERS[e];
            edge_verts[e] = interpolate_edge(pos[c.x], vals[c.x], pos[c.y], vals[c.y]);
        }
    }

    // Write triangles
    for (var t = 0u; t < tri_cnt; t++) {
        let e0 = u32(tri_table[table_offset + t * 3u + 0u]);
        let e1 = u32(tri_table[table_offset + t * 3u + 1u]);
        let e2 = u32(tri_table[table_offset + t * 3u + 2u]);

        let v0 = edge_verts[e0];
        let v1 = edge_verts[e1];
        let v2 = edge_verts[e2];

        // Face normal
        let d1 = v1 - v0;
        let d2 = v2 - v0;
        var norm = normalize(cross(d1, d2));
        if (dot(norm, norm) < 1e-6) {
            norm = vec3<f32>(0.0, 0.0, 1.0);
        }

        let global_tri_idx = base_tri_idx + t;
        let vert_base = global_tri_idx * 3u;
        let out_v_offset = vert_base * 6u;

        // Vertex 0
        out_vertices[out_v_offset + 0u] = v0.x;
        out_vertices[out_v_offset + 1u] = v0.y;
        out_vertices[out_v_offset + 2u] = v0.z;
        out_vertices[out_v_offset + 3u] = norm.x;
        out_vertices[out_v_offset + 4u] = norm.y;
        out_vertices[out_v_offset + 5u] = norm.z;

        // Vertex 1
        out_vertices[out_v_offset + 6u] = v1.x;
        out_vertices[out_v_offset + 7u] = v1.y;
        out_vertices[out_v_offset + 8u] = v1.z;
        out_vertices[out_v_offset + 9u] = norm.x;
        out_vertices[out_v_offset + 10u] = norm.y;
        out_vertices[out_v_offset + 11u] = norm.z;

        // Vertex 2
        out_vertices[out_v_offset + 12u] = v2.x;
        out_vertices[out_v_offset + 13u] = v2.y;
        out_vertices[out_v_offset + 14u] = v2.z;
        out_vertices[out_v_offset + 15u] = norm.x;
        out_vertices[out_v_offset + 16u] = norm.y;
        out_vertices[out_v_offset + 17u] = norm.z;

        let out_i_offset = global_tri_idx * 3u;
        out_indices[out_i_offset + 0u] = vert_base + 0u;
        out_indices[out_i_offset + 1u] = vert_base + 1u;
        out_indices[out_i_offset + 2u] = vert_base + 2u;
    }
}
