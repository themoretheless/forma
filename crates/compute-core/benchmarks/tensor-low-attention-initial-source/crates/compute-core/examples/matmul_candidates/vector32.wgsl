// Fully aligned 32x32 tile with vec4 global/shared loads and vec4 stores.
struct Params { rows: u32, inner: u32, columns: u32, groups: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> b: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> output: array<vec4<f32>>;
var<workgroup> tile_a: array<vec4<f32>, 256>;
var<workgroup> tile_b: array<vec4<f32>, 256>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let lane_row = (lid.x / 8u) * 4u;
    let lane_col4 = lid.x % 8u;
    let tile_columns = params.columns / 32u;
    let tile_rows = params.rows / 32u;
    let inner_tiles = params.inner / 32u;
    for (var tile = wid.x; tile < tile_rows * tile_columns; tile += params.groups) {
        let tile_row = (tile / tile_columns) * 32u;
        let tile_col4 = (tile % tile_columns) * 8u;
        let row = tile_row + lane_row;
        var sums: array<vec4<f32>, 4>;
        for (var kt = 0u; kt < inner_tiles; kt++) {
            for (var slot = lid.x; slot < 256u; slot += 64u) {
                let local_row = slot / 8u;
                let local_col4 = slot % 8u;
                tile_a[slot] = a[(tile_row + local_row) * (params.inner / 4u) + kt * 8u + local_col4];
                tile_b[slot] = b[(kt * 32u + local_row) * (params.columns / 4u) + tile_col4 + local_col4];
            }
            workgroupBarrier();
            for (var k = 0u; k < 32u; k++) {
                let av = vec4<f32>(
                    tile_a[lane_row * 8u + k / 4u][k % 4u],
                    tile_a[(lane_row + 1u) * 8u + k / 4u][k % 4u],
                    tile_a[(lane_row + 2u) * 8u + k / 4u][k % 4u],
                    tile_a[(lane_row + 3u) * 8u + k / 4u][k % 4u]
                );
                let bv = tile_b[k * 8u + lane_col4];
                sums[0] = fma(av.xxxx, bv, sums[0]);
                sums[1] = fma(av.yyyy, bv, sums[1]);
                sums[2] = fma(av.zzzz, bv, sums[2]);
                sums[3] = fma(av.wwww, bv, sums[3]);
            }
            workgroupBarrier();
        }
        for (var r = 0u; r < 4u; r++) {
            output[(row + r) * (params.columns / 4u) + tile_col4 + lane_col4] = sums[r];
        }
    }
}
