struct Params {
    pair_count: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a_points: array<f32>;
@group(0) @binding(2) var<storage, read> b_points: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_dist: array<f32>;

const WG: u32 = 256;

@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.pair_count) {
        return;
    }
    let base = i * 3u;
    let dx = a_points[base] - b_points[base];
    let dy = a_points[base + 1u] - b_points[base + 1u];
    let dz = a_points[base + 2u] - b_points[base + 2u];
    out_dist[i] = dx * dx + dy * dy + dz * dz;
}
