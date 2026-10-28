// Header: count, rank, operation, groups, offsetA, offsetB, outputOffset, reserved.
// Then rank triples: dimension, strideA, strideB. All offsets/strides are f32 units.
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
const WG: u32 = 256;
fn address(index: u32, operand: u32) -> u32 {
    var offset = params[4u + operand];
    var remaining = index;
    for (var axis = params[1]; axis > 0u; axis--) {
        let descriptor = 8u + (axis - 1u) * 3u;
        let dimension = params[descriptor];
        offset += (remaining % dimension) * params[descriptor + 1u + operand];
        remaining /= dimension;
    }
    return offset;
}
fn apply_unary(operation: u32, x: f32) -> f32 {
    switch operation {
        case 1u: { return -x; }
        case 2u: { return abs(x); }
        case 3u: { return x * x; }
        case 4u: { return sqrt(x); }
        case 5u: { return 1.0 / x; }
        case 6u: { return exp(x); }
        case 7u: { return log(x); }
        case 8u: { return sin(x); }
        case 9u: { return cos(x); }
        default: { return x; }
    }
}
fn apply_binary(operation: u32, x: f32, y: f32) -> f32 {
    switch operation {
        case 32u: { return x + y; }
        case 33u: { return x - y; }
        case 34u: { return x * y; }
        case 35u: { return x / y; }
        case 36u: { return min(x, y); }
        default: { return max(x, y); }
    }
}
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let count = params[0];
    let stride = params[3] * WG;
    var index = gid.x;
    while index < count {
        let x = a[address(index, 0u)];
        var value = apply_unary(params[2], x);
        if params[2] >= 32u { value = apply_binary(params[2], x, b[address(index, 1u)]); }
        output[params[6] + index] = value;
        if stride >= count - index { break; }
        index += stride;
    }
}
