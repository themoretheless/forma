fn d64_from_i32(value:i32)->D64 {
    if value==0 { return D64(0u); }
    var mag=u32(value); var sign=0u;
    if value<0 { mag=u32(-(value+1))+1u; sign=0x80000000u; }
    let top=31u-countLeadingZeros(mag);
    return d64_pack(sign,i32(top),w64_shl(D64(mag,0u),55u-top));
}
// Caller bounds the finite value to the signed i32 range.
fn d64_trunc_i32(value:D64)->i32 {
    if d64_zero(value) { return 0; }
    let parts=d64_parts(value);
    if parts.exponent<0 { return 0; }
    let mag=w64_shr(parts.sig,u32(52-parts.exponent)).x;
    if (value.y & 0x80000000u)!=0u { return -i32(mag); }
    return i32(mag);
}
fn d64_exp(value:D64)->D64 {
    if d64_nan(value) { return d64_quiet(value); }
    if d64_lt(d64_from_i32(1024),value) { return D64(0u,0x7ff00000u); }
    if d64_lt(value,d64_from_i32(-2048)) { return D64(0u); }
    let k=d64_trunc_i32(d64_div(value,D64_LN2));
    let r=d64_sub(d64_sub(value,d64_mul(d64_from_i32(k),D64_LN2_HI)),d64_mul(d64_from_i32(k),D64_LN2_LO));
    let p=d64_parts(d64_exp_poly(r));
    return d64_pack(0u,p.exponent+k,w64_shl(p.sig,3u));
}
fn d64_log(value:D64)->D64 {
    if d64_nan(value) { return d64_quiet(value); }
    if d64_zero(value) { return D64(0u,0xfff00000u); }
    if (value.y & 0x80000000u)!=0u { return d64_qnan(); }
    if d64_inf(value) { return value; }
    let p=d64_parts(value);
    var exponent=p.exponent;
    var mantissa=D64(p.sig.x,(p.sig.y & 0xfffffu) | 0x3ff00000u);
    if d64_lt(D64_SQRT2,mantissa) { mantissa.y-=0x100000u; exponent+=1; }
    let z=d64_div(d64_sub(mantissa,D64_ONE),d64_add(mantissa,D64_ONE));
    let small=d64_mul(d64_mul(D64_TWO,z),d64_log_poly(d64_mul(z,z)));
    let k=d64_from_i32(exponent);
    return d64_add(d64_mul(k,D64_LN2_HI),d64_add(small,d64_mul(k,D64_LN2_LO)));
}
struct D64Angle { value: D64, quadrant:u32 }
fn d64_angle(absolute:D64)->D64Angle {
    if d64_lt(absolute,D64_HALF) { return D64Angle(absolute,0u); }
    let p=d64_parts(absolute);
    let mant=array<u32,4>(p.sig.x & 65535u,p.sig.x >> 16u,p.sig.y & 65535u,p.sig.y >> 16u);
    var product:array<u32,84>;
    for(var i=0u;i<4u;i+=1u) {
        var carry=0u;
        for(var j=0u;j<80u;j+=1u) {
            let term=mant[i]*D64_TWO_OVER_PI[j]+product[i+j]+carry;
            product[i+j]=term & 65535u; carry=term>>16u;
        }
        product[i+80u]=carry;
    }
    let point=u32(1332-p.exponent);
    var quadrant=((product[point/16u] >> (point%16u)) & 1u) | (((product[(point+1u)/16u] >> ((point+1u)%16u)) & 1u)<<1u);
    let negative=((product[(point-1u)/16u] >> ((point-1u)%16u)) & 1u)!=0u;
    // Retain the fractional numerator, then complement within its exact width
    // when choosing the nearest multiple of pi/2.
    for(var i=0u;i<84u;i+=1u) {
        if i>point/16u { product[i]=0u; }
        else if i==point/16u { product[i]&=(1u << (point%16u))-1u; }
    }
    if negative {
        quadrant=(quadrant+1u)&3u;
        var borrow=0u;
        for(var i=0u;i<84u;i+=1u) {
            var digit=0u;
            if i==point/16u { digit=1u << (point%16u); }
            let sub=product[i]+borrow;
            product[i]=(digit-sub)&65535u;
            borrow=u32(digit<sub);
        }
    }
    var leading=i32(point)-1;
    loop {
        if leading<0 { return D64Angle(D64(0u),quadrant); }
        if ((product[u32(leading)/16u] >> (u32(leading)%16u)) & 1u)!=0u { break; }
        leading-=1;
    }
    var sig=D64(0u);
    for(var i=0u;i<56u;i+=1u) {
        sig=w64_shl(sig,1u);
        let pos=leading-i32(i);
        if pos>=0 { sig.x |= (product[u32(pos)/16u] >> (u32(pos)%16u)) & 1u; }
    }
    for(var i=0;i<leading-55;i+=1) { sig.x |= (product[u32(i)/16u] >> (u32(i)%16u)) & 1u; }
    let fraction=d64_pack(select(0u,0x80000000u,negative),leading-i32(point),sig);
    let reduced=d64_add(d64_mul(fraction,D64_PIO2),d64_mul(fraction,D64_PIO2_LO));
    return D64Angle(reduced,quadrant);
}
fn d64_sincos(value:D64, cosine:bool)->D64 {
    if d64_nan(value) { return d64_quiet(value); }
    if d64_inf(value) { return d64_qnan(); }
    if d64_zero(value) { return select(value,D64_ONE,cosine); }
    let angle=d64_angle(d64_abs(value));
    let squared=d64_mul(angle.value,angle.value);
    let sine=d64_mul(angle.value,d64_sin_poly(squared));
    let cosvalue=d64_cos_poly(squared);
    let quadrant=(angle.quadrant+u32(cosine))&3u;
    var result=select(sine,cosvalue,(quadrant&1u)!=0u);
    if quadrant>=2u { result=d64_neg(result); }
    if !cosine && (value.y & 0x80000000u)!=0u { result=d64_neg(result); }
    return result;
}
