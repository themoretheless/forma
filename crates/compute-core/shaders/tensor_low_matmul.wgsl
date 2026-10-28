// Header: m,n,k,batchRank,offsetA,offsetB,outputOffset,tileCount,tilesM,tilesN,
// groups,strideAM,strideAK,strideBK,strideBN,dtype.
// Then batchRank triples: batchDim,strideA,strideB.
@group(0) @binding(0) var<storage, read> params: array<u32>;
@group(0) @binding(1) var<storage, read> a: array<u32>;
@group(0) @binding(2) var<storage, read> b: array<u32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;
var<workgroup> tile_a: array<f32, 256>;
var<workgroup> tile_b: array<f32, 256>;
fn load_a(address: u32) -> f32 {
    let bits = (a[address / 2u] >> ((address & 1u) * 16u)) & 0xffffu;
    return bitcast<f32>(low_decode(bits, params[15]));
}
fn load_b(address: u32) -> f32 {
    let bits = (b[address / 2u] >> ((address & 1u) * 16u)) & 0xffffu;
    return bitcast<f32>(low_decode(bits, params[15]));
}
fn batch_address(index: u32, operand: u32) -> u32 {
    var offset = params[4u + operand];
    var remaining = index;
    for (var axis = params[3]; axis > 0u; axis--) {
        let descriptor = 16u + (axis - 1u) * 3u;
        offset += (remaining % params[descriptor]) * params[descriptor + 1u + operand];
        remaining /= params[descriptor];
    }
    return offset;
}
@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let local_row = lid.x / 16u;
    let local_column = lid.x % 16u;
    var tile = wid.x;
    while tile < params[7] {
        let tiles_per_batch = params[8] * params[9];
        let batch = tile / tiles_per_batch;
        let within_batch = tile % tiles_per_batch;
        let row = (within_batch / params[9]) * 16u + local_row;
        let column = (within_batch % params[9]) * 16u + local_column;
        let offset_a = batch_address(batch, 0u);
        let offset_b = batch_address(batch, 1u);
        var sum = 0.0;
        var inner = 0u;
        while inner < params[2] {
            let ka = inner + local_column;
            let kb = inner + local_row;
            var av = 0.0;
            var bv = 0.0;
            if row < params[0] && ka < params[2] { av = load_a(offset_a + row*params[11] + ka*params[12]); }
            if kb < params[2] && column < params[1] { bv = load_b(offset_b + kb*params[13] + column*params[14]); }
            tile_a[lid.x] = av;
            tile_b[lid.x] = bv;
            workgroupBarrier();
            for (var k = 0u; k < 16u; k++) { sum += tile_a[local_row*16u+k] * tile_b[k*16u+local_column]; }
            workgroupBarrier();
            if 16u >= params[2] - inner { break; }
            inner += 16u;
        }
        if row < params[0] && column < params[1] {
            output[params[6] + batch*params[0]*params[1] + row*params[1] + column] = sum;
        }
        if params[10] >= params[7] - tile { break; }
        tile += params[10];
    }
}
