//! Only operand loads differ from the shared tiled/split attention shader.
fn replace(source: &mut String, from: &str, to: &str) {
    assert!(
        source.contains(from),
        "attention specialization hook missing: {from}"
    );
    *source = source.replace(from, to);
}
pub(super) fn source(dtype: tensor_core::LowDtype) -> String {
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
    let mut loader = include_str!("../../../shaders/tensor_low_attention_load.wgsl").to_owned();
    if dtype == tensor_core::LowDtype::F16 {
        // Every finite nonzero f16 decodes to a normal f32, so no input-flush
        // protection is needed. Keep the exact integer decoder unchanged.
        loader.truncate(
            loader
                .find("fn low_attention_product")
                .expect("product hook"),
        );
        loader.push_str("fn low_attention_product(a: u32, b: u32) -> f32 { return bitcast<f32>(a) * bitcast<f32>(b); }\n");
    }
    format!(
        "const LOW_DTYPE: u32 = {}u;\n{}\n{loader}\n{source}",
        dtype as u32,
        include_str!("../../../shaders/low_codec.wgsl"),
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn packed_attention_specialization_validates() {
        for dtype in [tensor_core::LowDtype::F16, tensor_core::LowDtype::Bf16] {
            let source = super::source(dtype);
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{dtype:?}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
            assert!(!source.contains("query: array<f32>"));
            if dtype == tensor_core::LowDtype::F16 {
                assert!(!source.contains("A BF16 subnormal times a large"));
            }
        }
    }
}
