@group(0) @binding(0) var<storage,read> f64_a: array<D64>;
@group(0) @binding(1) var<storage,read> f64_b: array<D64>;
@group(0) @binding(2) var<storage,read_write> f64_output: array<D64>;
@group(0) @binding(3) var<storage,read> f64_meta: array<u32>;
const WG: u32 = 256;
var<workgroup> f64_shared: array<D64,256>;
fn d64_index(linear:u32, start:u32, rank:u32, offset:u32)->u32 {
    var remaining=linear; var address=offset;
    for(var axis=rank;axis>0u;axis-=1u) {
        let n=f64_meta[start+axis-1u];
        address+=(remaining%n)*f64_meta[start+rank+axis-1u]; remaining/=n;
    }
    return address;
}
fn d64_min(a:D64,b:D64)->D64 {
    if d64_nan(a) { return d64_quiet(a); }
    if d64_nan(b) { return d64_quiet(b); }
    if d64_zero(a) && d64_zero(b) { return D64(0u,(a.y|b.y)&0x80000000u); }
    return select(b,a,d64_lt(a,b));
}
fn d64_max(a:D64,b:D64)->D64 {
    if d64_nan(a) { return d64_quiet(a); }
    if d64_nan(b) { return d64_quiet(b); }
    if d64_zero(a) && d64_zero(b) { return D64(0u,a.y&b.y&0x80000000u); }
    return select(a,b,d64_lt(a,b));
}
fn d64_binary(a:D64,b:D64,op:u32)->D64 {
    switch op {
        case 0u: { return d64_add(a,b); }
        case 1u: { return d64_sub(a,b); }
        case 2u: { return d64_mul(a,b); }
        case 3u: { return d64_div(a,b); }
        case 4u: { return d64_min(a,b); }
        default: { return d64_max(a,b); }
    }
}
fn d64_unary(a:D64,op:u32)->D64 {
    switch op {
        case 0u: { return d64_neg(a); }
        case 1u: { return d64_abs(a); }
        case 2u: { return d64_mul(a,a); }
        case 3u: { return d64_sqrt(a); }
        case 4u: { return d64_div(D64_ONE,a); }
        case 5u: { return d64_exp(a); }
        case 6u: { return d64_log(a); }
        case 7u: { return d64_sincos(a,false); }
        case 8u: { return d64_sincos(a,true); }
        default: { return a; }
    }
}
fn d64_identity(op:u32)->D64 {
    switch op {
        case 1u: { return D64_ONE; }
        case 2u: { return D64(0u,0x7ff00000u); }
        case 3u: { return D64(0u,0xfff00000u); }
        default: { return D64(0u); }
    }
}
fn d64_fold(a:D64,b:D64,op:u32)->D64 {
    switch op {
        case 1u: { return d64_mul(a,b); }
        case 2u: { return d64_min(a,b); }
        case 3u: { return d64_max(a,b); }
        default: { return d64_add(a,b); }
    }
}
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) id:vec3<u32>, @builtin(local_invocation_index) lane:u32, @builtin(workgroup_id) group:vec3<u32>, @builtin(num_workgroups) groups:vec3<u32>) {
    let mode=f64_meta[0]; let op=f64_meta[1]; let count=f64_meta[2];
    let ar=f64_meta[3]; let br=f64_meta[4]; let ao=f64_meta[5]; let bo=f64_meta[6];
    let inner=f64_meta[7]; let rows=f64_meta[8]; let cols=f64_meta[9];
    let bm=10u+2u*ar;
    if mode==2u {
        for(var i=group.x;i<count;i+=groups.x) {
            let base=d64_index(i,10u,ar,ao);
            var sum=d64_identity(op);
            for(var r=lane;r<inner;r+=WG) { sum=d64_fold(sum,f64_a[base+d64_index(r,bm,br,0u)],op); }
            f64_shared[lane]=sum;
            workgroupBarrier();
            for(var width=WG/2u;width>0u;width/=2u) {
                if lane<width { f64_shared[lane]=d64_fold(f64_shared[lane],f64_shared[lane+width],op); }
                workgroupBarrier();
            }
            if lane==0u { f64_output[i]=f64_shared[0]; }
            workgroupBarrier();
        }
        return;
    }
    for(var i=id.x;i<count;i+=groups.x*WG) {
        if mode==3u {
            let row=(i/cols)%rows; let batch=i/cols/rows; let col=i%cols;
            var sum=D64(0u);
            for(var r=0u;r<inner;r+=1u) {
                let a=f64_a[d64_index((batch*rows+row)*inner+r,10u,ar,ao)];
                let b=f64_b[d64_index((batch*inner+r)*cols+col,bm,br,bo)];
                sum=d64_add(sum,d64_mul(a,b));
            }
            f64_output[i]=sum;
        } else {
            let a=f64_a[d64_index(i,10u,ar,ao)];
            if mode==0u { f64_output[i]=d64_unary(a,op); }
            else { f64_output[i]=d64_binary(a,f64_b[d64_index(i,bm,br,bo)],op); }
        }
    }
}
