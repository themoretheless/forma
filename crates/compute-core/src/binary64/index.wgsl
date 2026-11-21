@group(0) @binding(0) var<storage,read> f64_a: array<D64>;
@group(0) @binding(1) var<storage,read> f64_b: array<D64>;
@group(0) @binding(2) var<storage,read_write> f64_output: array<D64>;
@group(0) @binding(3) var<storage,read> f64_meta: array<u32>;
const WG: u32 = 256;
fn d64_index(linear:u32, start:u32, rank:u32, offset:u32)->u32 {
    var remaining=linear; var address=offset;
    for(var axis=rank;axis>0u;axis-=1u) {
        let n=f64_meta[start+axis-1u];
        address+=(remaining%n)*f64_meta[start+rank+axis-1u]; remaining/=n;
    }
    return address;
}
