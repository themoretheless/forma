// Exclusive Blelloch scan inside each block; block totals feed the next level.
// Workgroups visit disjoint blocks in strides, independent of dispatch limits.
struct Params {
    count: u32,
    groups: u32,
    normalize: u32,
    _pad: u32,
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
@group(0) @binding(3) var<storage, read_write> totals: array<u32>;
const WG: u32 = 256;
var<workgroup> scratch: array<u32, WG>;

@compute @workgroup_size(WG)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let blocks = params.count / WG + select(0u, 1u, params.count % WG != 0u);
    for (var block = wid.x; block < blocks; block += params.groups) {
        let i = block * WG + lid.x;
        var value = 0u;
        if (i < params.count) {
            value = input[i];
            if (params.normalize != 0u) {
                value = select(0u, 1u, value != 0u);
            }
        }
        scratch[lid.x] = value;
        workgroupBarrier();
        for (var step = 1u; step < WG; step *= 2u) {
            let right = (lid.x + 1u) * step * 2u - 1u;
            if (right < WG) {
                scratch[right] += scratch[right - step];
            }
            workgroupBarrier();
        }
        if (lid.x == 0u) {
            totals[block] = scratch[WG - 1u];
            scratch[WG - 1u] = 0u;
        }
        workgroupBarrier();
        for (var step = WG / 2u; step > 0u; step /= 2u) {
            let right = (lid.x + 1u) * step * 2u - 1u;
            if (right < WG) {
                let left = scratch[right - step];
                scratch[right - step] = scratch[right];
                scratch[right] += left;
            }
            workgroupBarrier();
        }
        if (i < params.count) {
            output[i] = scratch[lid.x];
        }
        workgroupBarrier();
    }
}
