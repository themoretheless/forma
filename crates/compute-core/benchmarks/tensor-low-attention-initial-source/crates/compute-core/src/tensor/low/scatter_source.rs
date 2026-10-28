//! Preserve the shared scatter traversal and specialize packed reads/stores.
use crate::shaders;
pub(super) fn source(packed_output: bool) -> String {
    let mut traversal =
        shaders::TENSOR_SCATTER_WGSL.replace("alias Value = f32;", "alias Value = u32;");
    let first = traversal.find("fn fold(").expect("scatter fold hook");
    let last = traversal.find("@compute").expect("scatter entry");
    traversal.replace_range(
        first..last,
        include_str!("../../../shaders/tensor_low_scatter_store.wgsl"),
    );
    traversal = traversal
        .replace(
            "atomicStore(&output[destination], bitcast<u32>(updates[update_address]));",
            "replace_value(destination, update_bits(update_address));",
        )
        .replace(
            "accumulate(destination, updates[update_address]);",
            "accumulate(destination, update_bits(update_address));",
        );
    format!(
        "const PACKED_OUTPUT:bool={packed_output};\n{}\n{}\n{traversal}",
        include_str!("../../../shaders/low_codec.wgsl"),
        include_str!("../../../shaders/float_bits_order.wgsl")
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn packed_scatter_specializations_validate() {
        for packed in [false, true] {
            let source = super::source(packed);
            assert!(!source.contains("updates[update_address]"));
            assert!(!source.contains("atomicStore(&output[destination]"));
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
