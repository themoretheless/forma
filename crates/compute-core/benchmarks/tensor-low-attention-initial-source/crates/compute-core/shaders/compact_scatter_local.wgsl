// Stable scatter from local scan offsets + global block offsets. Tail writes
// target [total,count), while scatter targets [0,total), so the two write sets
// are disjoint even in this single dispatch. No atomics or global spin barriers.
struct Params { count: u32, groups: u32, block_size: u32, _pad: u32 };
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> keep: array<u32>;
@group(0) @binding(3) var<storage, read> local_offsets: array<u32>;
@group(0) @binding(4) var<storage, read> block_offsets: array<u32>;
@group(0) @binding(5) var<storage, read_write> output: array<f32>;
@group(0) @binding(6) var<storage, read_write> selected_count: array<u32>;
const WG: u32 = 256;
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var total = 0u;
    if params.count != 0u {
        let last = params.count-1u;
        total = block_offsets[last/params.block_size]+local_offsets[last]+select(0u,1u,keep[last]!=0u);
    }
    if gid.x == 0u { selected_count[0] = total; }
    for (var i = gid.x; i < params.count; i += params.groups*WG) {
        if total == 0u {
            output[i] = 0.0;
        } else if total == params.count {
            output[i] = input[i];
        } else {
            if keep[i] != 0u {
                output[block_offsets[i/params.block_size]+local_offsets[i]] = input[i];
            }
            if i >= total { output[i] = 0.0; }
        }
    }
}
