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
fn w32_shr_pair(lo:u32, hi:u32, s:u32)->u32 {
    if s==0u { return lo; }
    return (lo>>s)|(hi<<(32u-s));
}
fn w32_shl_pair(hi:u32, lo:u32, s:u32)->u32 {
    if s==0u { return hi; }
    return (hi<<s)|(lo>>(32u-s));
}
fn d64_mul_sig_word(sig:D64, w:u32, carry:D64)->vec3<u32> {
    let lo=w32_mul(sig.x,w);
    let hi=w32_mul(sig.y,w);
    let t0=w64_add(D64(lo.x,0u),D64(carry.x,0u));
    let carry_out=w64_add(w64_add(D64(lo.y,0u),hi),D64(carry.y+t0.y,0u));
    return vec3<u32>(t0.x,carry_out.x,carry_out.y);
}
struct D64Angle { value: D64, quadrant:u32 }
fn d64_angle(absolute:D64)->D64Angle {
    if d64_lt(absolute,D64_HALF) { return D64Angle(absolute,0u); }
    let p=d64_parts(absolute);
    let point=u32(1332-p.exponent);
    let pw=point/32u;
    let pb=point%32u;
    var carry=D64(0u);
    var w_m4=0u; var w_m3=0u; var w_m2=0u; var w_m1=0u; var w_0=0u; var w_p1=0u;
    for(var k=0u;k<8u;k+=1u) {
        let step=d64_mul_sig_word(p.sig,d64_two_over_pi_word(pw-6u+k),carry);
        carry=D64(step.y,step.z);
        w_m4=w_m3; w_m3=w_m2; w_m2=w_m1; w_m1=w_0; w_0=w_p1; w_p1=step.x;
    }
    var quadrant=w32_shr_pair(w_0,w_p1,pb)&3u;
    var f3=w32_shr_pair(w_m1,w_0,pb);
    var f2=w32_shr_pair(w_m2,w_m1,pb);
    var f1=w32_shr_pair(w_m3,w_m2,pb);
    var f0=w32_shr_pair(w_m4,w_m3,pb);
    let negative=(f3&0x80000000u)!=0u;
    if negative {
        quadrant=(quadrant+1u)&3u;
        f0=(~f0)+1u;
        let c0=u32(f0==0u);
        f1=(~f1)+c0;
        let c1=u32(f1==0u && c0!=0u);
        f2=(~f2)+c1;
        let c2=u32(f2==0u && c1!=0u);
        f3=(~f3)+c2;
    }
    if (f3|f2|f1|f0)==0u { return D64Angle(D64(0u),quadrant); }
    var lz=0u;
    var w_top=f3;
    var w_mid=f2;
    var w_low=f1;
    var rem32=f0;
    if w_top==0u { lz+=32u; w_top=w_mid; w_mid=w_low; w_low=rem32; rem32=0u; }
    if w_top==0u { lz+=32u; w_top=w_mid; w_mid=w_low; w_low=0u; }
    if w_top==0u { lz+=32u; w_top=w_mid; w_mid=0u; }
    let bit_lz=countLeadingZeros(w_top);
    lz+=bit_lz;
    let hi32=w32_shl_pair(w_top,w_mid,bit_lz);
    let lo32=w32_shl_pair(w_mid,w_low,bit_lz);
    let tail=(w_low<<bit_lz)|rem32;
    let sig=D64(((lo32>>8u)|(hi32<<24u))|u32(((lo32&255u)|tail)!=0u),hi32>>8u);
    let fraction=d64_pack(select(0u,0x80000000u,negative),-1-i32(lz),sig);
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
