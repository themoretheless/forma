alias Value = f32;
// rows,axisLength,blocksPerRow,groups,inclusive,reverse.
@group(0) @binding(0) var<storage,read> p:array<u32>;
@group(0) @binding(1) var<storage,read> input:array<Value>;
@group(0) @binding(2) var<storage,read_write> output:array<Value>;
@group(0) @binding(3) var<storage,read_write> totals:array<Value>;
var<workgroup> scratch:array<Value,256>;
@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid:vec3<u32>,@builtin(workgroup_id) wid:vec3<u32>){
    let count=p[0]*p[2];var work=wid.x;
    while work<count{
        let row=work/p[2];let block=work%p[2];let position=block*256u+lid.x;
        var value=Value(0);var physical=position;
        if position<p[1]{if p[5]!=0u{physical=p[1]-1u-position;}value=input[row*p[1]+physical];}
        scratch[lid.x]=value;workgroupBarrier();
        for(var step=1u;step<256u;step*=2u){var previous=Value(0);if lid.x>=step{previous=scratch[lid.x-step];}workgroupBarrier();scratch[lid.x]+=previous;workgroupBarrier();}
        if position<p[1]{var result=scratch[lid.x];if p[4]==0u{result=Value(0);if lid.x>0u{result=scratch[lid.x-1u];}}output[row*p[1]+physical]=result;}
        if lid.x==0u{totals[work]=scratch[255];}workgroupBarrier();
        if p[3]>=count-work{break;}work+=p[3];
    }
}
