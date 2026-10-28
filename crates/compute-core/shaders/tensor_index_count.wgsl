// indexCount,rank,groups,indexOffset,axisLength,countOffset; then(dim,stride).
@group(0) @binding(0) var<storage,read> p:array<u32>;
@group(0) @binding(1) var<storage,read> indices:array<u32>;
@group(0) @binding(2) var<storage,read_write> invalid:array<atomic<u32>>;
var<workgroup> scratch:array<u32,256>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid:vec3<u32>,@builtin(local_invocation_id) lid:vec3<u32>){
    var sum=0u;var i=gid.x;
    while i<p[0]{var rem=i;var address=p[3];for(var axis=p[1];axis>0u;axis--){let d=6u+(axis-1u)*2u;address+=(rem%p[d])*p[d+1u];rem/=p[d];}sum+=select(0u,1u,indices[address]>=p[4]);if p[2]*256u>=p[0]-i{break;}i+=p[2]*256u;}
    scratch[lid.x]=sum;for(var step=128u;step>0u;step/=2u){workgroupBarrier();if lid.x<step{scratch[lid.x]+=scratch[lid.x+step];}}
    if lid.x==0u && scratch[0]!=0u{atomicAdd(&invalid[p[5]],scratch[0]);}
}
