@group(0) @binding(0) var<storage,read> p:array<u32>;
@group(0) @binding(1) var<storage,read_write> output:array<u32>;
@compute @workgroup_size(1)
fn main(){output[p[0]]=0u;}
