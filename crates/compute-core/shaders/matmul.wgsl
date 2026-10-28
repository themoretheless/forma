// Fixed 32x32 output tile; 64 one-dimensional lanes each accumulate a 4x4
// register tile. Shared loads are reused across columns/rows and accumulators.
// Tile geometry is intentionally fixed rather than using the WG tuning anchor.
struct Params { rows: u32, inner: u32, columns: u32, groups: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
const TILE: u32 = 32;
var<workgroup> tile_a: array<f32, 1024>;
var<workgroup> tile_b: array<f32, 1024>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let lane_row = (lid.x / 8u) * 4u;
    let lane_col = (lid.x % 8u) * 4u;
    let tile_columns = params.columns / TILE + select(0u, 1u, params.columns % TILE != 0u);
    let tile_rows = params.rows / TILE + select(0u, 1u, params.rows % TILE != 0u);
    let inner_tiles = params.inner / TILE + select(0u, 1u, params.inner % TILE != 0u);
    for (var tile = wid.x; tile < tile_rows * tile_columns; tile += params.groups) {
        let tile_row = (tile / tile_columns) * TILE;
        let tile_col = (tile % tile_columns) * TILE;
        let row = tile_row + lane_row;
        let col = tile_col + lane_col;
        var sums: array<vec4<f32>, 4>;
        for (var kt = 0u; kt < inner_tiles; kt++) {
            for (var slot = lid.x; slot < 1024u; slot += 64u) {
                let local_row = slot / TILE;
                let local_col = slot % TILE;
                let a_row = tile_row + local_row;
                let a_col = kt * TILE + local_col;
                let b_row = kt * TILE + local_row;
                let b_col = tile_col + local_col;
                tile_a[slot] = 0.0;
                tile_b[slot] = 0.0;
                if (a_row < params.rows && a_col < params.inner) {
                    tile_a[slot] = a[a_row * params.inner + a_col];
                }
                if (b_row < params.inner && b_col < params.columns) {
                    tile_b[slot] = b[b_row * params.columns + b_col];
                }
            }
            workgroupBarrier();
            for (var k = 0u; k < TILE; k++) {
                let av = vec4<f32>(
                    tile_a[lane_row * TILE + k], tile_a[(lane_row + 1u) * TILE + k],
                    tile_a[(lane_row + 2u) * TILE + k], tile_a[(lane_row + 3u) * TILE + k]
                );
                let bv = vec4<f32>(
                    tile_b[k * TILE + lane_col], tile_b[k * TILE + lane_col + 1u],
                    tile_b[k * TILE + lane_col + 2u], tile_b[k * TILE + lane_col + 3u]
                );
                sums[0] = fma(av.xxxx, bv, sums[0]);
                sums[1] = fma(av.yyyy, bv, sums[1]);
                sums[2] = fma(av.zzzz, bv, sums[2]);
                sums[3] = fma(av.wwww, bv, sums[3]);
            }
            workgroupBarrier();
        }
        for (var r = 0u; r < 4u; r++) {
            for (var c = 0u; c < 4u; c++) {
                if (row + r < params.rows && col + c < params.columns) {
                    output[(row + r) * params.columns + col + c] = sums[r][c];
                }
            }
        }
    }
}
