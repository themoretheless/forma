use super::*;

#[test]
fn low_reductions_keep_f32_partials_and_round_the_completed_result_once() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for count in [1, 257, 4097, 131077] {
            let mut graph = b.program();
            let x = graph.input_low(dtype, shape(&[2, count])).unwrap();
            let mut outputs = Vec::new();
            for op in [
                ReduceOp::Sum,
                ReduceOp::Product,
                ReduceOp::Min,
                ReduceOp::Max,
            ] {
                outputs.push(graph.reduce_low_f32(x, op, &[1], false).unwrap());
                outputs.push(graph.reduce_low(x, op, &[1], false).unwrap());
            }
            outputs.push(graph.mean_low_f32(x, &[1], false).unwrap());
            outputs.push(graph.mean_low(x, &[1], false).unwrap());
            let program = graph.compile(&outputs).unwrap();
            let mut logical = vec![1. / 256.; count];
            logical[0] = 0.5;
            if count > 1 {
                logical[count - 1] = 0.125;
            }
            let mut physical = Vec::new();
            for &x in &logical {
                physical.extend([encode(dtype, x), encode(dtype, -x)]);
            }
            let input = b.upload_low(dtype, shape(&[count, 2]), &physical).unwrap();
            let input = b.permute_low(&input, &[1, 0]).unwrap();
            let out = program.run_typed(&[(&input).into()]).unwrap();
            for (i, op) in [
                ReduceOp::Sum,
                ReduceOp::Product,
                ReduceOp::Min,
                ReduceOp::Max,
            ]
            .into_iter()
            .enumerate()
            {
                let expected: Vec<_> = [1., -1.]
                    .iter()
                    .map(|&sign| {
                        let iter = logical.iter().map(|&x| f64::from(sign * x));
                        (match op {
                            ReduceOp::Sum => iter.sum::<f64>(),
                            ReduceOp::Product => iter.product::<f64>(),
                            ReduceOp::Min => iter.fold(f64::INFINITY, f64::min),
                            ReduceOp::Max => iter.fold(f64::NEG_INFINITY, f64::max),
                        }) as f32
                    })
                    .collect();
                // Large products underflow, so their zero sign is not fixed by
                // the arithmetic contract; the exact dyadic other cases are.
                if op == ReduceOp::Product && count > 1 {
                    assert_eq!(
                        b.read_f32(out[i * 2].as_tensor().unwrap()).unwrap(),
                        expected
                    );
                } else {
                    floats(&b, &out[i * 2], &[2], &expected);
                }
                let f32_result = b.read_f32(out[i * 2].as_tensor().unwrap()).unwrap();
                low_values(
                    &b,
                    &out[i * 2 + 1],
                    &[2],
                    dtype,
                    &f32_result
                        .iter()
                        .map(|&x| encode(dtype, x))
                        .collect::<Vec<_>>(),
                );
            }
            let mean = b.read_f32(out[8].as_tensor().unwrap()).unwrap();
            let expected = logical.iter().map(|&x| f64::from(x)).sum::<f64>() / count as f64;
            for (&actual, sign) in mean.iter().zip([1., -1.]) {
                assert!(
                    actual.is_finite()
                        && (f64::from(actual) - sign * expected).abs() <= expected * 2e-6
                );
            }
            low_values(
                &b,
                &out[9],
                &[2],
                dtype,
                &mean.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>(),
            );
            traces(&program, 1);
        }
    }
}

#[test]
fn low_extrema_hierarchy_preserves_subnormals_and_both_zero_signs() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let count = 131077;
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[2, count])).unwrap();
        let min = graph.reduce_low_f32(x, ReduceOp::Min, &[1], true).unwrap();
        let max = graph.reduce_low_f32(x, ReduceOp::Max, &[1], true).unwrap();
        let program = graph.compile(&[min, max]).unwrap();
        let mut bits = vec![2; count];
        bits[count - 1] = 1;
        bits.extend((0..count).map(|i| if i % 2 == 0 { 0x8000 } else { 0 }));
        let x = b.upload_low(dtype, shape(&[2, count]), &bits).unwrap();
        let out = program.run_typed(&[(&x).into()]).unwrap();
        floats(&b, &out[0], &[2, 1], &[decode(dtype, 1), -0.]);
        floats(&b, &out[1], &[2, 1], &[decode(dtype, 2), 0.]);
    }
}

#[test]
fn unsigned_reductions_wrap_exactly_and_empty_contractions_use_typed_identities() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    let x = graph.input_u32(shape(&[3, 5])).unwrap();
    let ops = [
        ReduceOp::Sum,
        ReduceOp::Product,
        ReduceOp::Min,
        ReduceOp::Max,
    ];
    let outputs: Vec<_> = ops
        .iter()
        .map(|&op| graph.reduce_u32(x, op, &[1], false).unwrap())
        .collect();
    let program = graph.compile(&outputs).unwrap();
    let data = [
        u32::MAX,
        2,
        3,
        7,
        11,
        1 << 31,
        3,
        65537,
        65539,
        13,
        (1 << 24) + 1,
        1,
        1,
        1,
        1,
    ];
    let x = b.upload_u32(shape(&[3, 5]), &data).unwrap();
    let out = program.run_typed(&[(&x).into()]).unwrap();
    for (result, op) in out.iter().zip(ops) {
        let expected: Vec<_> = data
            .chunks(5)
            .map(|row| match op {
                ReduceOp::Sum => row.iter().fold(0u32, |a, &x| a.wrapping_add(x)),
                ReduceOp::Product => row.iter().fold(1u32, |a, &x| a.wrapping_mul(x)),
                ReduceOp::Min => *row.iter().min().unwrap(),
                ReduceOp::Max => *row.iter().max().unwrap(),
            })
            .collect();
        assert_eq!(result.dtype(), MlxDtype::U32);
        assert_eq!(b.read_u32(result.as_tensor().unwrap()).unwrap(), expected);
    }
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[2, 0, 3])).unwrap();
        let sum = graph.reduce_low_f32(x, ReduceOp::Sum, &[1], false).unwrap();
        let product = graph.reduce_low(x, ReduceOp::Product, &[1], false).unwrap();
        let identity = graph.reduce_low(x, ReduceOp::Sum, &[], false).unwrap();
        let empty = graph.mean_low_f32(x, &[0], false).unwrap();
        let program = graph.compile(&[sum, product, identity, empty]).unwrap();
        let x = b.upload_low(dtype, shape(&[2, 0, 3]), &[]).unwrap();
        let out = program.run_typed(&[(&x).into()]).unwrap();
        floats(&b, &out[0], &[2, 3], &[0.; 6]);
        low_values(&b, &out[1], &[2, 3], dtype, &[encode(dtype, 1.); 6]);
        low_values(&b, &out[2], &[2, 0, 3], dtype, &[]);
        floats(&b, &out[3], &[0, 3], &[]);
    }
}
