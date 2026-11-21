// Binary64 division and square root appended to arithmetic_base.wgsl.
// --- DIV ---
fn d64_div(a: D64, b: D64) -> D64 {
    let sign = (a.y ^ b.y) & 0x80000000u;
    var result = D64(0u, sign);
    var done = false;
    if d64_nan(a) { result = d64_quiet(a); done = true; }
    else if d64_nan(b) { result = d64_quiet(b); done = true; }
    else if (d64_inf(a) && d64_inf(b)) || (d64_zero(a) && d64_zero(b)) { result = d64_qnan(); done = true; }
    else if d64_inf(a) || d64_zero(b) { result = D64(0u, sign | 0x7ff00000u); done = true; }
    else if d64_zero(a) || d64_inf(b) { done = true; }
    if !done {
        let p = d64_parts(a);
        let q = d64_parts(b);
        var remainder = p.sig;
        var exponent = p.exponent - q.exponent;
        if w64_lt(remainder, q.sig) { remainder = w64_shl(remainder, 1u); exponent -= 1; }
        var sig = D64(0u);
        for (var i = 0u; i < 56u; i += 1u) {
            sig = w64_shl(sig, 1u);
            if !w64_lt(remainder, q.sig) { remainder = w64_sub(remainder, q.sig); sig.x |= 1u; }
            if i != 55u { remainder = w64_shl(remainder, 1u); }
        }
        sig.x |= u32(any(remainder != D64(0u)));
        result = d64_pack(sign, exponent, sig);
    }
    return result;
}
// --- SQRT ---
fn d64_sqrt(a: D64) -> D64 {
    var result = a;
    var done = false;
    if d64_nan(a) { result = d64_quiet(a); done = true; }
    else if d64_zero(a) { done = true; }
    else if (a.y & 0x80000000u) != 0u { result = d64_qnan(); done = true; }
    else if d64_inf(a) { done = true; }
    if !done {
        let p = d64_parts(a);
        let odd = u32(p.exponent & 1);
        let mantissa = w64_shl(p.sig, odd);
        let exponent = (p.exponent - i32(odd)) / 2;
        var remainder = D64(0u);
        var root = D64(0u);
        for (var step = 0u; step < 56u; step += 1u) {
            let pos = 110u - 2u * step;
            var pair = 0u;
            if pos >= 58u { pair = w64_bit(mantissa, pos - 58u) | (w64_bit(mantissa, pos - 57u) << 1u); }
            remainder = w64_shl(remainder, 2u);
            remainder.x |= pair;
            let trial = w64_shl(root, 2u) | D64(1u, 0u);
            root = w64_shl(root, 1u);
            if !w64_lt(remainder, trial) { remainder = w64_sub(remainder, trial); root.x |= 1u; }
        }
        root.x |= u32(any(remainder != D64(0u)));
        result = d64_pack(0u, exponent, root);
    }
    return result;
}
