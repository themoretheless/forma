alias Value = f32;
// updateCount, rank, groups, indexOffset, updateOffset, outputOffset,
// axisLength, outputAxisStride, operation;
// then (dim,outputStride,indexStride,updateStride,indexFlatStride) per update axis.
@group(0) @binding(0) var<storage, read> p: array<u32>;
@group(0) @binding(1) var<storage, read> indices: array<u32>;
@group(0) @binding(2) var<storage, read> updates: array<Value>;
@group(0) @binding(3) var<storage, read> owners: array<u32>;
@group(0) @binding(4) var<storage, read_write> output: array<atomic<u32>>;

fn fold(a: Value, b: Value) -> Value {
    switch p[8] {
        case 1u: { return a + b; }
        case 2u: { return a * b; }
        case 3u: { return min(a, b); }
        default: { return max(a, b); }
    }
}

fn accumulate(address: u32, update: Value) {
    // u32 uses native atomics where available; specialized source replaces this.
    var old = atomicLoad(&output[address]);
    loop {
        let value = fold(bitcast<Value>(old), update);
        let attempt = atomicCompareExchangeWeak(&output[address], old, bitcast<u32>(value));
        if attempt.exchanged { break; }
        old = attempt.old_value;
    }
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var i = gid.x;
    while i < p[0] {
        var remaining = i;
        var destination = p[5];
        var index_address = p[3];
        var update_address = p[4];
        var logical_index = 0u;
        for (var axis = p[1]; axis > 0u; axis--) {
            let d = 9u + (axis - 1u) * 5u;
            let coordinate = remaining % p[d];
            remaining /= p[d];
            destination += coordinate * p[d + 1u];
            index_address += coordinate * p[d + 2u];
            update_address += coordinate * p[d + 3u];
            logical_index += coordinate * p[d + 4u];
        }
        let selected = indices[index_address];
        // Validate before touching owners or the destination buffer.
        if selected < p[6] {
            destination += selected * p[7];
            if p[8] == 0u {
                if owners[selected] == logical_index + 1u {
                    atomicStore(&output[destination], bitcast<u32>(updates[update_address]));
                }
            } else {
                accumulate(destination, updates[update_address]);
            }
        }
        if p[2] * 256u >= p[0] - i { break; }
        i += p[2] * 256u;
    }
}
