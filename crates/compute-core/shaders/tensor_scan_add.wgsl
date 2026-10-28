alias Value = f32;
// count,axisLength,blocksPerRow,groups,reverse.
@group(0) @binding(0) var<storage,read> p:array<u32>;
@group(0) @binding(1) var<storage,read> offsets:array<Value>;
@group(0) @binding(2) var<storage,read_write> output:array<Value>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid:vec3<u32>){var i=gid.x;
    while i<p[0]{let row=i/p[1];var column=i%p[1];if p[4]!=0u{column=p[1]-1u-column;}output[i]+=offsets[row*p[2]+column/256u];if p[3]*256u>=p[0]-i{break;}i+=p[3]*256u;}
}
