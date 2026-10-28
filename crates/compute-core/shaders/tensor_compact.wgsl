alias Value = f32;
// count,rank,groups,scanBlockSize,inputOffset,outputOffset,countOffset;
// then(dim,inputStride). Mask/local/block offsets refer to dense logical input.
@group(0) @binding(0) var<storage,read> p:array<u32>;
@group(0) @binding(1) var<storage,read> input:array<Value>;
@group(0) @binding(2) var<storage,read> mask:array<u32>;
@group(0) @binding(3) var<storage,read> local_offsets:array<u32>;
@group(0) @binding(4) var<storage,read> block_offsets:array<u32>;
@group(0) @binding(5) var<storage,read_write> output:array<Value>;
@group(0) @binding(6) var<storage,read_write> count:array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid:vec3<u32>){
    var selected=0u;if p[0]>0u{let last=p[0]-1u;selected=local_offsets[last]+block_offsets[last/p[3]]+select(0u,1u,mask[last]!=0u);}
    if gid.x==0u{count[p[6]]=selected;}
    var i=gid.x;while i<p[0]{if mask[i]!=0u{var rem=i;var address=p[4];for(var axis=p[1];axis>0u;axis--){let d=7u+(axis-1u)*2u;address+=(rem%p[d])*p[d+1u];rem/=p[d];}let position=local_offsets[i]+block_offsets[i/p[3]];output[p[5]+position]=input[address];}
        if i>=selected{output[p[5]+i]=Value(0);}if p[2]*256u>=p[0]-i{break;}i+=p[2]*256u;
    }
}
