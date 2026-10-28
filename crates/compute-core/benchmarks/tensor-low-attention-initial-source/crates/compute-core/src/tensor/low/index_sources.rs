//! Specialize the shared tensor traversal; only storage loads/stores differ.
use crate::shaders;
fn packed(buffer: &str, address: &str) -> String {
    format!("(({buffer}[({address})/2u]>>((({address})&1u)*16u))&65535u)")
}
fn words(source: &str) -> String {
    source.replace("alias Value = f32;", "alias Value = u32;")
}
fn packed_writer(source: String, output_offset: u32) -> String {
    let traversal = source.split_once("@compute").expect("scalar writer").0;
    format!(
        "{traversal}\nconst OUTPUT_OFFSET:u32={output_offset}u;\n{}",
        include_str!("../../../shaders/tensor_packed_write.wgsl")
    )
}
pub(super) fn compare() -> String {
    let mut source = words(shaders::TENSOR_COMPARE_WGSL);
    let first = source.find("fn compare(").expect("comparison hook");
    let last = source.find("@compute").expect("comparison entry");
    source.replace_range(
        first..last,
        include_str!("../../../shaders/low_ieee_compare.wgsl"),
    );
    for (buffer, address) in [("a", "aa"), ("b", "bb")] {
        source = source.replace(
            &format!("{buffer}[{address}]"),
            &format!("low_decode({},p[7u+p[1]*3u])", packed(buffer, address)),
        );
    }
    format!(
        "{}\n{}\n{source}",
        include_str!("../../../shaders/low_codec.wgsl"),
        include_str!("../../../shaders/float_bits_order.wgsl")
    )
}
pub(super) fn select() -> String {
    let source = words(shaders::TENSOR_SELECT_WGSL)
        .replace("a[aa]", &packed("a", "aa"))
        .replace("b[bb]", &packed("b", "bb"));
    packed_writer(source, 6)
}
pub(super) fn gather() -> String {
    let source = words(shaders::TENSOR_GATHER_WGSL).replace(
        "input[source+selected*p[7]]",
        &packed("input", "source+selected*p[7]"),
    );
    packed_writer(source, 5)
}
pub(super) fn compact() -> String {
    let source = words(shaders::TENSOR_COMPACT_WGSL)
        .replace("output:array<Value>", "output:array<atomic<u32>>")
        .replace(
            "output[p[5]+position]=input[address];",
            &format!(
                "packed_store(p[5]+position,{});",
                packed("input", "address")
            ),
        )
        .replace("output[p[5]+i]=Value(0);", "packed_store(p[5]+i,0u);");
    // Prefix-selected positions and zero-tail positions never overlap, but
    // neighboring positions can share a physical word. CAS preserves both.
    format!(
        "{source}\n{}",
        r#"
fn packed_store(address:u32,value:u32) {
    let shift=(address&1u)*16u;
    let mask=65535u<<shift;
    var old=atomicLoad(&output[address/2u]);
    loop {
        let result=atomicCompareExchangeWeak(&output[address/2u],old,(old&~mask)|(value<<shift));
        if result.exchanged { break; }
        old=result.old_value;
    }
}
"#
    )
}
pub(super) fn scan() -> String {
    let source = shaders::TENSOR_SCAN_BLOCKS_WGSL
        .replace("input:array<Value>", "input:array<u32>")
        .replace(
            "input[row*p[1]+physical]",
            "low_scan_load(row*p[1]+physical)",
        );
    format!(
        "{}\n{}\n{source}",
        include_str!("../../../shaders/low_codec.wgsl"),
        include_str!("../../../shaders/tensor_low_scan_load.wgsl")
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn packed_index_specializations_validate() {
        for (name, source) in [
            ("compare", super::compare()),
            ("select", super::select()),
            ("gather", super::gather()),
            ("compact", super::compact()),
            ("scan", super::scan()),
        ] {
            assert!(!source.contains("alias Value = f32;") || name == "scan");
            assert!(!source.contains("input[source+selected*p[7]]"));
            assert!(!source.contains("input[row*p[1]+physical]"));
            assert!(!source.contains("output[p[5]+position]=input[address]"));
            let module = naga::front::wgsl::parse_str(&source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
