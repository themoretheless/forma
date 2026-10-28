//! Only operand loads differ from the shared tiled/split attention shader.
fn replace(source: &mut String, from: &str, to: &str) {
    assert!(
        source.contains(from),
        "attention specialization hook missing: {from}"
    );
    *source = source.replace(from, to);
}
pub(super) fn source(dtype: tensor_core::LowDtype) -> String {
    // Git checkouts on Windows may use CRLF; specialization hooks use LF.
    let mut source = crate::shaders::TENSOR_ATTENTION_WGSL.replace("\r\n", "\n");
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
    let mut loader =
        include_str!("../../../shaders/tensor_low_attention_load.wgsl").replace("\r\n", "\n");
    if dtype == tensor_core::LowDtype::F16 {
        // Bounded query reuse benefits f16 decoding. BF16 keeps its original
        // source with no cache storage or added synchronization.
        replace(
            &mut source,
            "low_query_bits(qbase + depth * params[17])",
            "cached_query_bits(qbase, depth)",
        );
        replace(
            &mut source,
            "        var maximum = 0.0;",
            "        if params[6] <= LOW_QUERY_CACHE_DEPTH {\n            var depth = lid.x;\n            while depth < params[6] {\n                low_query_cache[depth] = low_query_bits(qbase + depth * params[17]);\n                depth += WG;\n            }\n        }\n        workgroupBarrier();\n        var maximum = 0.0;",
        );
        // Only normal half inputs use the native conversion. The portable
        // unpack contract permits flushing half subnormals, so those and all
        // special encodings keep the original exact integer decoder.
        replace(
            &mut loader,
            "return low_decode((word >> (16u * (address % 2u))) & 0xffffu, LOW_DTYPE);",
            "let bits = (word >> (16u * (address % 2u))) & 0xffffu;\n    let exponent = bits & 0x7c00u;\n    if exponent == 0u || exponent == 0x7c00u { return low_decode(bits, 0u); }\n    return bitcast<u32>(unpack2x16float(bits).x);",
        );
        // Every finite nonzero f16 decodes to a normal f32, so no input-flush
        // protection is needed. Subnormal/special encodings retain integer decoding.
        loader.truncate(
            loader
                .find("fn low_attention_product")
                .expect("product hook"),
        );
        loader.push_str("fn low_attention_product(a: u32, b: u32) -> f32 { return bitcast<f32>(a) * bitcast<f32>(b); }\n");
        loader.push_str(
            r#"
const LOW_QUERY_CACHE_DEPTH: u32 = 256u;
var<workgroup> low_query_cache: array<u32, LOW_QUERY_CACHE_DEPTH>;
fn cached_query_bits(base: u32, depth: u32) -> u32 {
    if params[6] <= LOW_QUERY_CACHE_DEPTH { return low_query_cache[depth]; }
    return low_query_bits(base + depth * params[17]);
}
"#,
        );
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
                assert!(source.contains("var<workgroup> low_query_cache"));
            } else {
                assert!(!source.contains("low_query_cache"));
                assert!(!source.contains("cached_query_bits"));
            }
        }
    }
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn guarded_f16_attention_loader_preserves_every_finite_pattern() {
        use crate::{Binding, ComputeRuntime, Kernel, gpu_compute::GpuContext};
        let Some(context) = GpuContext::new() else {
            assert!(std::env::var_os("COMPUTE_REQUIRE_GPU").is_none());
            return;
        };
        let runtime = ComputeRuntime::new(&context).unwrap();
        // This entry calls the actual production query loader, including its
        // extracted-halfword native branch and original integer fallback.
        let source = format!(
            "{}\n{}",
            super::source(tensor_core::LowDtype::F16),
            r#"
@compute @workgroup_size(64)
fn decode_probe(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < params[1] {
        output[gid.x] = bitcast<f32>(low_query_bits(gid.x * 2u + params[0]));
    }
}
"#
        );
        use Binding::{StorageRead as R, StorageReadWrite as W};
        let kernel = Kernel::new(
            runtime.device(),
            "production f16 attention loader exhaustive probe",
            &source,
            "decode_probe",
            &[R, R, R, R, R, W, W],
        )
        .unwrap();
        let patterns: Vec<u16> = (0..=u16::MAX).filter(|x| x & 0x7c00 != 0x7c00).collect();
        let expected: Vec<u32> = patterns
            .iter()
            .map(|&bits| {
                let exponent = i32::from((bits >> 10) & 31);
                let fraction = f64::from(bits & 1023);
                let magnitude = if exponent == 0 {
                    fraction * 2f64.powi(-24)
                } else {
                    (1.0 + fraction / 1024.0) * 2f64.powi(exponent - 15)
                };
                let signed = if bits & 0x8000 == 0 {
                    magnitude
                } else {
                    -magnitude
                };
                (signed as f32).to_bits()
            })
            .collect();
        let neighbors = [
            0u16, 0x8000, 1, 0x8001, 0x7bff, 0x7c00, 0xfc00, 0x7e01, 0xfe55,
        ];
        let count = patterns.len() * neighbors.len();
        let input = runtime.zeros::<u32>(count + 2).unwrap();
        let output = runtime.zeros::<f32>(count).unwrap();
        let params = runtime.zeros::<u32>(2).unwrap();
        let unused_read = runtime.zeros::<u32>(1).unwrap();
        let unused_write = runtime.zeros::<u32>(2).unwrap();
        for parity in 0..2u32 {
            let mut words = Vec::with_capacity(count + 2);
            words.push(0xfe557e01);
            for neighbor in neighbors {
                for &bits in &patterns {
                    words.push(if parity == 0 {
                        u32::from(bits) | (u32::from(neighbor) << 16)
                    } else {
                        u32::from(neighbor) | (u32::from(bits) << 16)
                    });
                }
            }
            words.push(0x7c00fc00);
            runtime.write(&input, 0, &words).unwrap();
            runtime
                .write(&params, 0, &[2 + parity, count as u32])
                .unwrap();
            kernel.dispatch(
                runtime.device(),
                runtime.queue(),
                &[
                    params.buffer(),
                    input.buffer(),
                    unused_read.buffer(),
                    unused_read.buffer(),
                    unused_read.buffer(),
                    output.buffer(),
                    unused_write.buffer(),
                ],
                count as u32,
            );
            let actual = runtime
                .read(&output)
                .unwrap()
                .wait(std::time::Duration::from_secs(30))
                .unwrap();
            for (index, value) in actual.into_iter().enumerate() {
                assert_eq!(
                    value.to_bits(),
                    expected[index % patterns.len()],
                    "parity={parity} pattern={:#06x} neighbor={:#06x}",
                    patterns[index % patterns.len()],
                    neighbors[index / patterns.len()]
                );
            }
        }
        println!(
            "production f16 loader: {} finite patterns × {} neighbors × 2 parities = {} exact f32 cases",
            patterns.len(),
            neighbors.len(),
            count * 2
        );
    }
}
