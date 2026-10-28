use super::*;
use tensor_core::TensorLowOpsBackend;

#[test]
fn eager_and_compiled_bf16_multiply_preserve_normal_results_from_tiny_operands() {
    let Some(b) = backend() else { return };
    let dtype = LowDtype::Bf16;
    let mut pairs = Vec::new();
    for a in [0x0001, 0x007f, 0, 0x3fc0] {
        let c = if a == 0x3fc0 { 0x4000 } else { 0x7f7f };
        for sign_a in [0, 0x8000] {
            for sign_c in [0, 0x8000] {
                let pair = (a | sign_a, c | sign_c);
                pairs.extend([pair, (pair.1, pair.0)]);
            }
        }
    }
    assert_eq!(pairs.len(), 32);
    let mut graph = b.program();
    let left = graph.input_low(dtype, shape(&[4, 8])).unwrap();
    let right = graph.input_low(dtype, shape(&[4, 8])).unwrap();
    let product = graph.binary_low(left, right, BinaryOp::Multiply).unwrap();
    let wide = graph.cast_to_f32(product).unwrap();
    let program = graph.compile(&[product, wide]).unwrap();
    for run in 0..2 {
        let left_bits: Vec<_> = pairs
            .iter()
            .map(|p| p.0 ^ if run == 0 { 0 } else { 0x8000 })
            .collect();
        let right_bits: Vec<_> = pairs.iter().map(|p| p.1).collect();
        let dims = if run == 0 { [4, 8] } else { [8, 4] };
        let left = b.upload_low(dtype, shape(&dims), &left_bits).unwrap();
        let right = b.upload_low(dtype, shape(&dims), &right_bits).unwrap();
        let (left, right) = if run == 0 {
            (left, right)
        } else {
            (
                b.permute_low(&left, &[1, 0]).unwrap(),
                b.permute_low(&right, &[1, 0]).unwrap(),
            )
        };
        let eager = b.binary_low(BinaryOp::Multiply, &left, &right).unwrap();
        let out = program
            .run_typed(&[(&left).into(), (&right).into()])
            .unwrap();
        traces(&program, run + 1);
        let eager_bits = b.read_low_bits(&eager).unwrap();
        let compiled_bits = b.read_low_bits(out[0].as_low().unwrap()).unwrap();
        let wide = b.read_f32(out[1].as_tensor().unwrap()).unwrap();
        assert_eq!(eager_bits.len(), 32);
        assert_eq!(compiled_bits.len(), 32);
        assert_eq!(wide.len(), 32);
        for i in 0..32 {
            let source = if run == 0 { i } else { (i % 8) * 4 + i / 8 };
            let product = f64::from(decode(dtype, left_bits[source]))
                * f64::from(decode(dtype, right_bits[source]));
            assert_eq!(f64::from(product as f32), product);
            if source < 16 {
                assert!((product as f32).is_normal());
            }
            let expected = encode(dtype, product as f32);
            for (path, &actual) in [("eager", &eager_bits[i]), ("compiled", &compiled_bits[i])] {
                if product == 0. {
                    assert_eq!(actual & 0x7fff, 0, "{path} zero control {i}");
                } else {
                    assert_eq!(actual, expected, "{path} normal product {i}: {product}");
                }
            }
            if product == 0. {
                assert_eq!(wide[i], 0.);
            } else {
                assert_eq!(wide[i].to_bits(), decode(dtype, expected).to_bits());
            }
        }
    }
}
