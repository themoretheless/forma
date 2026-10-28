alias Value = f32;
// outputCount,rank,groups,inputOffset,indexOffset,outputOffset,axisLength,axisStride;
// then(dim,inputStride,indexStride) for output dimensions.
@group(0) @binding(0) var<storage,read> p:array<u32>;
@group(0) @binding(1) var<storage,read> input:array<Value>;
@group(0) @binding(2) var<storage,read> indices:array<u32>;
@group(0) @binding(3) var<storage,read_write> output:array<Value>;
fn result_value(i:u32)->Value {
    var rem=i;var source=p[3];var index=p[4];
    for(var axis=p[1];axis>0u;axis--){let d=8u+(axis-1u)*3u;let c=rem%p[d];rem/=p[d];source+=c*p[d+1u];index+=c*p[d+2u];}
    let selected=indices[index];var value=Value(0);if selected<p[6]{value=input[source+selected*p[7]];}return value;
}
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid:vec3<u32>){var i=gid.x;
    while i<p[0]{output[p[5]+i]=result_value(i);
        if p[2]*256u>=p[0]-i{break;}i+=p[2]*256u;
    }
}
