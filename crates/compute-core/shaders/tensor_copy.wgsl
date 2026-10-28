alias Value = f32;
// count,rank,groups,inputOffset,outputOffset; then (dimension,inputStride).
@group(0) @binding(0) var<storage,read> p: array<u32>;
@group(0) @binding(1) var<storage,read> input: array<Value>;
@group(0) @binding(2) var<storage,read_write> output: array<Value>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var i=gid.x;
    while i<p[0] {
        var rem=i;var address=p[3];
        for(var axis=p[1];axis>0u;axis--){let d=5u+(axis-1u)*2u;address+=(rem%p[d])*p[d+1u];rem/=p[d];}
        output[p[4]+i]=input[address];
        if p[2]*256u>=p[0]-i{break;}i+=p[2]*256u;
    }
}
