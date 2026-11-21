var<workgroup> f64_shared: array<D64, 256>;
fn d64_min(a: D64, b: D64) -> D64 {
    var result = select(b, a, d64_lt(a, b));
    if d64_nan(a) { result = d64_quiet(a); }
    else if d64_nan(b) { result = d64_quiet(b); }
    else if d64_zero(a) && d64_zero(b) { result = D64(0u, (a.y | b.y) & 0x80000000u); }
    return result;
}
fn d64_max(a: D64, b: D64) -> D64 {
    var result = select(a, b, d64_lt(a, b));
    if d64_nan(a) { result = d64_quiet(a); }
    else if d64_nan(b) { result = d64_quiet(b); }
    else if d64_zero(a) && d64_zero(b) { result = D64(0u, a.y & b.y & 0x80000000u); }
    return result;
}
fn d64_binary(a: D64, b: D64, op: u32) -> D64 {
    var result = d64_max(a, b);
    if op == 0u { result = d64_add(a, b); }
    else if op == 1u { result = d64_sub(a, b); }
    else if op == 2u { result = d64_mul(a, b); }
    else if op == 3u { result = d64_div(a, b); }
    else if op == 4u { result = d64_min(a, b); }
    return result;
}
fn d64_unary(a: D64, op: u32) -> D64 {
    var result = a;
    if op == 0u { result = d64_neg(a); }
    else if op == 1u { result = d64_abs(a); }
    else if op == 2u { result = d64_mul(a, a); }
    else if op == 3u { result = d64_sqrt(a); }
    else if op == 4u { result = d64_div(D64_ONE, a); }
    return result;
}
fn d64_identity(op: u32) -> D64 {
    var result = D64(0u);
    if op == 1u { result = D64_ONE; }
    else if op == 2u { result = D64(0u, 0x7ff00000u); }
    else if op == 3u { result = D64(0u, 0xfff00000u); }
    return result;
}
fn d64_fold(a: D64, b: D64, op: u32) -> D64 {
    var result = d64_add(a, b);
    if op == 1u { result = d64_mul(a, b); }
    else if op == 2u { result = d64_min(a, b); }
    else if op == 3u { result = d64_max(a, b); }
    return result;
}
@compute @workgroup_size(WG)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) lane: u32, @builtin(workgroup_id) group: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
    let mode = f64_meta[0]; let op = f64_meta[1]; let count = f64_meta[2];
    let ar = f64_meta[3]; let br = f64_meta[4]; let ao = f64_meta[5]; let bo = f64_meta[6];
    let inner = f64_meta[7]; let rows = f64_meta[8]; let cols = f64_meta[9];
    let bm = 10u + 2u * ar;
    if mode == 2u {
        for (var i = group.x; i < count; i += groups.x) {
            let base = d64_index(i, 10u, ar, ao);
            var sum = d64_identity(op);
            for (var r = lane; r < inner; r += WG) { sum = d64_fold(sum, f64_a[base + d64_index(r, bm, br, 0u)], op); }
            f64_shared[lane] = sum;
            workgroupBarrier();
            for (var width = WG / 2u; width > 0u; width /= 2u) {
                if lane < width { f64_shared[lane] = d64_fold(f64_shared[lane], f64_shared[lane + width], op); }
                workgroupBarrier();
            }
            if lane == 0u { f64_output[i] = f64_shared[0]; }
            // Wait until every lane finishes the shared reduction before reusing f64_shared.
            workgroupBarrier();
        }
    } else {
        for (var i = id.x; i < count; i += groups.x * WG) {
            if mode == 3u {
                let row = (i / cols) % rows; let batch = i / cols / rows; let col = i % cols;
                var sum = D64(0u);
                for (var r = 0u; r < inner; r += 1u) {
                    let a = f64_a[d64_index((batch * rows + row) * inner + r, 10u, ar, ao)];
                    let b = f64_b[d64_index((batch * inner + r) * cols + col, bm, br, bo)];
                    sum = d64_add(sum, d64_mul(a, b));
                }
                f64_output[i] = sum;
            } else {
                let a = f64_a[d64_index(i, 10u, ar, ao)];
                if mode == 0u { f64_output[i] = d64_unary(a, op); }
                else { f64_output[i] = d64_binary(a, f64_b[d64_index(i, bm, br, bo)], op); }
            }
        }
    }
}
