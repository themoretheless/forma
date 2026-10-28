// x and y are decoded f32 IEEE bits. Integer comparisons preserve all
// finite subnormals; NaNs are unordered and either signed zero is equal.
fn compare(x:Value,y:Value)->bool {
    if (x&0x7fffffffu)>0x7f800000u || (y&0x7fffffffu)>0x7f800000u {
        return p[3]==1u;
    }
    let equal=x==y || ((x|y)&0x7fffffffu)==0u;
    let less=!equal && float_bits_key(x)<float_bits_key(y);
    switch p[3] {
        case 0u: { return equal; }
        case 1u: { return !equal; }
        case 2u: { return less; }
        case 3u: { return less || equal; }
        case 4u: { return !less && !equal; }
        default: { return !less; }
    }
}
