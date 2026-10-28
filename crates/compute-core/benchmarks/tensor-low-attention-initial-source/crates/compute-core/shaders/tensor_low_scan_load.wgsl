// First scan pass adds rank, inputOffset,dtype after the six shared words,
// followed by (dim,stride) in the row-major permutation ending in scan axis.
fn low_scan_load(linear:u32)->f32 {
    var remaining=linear;
    var address=p[7];
    for(var axis=p[6];axis>0u;axis--) {
        let descriptor=9u+(axis-1u)*2u;
        address+=(remaining%p[descriptor])*p[descriptor+1u];
        remaining/=p[descriptor];
    }
    let bits=(input[address/2u]>>((address&1u)*16u))&65535u;
    return bitcast<f32>(low_decode(bits,p[8]));
}
