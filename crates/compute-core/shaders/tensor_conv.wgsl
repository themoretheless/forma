// Channel-first grouped cross-correlation, ranks 1..3. One group reduces an
// output at a time; only 256 f32 partials are retained in workgroup memory.
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;
@group(0) @binding(2) var<storage, read> weight: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
var<workgroup> partial: array<f32, 256>;

fn product(a: u32, b: u32) -> f32 { return input[a] * weight[b]; }

@compute @workgroup_size(256)
fn main(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_id) lane: vec3<u32>) {
    var index = group.x;
    loop {
        if index >= params[0] { break; }
        var spatial: array<u32, 3>;
        var remaining = index;
        var axis = params[2];
        loop {
            if axis == 0u { break; }
            axis -= 1u;
            let p = 15u + axis * 8u;
            spatial[axis] = remaining % params[p + 1u];
            remaining /= params[p + 1u];
        }
        let channel = remaining % params[3];
        let batch = remaining / params[3];
        let input_group = (channel / params[4]) * params[5];
        var sum = 0.0;
        var term = lane.x;
        loop {
            if term >= params[7] { break; }
            let c = term / params[6];
            var kernel = term % params[6];
            var a = params[8] + batch * params[11] + (input_group + c) * params[12];
            var b = params[9] + channel * params[13] + c * params[14];
            var valid = true;
            axis = params[2];
            loop {
                if axis == 0u { break; }
                axis -= 1u;
                let p = 15u + axis * 8u;
                let k = kernel % params[p + 2u];
                kernel /= params[p + 2u];
                let padded = spatial[axis] * params[p + 3u] + k * params[p + 4u];
                if padded < params[p + 5u] || padded - params[p + 5u] >= params[p] {
                    valid = false;
                } else {
                    a += (padded - params[p + 5u]) * params[p + 6u];
                }
                b += k * params[p + 7u];
            }
            if valid { sum += product(a, b); }
            if params[7] - term <= 256u { break; }
            term += 256u;
        }
        partial[lane.x] = sum;
        workgroupBarrier();
        var width = 128u;
        loop {
            if width == 0u { break; }
            if lane.x < width { partial[lane.x] += partial[lane.x + width]; }
            workgroupBarrier();
            width /= 2u;
        }
        if lane.x == 0u { output[params[10] + index] = partial[0]; }
        // All lanes finish reading the shared result before another iteration.
        workgroupBarrier();
        if params[0] - index <= params[1] { break; }
        index += params[1];
    }
}
