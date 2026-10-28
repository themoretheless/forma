alias Value = f32;
// count,rank,groups,operation,offsetA,offsetB,outputOffset; then(dim,strideA,strideB).
@group(0) @binding(0) var<storage,read> p: array<u32>;
@group(0) @binding(1) var<storage,read> a: array<Value>;
@group(0) @binding(2) var<storage,read> b: array<Value>;
@group(0) @binding(3) var<storage,read_write> output: array<u32>;
fn compare(x:Value,y:Value)->bool{switch p[3]{case 0u:{return x==y;}case 1u:{return x!=y;}case 2u:{return x<y;}case 3u:{return x<=y;}case 4u:{return x>y;}default:{return x>=y;}}}
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid:vec3<u32>){
    var i=gid.x;
    while i<p[0]{var rem=i;var aa=p[4];var bb=p[5];
        for(var axis=p[1];axis>0u;axis--){let d=7u+(axis-1u)*3u;let c=rem%p[d];rem/=p[d];aa+=c*p[d+1u];bb+=c*p[d+2u];}
        output[p[6]+i]=select(0u,1u,compare(a[aa],b[bb]));
        if p[2]*256u>=p[0]-i{break;}i+=p[2]*256u;
    }
}
