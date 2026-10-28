//! Packed input hooks over the shared stats traversals. F32 shaders retain
//! their four-word summaries; low summaries add a per-row exact scaling flag.
use crate::shaders;
fn replace(source: &mut String, from: &str, to: &str) {
    assert!(
        source.contains(from),
        "statistics specialization hook missing: {from}"
    );
    *source = source.replace(from, to);
}
fn common(source: &str, reads_input: bool) -> String {
    let mut result = format!(
        "{}\n{}\n{}\n{source}",
        include_str!("../../../shaders/low_codec.wgsl"),
        include_str!("../../../shaders/float_bits_order.wgsl"),
        include_str!("../../../shaders/tensor_low_stats.wgsl")
    );
    if reads_input {
        replace(&mut result, "input: array<f32>", "input: array<u32>");
        result.push_str("\nfn low_read(address: u32) -> f32 {\nlet word = input[address / 2u];\nlet bits = (word >> (16u * (address % 2u))) & 0xffffu;\nreturn bitcast<f32>(low_decode(bits, params[low_stats_extra()]));\n}\n");
    }
    result
}
pub(super) fn small() -> String {
    let mut s = common(shaders::TENSOR_STATS_SMALL_WGSL, true);
    replace(
        &mut s,
        "input[params[2] + row_address.x + item_address.x]",
        "low_read(params[2] + row_address.x + item_address.x)",
    );
    replace(
        &mut s,
        "vec2<f32>(min(a.x, b.x), max(a.y, b.y))",
        "low_extrema(a, b)",
    );
    replace(
        &mut s,
        "extrema = scratch[0];",
        "extrema = scratch[0];\nlet lifted = low_needs_lift(extrema);\nx = low_coordinate(x, lifted);\nextrema = vec2<f32>(low_coordinate(extrema.x, lifted), low_coordinate(extrema.y, lifted));",
    );
    replace(
        &mut s,
        "bitcast<u32>(anchor + scale * mean)",
        "low_restore(bitcast<u32>(anchor + scale * mean), lifted, 64u)",
    );
    replace(
        &mut s,
        "variance[params[10] + row] = bits;",
        "variance[params[10] + row] = low_restore(bits, lifted, 128u);",
    );
    let start = s.find("                    var normalized = 0.0;").unwrap();
    let end = s[start..]
        .find("                    output[destination]")
        .unwrap()
        + start;
    s.replace_range(start..end, "                    let normalized = low_normalized(centered, scaled_variance, scale, bitcast<f32>(params[8]), lifted);\n");
    s
}
pub(super) fn reduce() -> String {
    let mut s = common(shaders::TENSOR_STATS_REDUCE_WGSL, true);
    replace(
        &mut s,
        "vec2<f32>(min(a.x, b.x), max(a.y, b.y))",
        "low_extrema(a, b)",
    );
    replace(
        &mut s,
        "input[base + decode(item, 13u + params[1] * 3u, params[4])]",
        "low_read(base + decode(item, 13u + params[1] * 3u, params[4]))",
    );
    replace(
        &mut s,
        "let row_state = state[row];",
        "let row_state = state[row * 2u];\nlet lifted = state[row * 2u + 1u].x != 0.0;",
    );
    replace(
        &mut s,
        "scaled_delta(x, row_state.x, row_state.y)",
        "scaled_delta(low_coordinate(x, lifted), row_state.x, row_state.y)",
    );
    s
}
pub(super) fn finish() -> String {
    let mut s = common(shaders::TENSOR_STATS_FINISH_WGSL, false);
    replace(
        &mut s,
        "vec2<f32>(min(value.x, next.x), max(value.y, next.y))",
        "low_extrema(value, next)",
    );
    replace(
        &mut s,
        "            var anchor = float_half(value.x)",
        "            let lifted = low_needs_lift(value);\n            value = vec2<f32>(low_coordinate(value.x, lifted), low_coordinate(value.y, lifted));\n            output[row * 2u + 1u] = vec4<f32>(select(0.0, 1.0, lifted), 0.0, 0.0, 0.0);\n            var anchor = float_half(value.x)",
    );
    replace(&mut s, "output[row]", "output[row * 2u]");
    replace(
        &mut s,
        "let old = previous[row];",
        "let old = previous[row * 2u];\n            output[row * 2u + 1u] = previous[row * 2u + 1u];",
    );
    s
}
pub(super) fn output() -> String {
    let mut s = common(shaders::TENSOR_STATS_OUTPUT_WGSL, true);
    replace(
        &mut s,
        "input[params[2] + row_address.x + item_address.x]",
        "low_read(params[2] + row_address.x + item_address.x)",
    );
    replace(
        &mut s,
        "let summary = state[row];",
        "let summary = state[row * 2u];\nlet lifted = state[row * 2u + 1u].x != 0.0;",
    );
    replace(
        &mut s,
        "bitcast<u32>(summary.x + summary.y * summary.z)",
        "low_restore(bitcast<u32>(summary.x + summary.y * summary.z), lifted, 64u)",
    );
    replace(
        &mut s,
        "variance[params[10] + row] = variance_bits;",
        "variance[params[10] + row] = low_restore(variance_bits, lifted, 128u);",
    );
    let start = s.find("                    var normalized = 0.0;").unwrap();
    let end = s[start..]
        .find("                    bits = bitcast<u32>(normalized);")
        .unwrap()
        + start;
    s.replace_range(start..end, "                    var centered = 0.0;\n                    if summary.y > 0.0 { centered = scaled_delta(low_coordinate(x, lifted), summary.x, summary.y) - summary.z; }\n                    let normalized = low_normalized(centered, summary.w, summary.y, bitcast<f32>(params[8]), lifted);\n");
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn packed_statistics_specializations_validate() {
        for (name, source) in [
            ("small", super::small()),
            ("reduce", super::reduce()),
            ("finish", super::finish()),
            ("output", super::output()),
        ] {
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
}
