// Binary64 words are (low, high). Arithmetic uses only integer WGSL operations.
// Round to nearest, ties to even. No exception flags or dynamic rounding mode.
alias D64 = vec2<u32>;
const D64_ONE: D64 = D64(0x00000000u, 0x3ff00000u);
fn d64_zero(a: D64) -> bool { return (a.x | (a.y & 0x7fffffffu)) == 0u; }
fn d64_nan(a: D64) -> bool { return (a.y & 0x7ff00000u) == 0x7ff00000u && ((a.y & 0xfffffu) | a.x) != 0u; }
fn d64_inf(a: D64) -> bool { return (a.y & 0x7fffffffu) == 0x7ff00000u && a.x == 0u; }
fn d64_qnan() -> D64 { return D64(0u, 0x7ff80000u); }
fn d64_quiet(a: D64) -> D64 { return D64(a.x, a.y | 0x80000u); }
fn d64_neg(a: D64) -> D64 { return D64(a.x, a.y ^ 0x80000000u); }
fn d64_abs(a: D64) -> D64 { return D64(a.x, a.y & 0x7fffffffu); }
fn w64_lt(a: D64, b: D64) -> bool { return a.y < b.y || (a.y == b.y && a.x < b.x); }
fn w64_add(a: D64, b: D64) -> D64 {
    let lo = a.x + b.x;
    return D64(lo, a.y + b.y + u32(lo < a.x));
}
fn w64_sub(a: D64, b: D64) -> D64 { return D64(a.x-b.x, a.y-b.y-u32(a.x < b.x)); }
fn w64_shl(a: D64, n: u32) -> D64 {
    if n == 0u { return a; }
    if n >= 64u { return D64(0u); }
    if n >= 32u { return D64(0u, a.x << (n-32u)); }
    return D64(a.x << n, (a.y << n) | (a.x >> (32u-n)));
}
fn w64_shr(a: D64, n: u32) -> D64 {
    if n == 0u { return a; }
    if n >= 64u { return D64(0u); }
    if n >= 32u { return D64(a.y >> (n-32u), 0u); }
    return D64((a.x >> n) | (a.y << (32u-n)), a.y >> n);
}
fn w64_jam(a: D64, n: u32) -> D64 {
    let shifted = w64_shr(a,n);
    return D64(shifted.x | u32(any(w64_shl(shifted,n) != a)), shifted.y);
}
fn w64_bit(a: D64, n: u32) -> u32 {
    if n >= 64u { return 0u; }
    if n >= 32u { return (a.y >> (n-32u)) & 1u; }
    return (a.x >> n) & 1u;
}
struct D64Parts { sig: D64, exponent: i32 }
// Call only for finite, nonzero inputs. Normalized leading bit is bit 52.
fn d64_parts(a: D64) -> D64Parts {
    let field = (a.y >> 20u) & 0x7ffu;
    let sig = D64(a.x, a.y & 0xfffffu);
    if field != 0u {
        return D64Parts(D64(sig.x, sig.y | 0x100000u), i32(field) - 1023);
    }
    let shift = select(countLeadingZeros(sig.x) + 21u, countLeadingZeros(sig.y) - 11u, sig.y != 0u);
    return D64Parts(w64_shl(sig, shift), -1022 - i32(shift));
}
// sig has a leading bit at 55 and three low guard/round/sticky bits.
fn d64_pack(sign: u32, exponent_in: i32, sig_in: D64) -> D64 {
    var exponent = exponent_in;
    var sig = sig_in;
    if exponent < -1022 {
        sig = w64_jam(sig, u32(-1022-exponent));
        exponent = -1022;
    }
    let tail = sig.x & 7u;
    var rounded = w64_shr(sig,3u);
    if tail > 4u || (tail == 4u && (rounded.x & 1u) != 0u) {
        rounded = w64_add(rounded,D64(1u,0u));
    }
    if (rounded.y & 0x200000u) != 0u {
        rounded = w64_shr(rounded,1u);
        exponent += 1;
    }
    if exponent > 1023 { return D64(0u, sign | 0x7ff00000u); }
    var field = 0u;
    if (rounded.y & 0x100000u) != 0u { field = u32(exponent+1023) << 20u; }
    return D64(rounded.x, sign | field | (rounded.y & 0xfffffu));
}
fn d64_add(a: D64, b: D64) -> D64 {
    if d64_nan(a) { return d64_quiet(a); }
    if d64_nan(b) { return d64_quiet(b); }
    if d64_inf(a) {
        if d64_inf(b) && ((a.y ^ b.y) & 0x80000000u) != 0u { return d64_qnan(); }
        return a;
    }
    if d64_inf(b) { return b; }
    if d64_zero(a) && d64_zero(b) { return D64(0u, a.y & b.y & 0x80000000u); }
    if d64_zero(a) { return b; }
    if d64_zero(b) { return a; }
    var large = a;
    var small = b;
    if w64_lt(d64_abs(a),d64_abs(b)) { large=b; small=a; }
    let p = d64_parts(large);
    let q = d64_parts(small);
    var exponent = p.exponent;
    let sign = large.y & 0x80000000u;
    let x = w64_shl(p.sig,3u);
    let y = w64_jam(w64_shl(q.sig,3u),u32(p.exponent-q.exponent));
    var sig: D64;
    if ((large.y ^ small.y) & 0x80000000u) == 0u {
        sig = w64_add(x,y);
        if (sig.y & 0x1000000u) != 0u { sig=w64_jam(sig,1u); exponent+=1; }
    } else {
        sig = w64_sub(x,y);
        if all(sig == D64(0u)) { return D64(0u); }
        let shift = select(countLeadingZeros(sig.x) + 24u, countLeadingZeros(sig.y) - 8u, sig.y != 0u);
        sig = w64_shl(sig, shift);
        exponent -= i32(shift);
    }
    return d64_pack(sign,exponent,sig);
}
fn d64_sub(a: D64, b: D64) -> D64 { return d64_add(a,d64_neg(b)); }
fn w32_mul(a: u32, b: u32) -> D64 {
    let a0 = a & 65535u; let a1 = a >> 16u;
    let b0 = b & 65535u; let b1 = b >> 16u;
    let p0 = a0 * b0; let p1 = a0 * b1; let p2 = a1 * b0; let p3 = a1 * b1;
    let mid = (p0 >> 16u) + (p1 & 65535u) + (p2 & 65535u);
    return D64((p0 & 65535u) | (mid << 16u), p3 + (p1 >> 16u) + (p2 >> 16u) + (mid >> 16u));
}
fn d64_mul(a: D64, b: D64) -> D64 {
    if d64_nan(a) { return d64_quiet(a); }
    if d64_nan(b) { return d64_quiet(b); }
    let sign = (a.y ^ b.y) & 0x80000000u;
    if d64_inf(a) || d64_inf(b) {
        if d64_zero(a) || d64_zero(b) { return d64_qnan(); }
        return D64(0u,sign | 0x7ff00000u);
    }
    if d64_zero(a) || d64_zero(b) { return D64(0u,sign); }
    let p = d64_parts(a);
    let q = d64_parts(b);
    let ll = w32_mul(p.sig.x, q.sig.x);
    let lh = w32_mul(p.sig.x, q.sig.y);
    let hl = w32_mul(p.sig.y, q.sig.x);
    let hh = w32_mul(p.sig.y, q.sig.y);
    let mid = w64_add(w64_add(D64(ll.y, 0u), lh), hl);
    let top = w64_add(D64(mid.y, 0u), hh);
    let high = (top.y >> 9u) & 1u;
    let s = 17u + high;
    let sticky = u32((ll.x | (mid.x & ((1u << s) - 1u))) != 0u);
    let sig = D64(((mid.x >> s) | (top.x << (32u - s))) | sticky, (top.x >> s) | (top.y << (32u - s)));
    return d64_pack(sign, p.exponent + q.exponent + i32(high), sig);
}
fn d64_div(a: D64, b: D64) -> D64 {
    if d64_nan(a) { return d64_quiet(a); }
    if d64_nan(b) { return d64_quiet(b); }
    let sign=(a.y ^ b.y) & 0x80000000u;
    if (d64_inf(a) && d64_inf(b)) || (d64_zero(a) && d64_zero(b)) { return d64_qnan(); }
    if d64_inf(a) || d64_zero(b) { return D64(0u,sign | 0x7ff00000u); }
    if d64_zero(a) || d64_inf(b) { return D64(0u,sign); }
    let p=d64_parts(a);
    let q=d64_parts(b);
    var remainder=p.sig;
    var exponent=p.exponent-q.exponent;
    if w64_lt(remainder,q.sig) { remainder=w64_shl(remainder,1u); exponent-=1; }
    var sig=D64(0u);
    for(var i=0u;i<56u;i+=1u) {
        sig=w64_shl(sig,1u);
        if !w64_lt(remainder,q.sig) { remainder=w64_sub(remainder,q.sig); sig.x|=1u; }
        if i != 55u { remainder=w64_shl(remainder,1u); }
    }
    sig.x |= u32(any(remainder != D64(0u)));
    return d64_pack(sign,exponent,sig);
}
fn d64_sqrt(a: D64) -> D64 {
    if d64_nan(a) { return d64_quiet(a); }
    if d64_zero(a) { return a; }
    if (a.y & 0x80000000u) != 0u { return d64_qnan(); }
    if d64_inf(a) { return a; }
    let p=d64_parts(a);
    let odd=u32(p.exponent & 1);
    let mantissa=w64_shl(p.sig,odd);
    let exponent=(p.exponent-i32(odd))/2;
    var remainder=D64(0u);
    var root=D64(0u);
    for(var step=0u;step<56u;step+=1u) {
        let pos=110u-2u*step;
        var pair=0u;
        if pos>=58u { pair=w64_bit(mantissa,pos-58u) | (w64_bit(mantissa,pos-57u)<<1u); }
        remainder=w64_shl(remainder,2u);
        remainder.x |= pair;
        let trial=w64_shl(root,2u) | D64(1u,0u);
        root=w64_shl(root,1u);
        if !w64_lt(remainder,trial) { remainder=w64_sub(remainder,trial); root.x|=1u; }
    }
    root.x |= u32(any(remainder != D64(0u)));
    return d64_pack(0u,exponent,root);
}
fn d64_eq(a: D64,b: D64)->bool {
    return !d64_nan(a) && !d64_nan(b) && (all(a==b) || (d64_zero(a) && d64_zero(b)));
}
fn d64_lt(a: D64,b: D64)->bool {
    if d64_nan(a) || d64_nan(b) || (d64_zero(a) && d64_zero(b)) { return false; }
    let sa=a.y>>31u; let sb=b.y>>31u;
    if sa != sb { return sa != 0u; }
    if sa != 0u { return w64_lt(d64_abs(b),d64_abs(a)); }
    return w64_lt(a,b);
}
