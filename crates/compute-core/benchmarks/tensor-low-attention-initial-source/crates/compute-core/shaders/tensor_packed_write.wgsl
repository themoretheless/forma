// Specialized callers define OUTPUT_OFFSET and result_value(logical_index).
// A single invocation owns the complete physical word, including odd views.
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid:vec3<u32>) {
    let offset=p[OUTPUT_OFFSET];
    let word_count=(p[0]/2u)+((p[0]%2u+(offset&1u)+1u)/2u);
    var i=gid.x;
    while i<word_count {
        let word=offset/2u+i;
        var bits=output[word];
        for(var half=0u;half<2u;half++) {
            let address=word*2u+half;
            if address>=offset && address-offset<p[0] {
                let shift=half*16u;
                bits=(bits&~(65535u<<shift))|(result_value(address-offset)<<shift);
            }
        }
        output[word]=bits;
        if p[2]*256u>=word_count-i { break; }
        i+=p[2]*256u;
    }
}
