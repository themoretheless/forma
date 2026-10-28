alias Value = f32;
// count,rank,groups,maskOffset,trueOffset,falseOffset,outputOffset;
// then(dim,maskStride,trueStride,falseStride).
@group(0) @binding(0) var<storage,read> p: array<u32>;
@group(0) @binding(1) var<storage,read> mask: array<u32>;
@group(0) @binding(2) var<storage,read> a: array<Value>;
@group(0) @binding(3) var<storage,read> b: array<Value>;
@group(0) @binding(4) var<storage,read_write> output: array<Value>;
fn result_value(i:u32)->Value {
    var rem=i;var mm=p[3];var aa=p[4];var bb=p[5];
    for(var axis=p[1];axis>0u;axis--){let d=7u+(axis-1u)*4u;let c=rem%p[d];rem/=p[d];mm+=c*p[d+1u];aa+=c*p[d+2u];bb+=c*p[d+3u];}
    return select(b[bb],a[aa],mask[mm]!=0u);
}
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid:vec3<u32>){
    var i=gid.x;
    while i<p[0]{output[p[6]+i]=result_value(i);
        if p[2]*256u>=p[0]-i{break;}i+=p[2]*256u;
    }
}
