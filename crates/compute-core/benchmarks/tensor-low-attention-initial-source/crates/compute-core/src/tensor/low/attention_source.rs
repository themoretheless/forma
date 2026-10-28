//! Only operand loads differ from the shared tiled/split attention shader.
fn replace(source: &mut String, from: &str, to: &str) {
    assert!(
        source.contains(from),
        "attention specialization hook missing: {from}"
    );
    *source = source.replace(from, to);
}
pub(super) fn source() -> String {
    let mut source = crate::shaders::TENSOR_ATTENTION_WGSL.to_owned();
    for name in ["query", "key", "value"] {
        replace(
            &mut source,
            &format!("{name}: array<f32>"),
            &format!("{name}: array<u32>"),
        );
    }
    replace(
        &mut source,
        "query[qbase + depth * params[17]]\n                            * key[kbase + key_index * params[19] + depth * params[20]]",
        "low_attention_product(low_query_bits(qbase + depth * params[17]), low_key_bits(kbase + key_index * params[19] + depth * params[20]))",
    );
    replace(
        &mut source,
        "value[vbase + (start + item) * params[22] + channel * params[23]]",
        "low_value(vbase + (start + item) * params[22] + channel * params[23])",
    );
    format!(
        "{}\n{}\n{source}",
        include_str!("../../../shaders/low_codec.wgsl"),
        include_str!("../../../shaders/tensor_low_attention_load.wgsl")
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn packed_attention_specialization_validates() {
        let source = super::source();
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        assert!(!source.contains("query: array<f32>"));
    }
}
