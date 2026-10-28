// Storage specialization of the shared scatter traversal. Update payloads
// stay packed; only the destination accumulator may occupy one f32 word.
fn update_bits(address:u32)->u32 {
    return (updates[address/2u]>>((address&1u)*16u))&65535u;
}
fn scatter_dtype()->u32 { return p[9u+p[1]*5u]; }
fn replace_value(address:u32,update:u32) {
    if !PACKED_OUTPUT {
        atomicStore(&output[address],low_decode(update,scatter_dtype()));
        return;
    }
    let shift=(address&1u)*16u;
    let mask=65535u<<shift;
    var old=atomicLoad(&output[address/2u]);
    loop {
        let attempt=atomicCompareExchangeWeak(&output[address/2u],old,(old&~mask)|(update<<shift));
        if attempt.exchanged { break; }
        old=attempt.old_value;
    }
}
fn fold_bits(a:u32,b:u32)->u32 {
    switch p[8] {
        case 1u: { return bitcast<u32>(bitcast<f32>(a)+bitcast<f32>(b)); }
        case 2u: { return bitcast<u32>(bitcast<f32>(a)*bitcast<f32>(b)); }
        case 3u: { return float_bits_min(a,b); }
        default: { return float_bits_max(a,b); }
    }
}
fn accumulate(address:u32,update:u32) {
    let decoded=low_decode(update,scatter_dtype());
    if PACKED_OUTPUT {
        // Host records only Min/Max here. Arithmetic folds use f32 output and
        // a separate final cast after every duplicate update has accumulated.
        let shift=(address&1u)*16u;
        let mask=65535u<<shift;
        var old=atomicLoad(&output[address/2u]);
        loop {
            let original=(old>>shift)&65535u;
            let bits=low_decode(original,scatter_dtype());
            let choose_update=select(float_bits_key(decoded)>float_bits_key(bits),float_bits_key(decoded)<float_bits_key(bits),p[8]==3u);
            let result=select(original,update,choose_update);
            let attempt=atomicCompareExchangeWeak(&output[address/2u],old,(old&~mask)|(result<<shift));
            if attempt.exchanged { break; }
            old=attempt.old_value;
        }
    } else {
        var old=atomicLoad(&output[address]);
        loop {
            let attempt=atomicCompareExchangeWeak(&output[address],old,fold_bits(old,decoded));
            if attempt.exchanged { break; }
            old=attempt.old_value;
        }
    }
}
