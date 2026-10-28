// Register-prefix four adjacent values per lane, then Blelloch-scan only the
// lane totals. A WG=256 block covers 1024 elements using the same shared-memory
// tree as the original 256-element scan. u32 arithmetic intentionally wraps.
struct Params { count: u32, groups: u32, normalize: u32, _pad: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
@group(0) @binding(3) var<storage, read_write> totals: array<u32>;
const WG: u32 = 256;
const ITEMS: u32 = 4;
var<workgroup> scratch: array<u32, WG>;
@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let size = WG * ITEMS;
    let blocks = params.count / size + select(0u, 1u, params.count % size != 0u);
    for (var block = wid.x; block < blocks; block += params.groups) {
        let start = block * size + lid.x * ITEMS;
        var prefixes: array<u32, ITEMS>;
        var sum = 0u;
        for (var k = 0u; k < ITEMS; k++) {
            prefixes[k] = sum;
            if start + k < params.count {
                let value = input[start+k];
                sum += select(value, select(0u,1u,value != 0u), params.normalize != 0u);
            }
        }
        scratch[lid.x] = sum;
        workgroupBarrier();
        for (var step = 1u; step < WG; step *= 2u) {
            let right = (lid.x+1u)*step*2u-1u;
            if right < WG { scratch[right] += scratch[right-step]; }
            workgroupBarrier();
        }
        if lid.x == 0u { totals[block] = scratch[WG-1u]; scratch[WG-1u] = 0u; }
        workgroupBarrier();
        for (var step = WG/2u; step > 0u; step /= 2u) {
            let right = (lid.x+1u)*step*2u-1u;
            if right < WG {
                let left = scratch[right-step];
                scratch[right-step] = scratch[right];
                scratch[right] += left;
            }
            workgroupBarrier();
        }
        let offset = scratch[lid.x];
        for (var k = 0u; k < ITEMS; k++) {
            if start+k < params.count { output[start+k] = offset+prefixes[k]; }
        }
        workgroupBarrier();
    }
}
