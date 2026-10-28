use super::*;

#[test]
fn finite_low_sign_and_extrema_bits_are_exact_through_custom_nodes() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let bits: Vec<_> = (0..=u16::MAX)
            .filter(|&x| decode(dtype, x).is_finite())
            .collect();
        let rows = bits.len() / 256;
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[rows, 256])).unwrap();
        let y = graph.input_low(dtype, shape(&[rows, 256])).unwrap();
        let neg = graph.unary_low(x, UnaryOp::Negate).unwrap();
        let abs = graph.unary_low(x, UnaryOp::Abs).unwrap();
        let min = graph.binary_low(x, y, BinaryOp::Min).unwrap();
        let max = graph.binary_low(x, y, BinaryOp::Max).unwrap();
        let program = graph.compile(&[neg, abs, min, max]).unwrap();
        let opposite: Vec<_> = bits.iter().map(|x| x ^ 0x8000).collect();
        for run in 0..2 {
            let physical = if run == 0 { &bits } else { &opposite };
            let other = if run == 0 { &opposite } else { &bits };
            let (x, y, logical) = if run == 0 {
                (
                    b.upload_low(dtype, shape(&[rows, 256]), physical).unwrap(),
                    b.upload_low(dtype, shape(&[rows, 256]), other).unwrap(),
                    physical.clone(),
                )
            } else {
                let x = b.upload_low(dtype, shape(&[256, rows]), physical).unwrap();
                let y = b.upload_low(dtype, shape(&[256, rows]), other).unwrap();
                let logical = (0..bits.len())
                    .map(|i| physical[(i % 256) * rows + i / 256])
                    .collect();
                (
                    b.permute_low(&x, &[1, 0]).unwrap(),
                    b.permute_low(&y, &[1, 0]).unwrap(),
                    logical,
                )
            };
            let outputs = program.run_typed(&[(&x).into(), (&y).into()]).unwrap();
            traces(&program, run + 1);
            let neg: Vec<_> = logical.iter().map(|x| x ^ 0x8000).collect();
            let abs: Vec<_> = logical.iter().map(|x| x & 0x7fff).collect();
            // Opposite finite operands: minimum is the negative magnitude,
            // maximum the positive magnitude, including the required zero ties.
            let min: Vec<_> = logical.iter().map(|x| x | 0x8000).collect();
            low_values(&b, &outputs[0], &[rows, 256], dtype, &neg);
            low_values(&b, &outputs[1], &[rows, 256], dtype, &abs);
            low_values(&b, &outputs[2], &[rows, 256], dtype, &min);
            low_values(&b, &outputs[3], &[rows, 256], dtype, &abs);
        }
    }
}

#[test]
fn low_operations_round_each_node_before_the_next_operation() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let half_ulp = if dtype == LowDtype::F16 {
            2f32.powi(-11)
        } else {
            2f32.powi(-8)
        };
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[3, 5])).unwrap();
        let step = graph.input_low(dtype, shape(&[])).unwrap();
        let first = graph.binary_low(x, step, BinaryOp::Add).unwrap();
        let second = graph.binary_low(first, step, BinaryOp::Add).unwrap();
        let widened = graph.cast_to_f32(second).unwrap();
        let program = graph.compile(&[second, widened]).unwrap();
        let step = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, half_ulp)])
            .unwrap();
        for (run, value) in [1., 2.].into_iter().enumerate() {
            let x = b
                .upload_low(dtype, shape(&[3, 5]), &[encode(dtype, value); 15])
                .unwrap();
            let out = program.run_typed(&[(&x).into(), (&step).into()]).unwrap();
            low_values(&b, &out[0], &[3, 5], dtype, &[encode(dtype, value); 15]);
            floats(&b, &out[1], &[3, 5], &[value; 15]);
            traces(&program, run + 1);
        }
        assert_ne!(encode(dtype, 1. + half_ulp + half_ulp), encode(dtype, 1.));
    }
}

#[test]
fn low_f32_evaluation_and_unsigned_wrapping_use_the_declared_dtype() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[3, 2])).unwrap();
        let scalar = graph.input_low(dtype, shape(&[])).unwrap();
        let unary = [
            UnaryOp::Square,
            UnaryOp::Sqrt,
            UnaryOp::Reciprocal,
            UnaryOp::Exp,
            UnaryOp::Log,
            UnaryOp::Sin,
            UnaryOp::Cos,
        ];
        let binary = [
            BinaryOp::Add,
            BinaryOp::Subtract,
            BinaryOp::Multiply,
            BinaryOp::Divide,
        ];
        let mut outputs = Vec::new();
        for op in unary {
            outputs.push(graph.unary_low(x, op).unwrap());
        }
        for op in binary {
            outputs.push(graph.binary_low(x, scalar, op).unwrap());
        }
        let program = graph.compile(&outputs).unwrap();
        let logical = [0.25f32, 1.5, 0.5, 2., 1., 3.];
        let physical = [0.25, 0.5, 1., 1.5, 2., 3.].map(|v| encode(dtype, v));
        let input = b.upload_low(dtype, shape(&[2, 3]), &physical).unwrap();
        let input = b.permute_low(&input, &[1, 0]).unwrap();
        let scalar = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 0.5)])
            .unwrap();
        let result = program
            .run_typed(&[(&input).into(), (&scalar).into()])
            .unwrap();
        for (i, op) in unary.into_iter().enumerate() {
            let expected: Vec<_> = logical
                .iter()
                .map(|&v| {
                    encode(
                        dtype,
                        match op {
                            UnaryOp::Square => v * v,
                            UnaryOp::Sqrt => v.sqrt(),
                            UnaryOp::Reciprocal => v.recip(),
                            UnaryOp::Exp => v.exp(),
                            UnaryOp::Log => v.ln(),
                            UnaryOp::Sin => v.sin(),
                            _ => v.cos(),
                        },
                    )
                })
                .collect();
            low_values(&b, &result[i], &[3, 2], dtype, &expected);
        }
        for (i, op) in binary.into_iter().enumerate() {
            let expected: Vec<_> = logical
                .iter()
                .map(|&v| {
                    encode(
                        dtype,
                        match op {
                            BinaryOp::Add => v + 0.5,
                            BinaryOp::Subtract => v - 0.5,
                            BinaryOp::Multiply => v * 0.5,
                            _ => v / 0.5,
                        },
                    )
                })
                .collect();
            low_values(&b, &result[unary.len() + i], &[3, 2], dtype, &expected);
        }
    }
    let ops = [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Min,
        BinaryOp::Max,
    ];
    let mut graph = b.program();
    let x = graph.input_u32(shape(&[2, 3])).unwrap();
    let y = graph.input_u32(shape(&[3])).unwrap();
    let outputs: Vec<_> = ops
        .iter()
        .map(|&op| graph.binary_u32(x, y, op).unwrap())
        .collect();
    let program = graph.compile(&outputs).unwrap();
    let av = [u32::MAX, 1 << 31, (1 << 24) + 1, 0, 3, 65537];
    let bv = [2, u32::MAX, 65537];
    let a = b.upload_u32(shape(&[2, 3]), &av).unwrap();
    let c = b.upload_u32(shape(&[3]), &bv).unwrap();
    let result = program.run_typed(&[(&a).into(), (&c).into()]).unwrap();
    for (output, op) in result.iter().zip(ops) {
        assert_eq!(output.dtype(), MlxDtype::U32);
        let expected: Vec<_> = av
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let y = bv[i % 3];
                match op {
                    BinaryOp::Add => x.wrapping_add(y),
                    BinaryOp::Subtract => x.wrapping_sub(y),
                    BinaryOp::Multiply => x.wrapping_mul(y),
                    BinaryOp::Min => x.min(y),
                    _ => x.max(y),
                }
            })
            .collect();
        assert_eq!(b.read_u32(output.as_tensor().unwrap()).unwrap(), expected);
    }
}
