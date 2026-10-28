use super::*;

#[test]
fn cancellation_accumulates_in_f32_before_low_rounding() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let a = graph.input_low(dtype, shape(&[65])).unwrap();
        let c = graph.input_low(dtype, shape(&[65])).unwrap();
        let wide = graph.matmul_low_f32(a, c).unwrap();
        let low = graph.matmul_low(a, c).unwrap();
        let program = graph.compile(&[wide, low]).unwrap();
        let mut bits = vec![encode(dtype, 0.); 65];
        bits[0] = encode(dtype, 4096.);
        bits[1] = encode(dtype, 1.);
        bits[64] = encode(dtype, -4096.);
        let a = b.upload_low(dtype, shape(&[65]), &bits).unwrap();
        let c = b
            .upload_low(dtype, shape(&[65]), &[encode(dtype, 1.); 65])
            .unwrap();
        let out = program.run_typed(&[(&a).into(), (&c).into()]).unwrap();
        floats(&b, &out[0], &[], &[1.]);
        low_values(&b, &out[1], &[], dtype, &[encode(dtype, 1.)]);
    }
}

#[test]
fn direct_low_matmul_has_unrounded_f32_outputs_with_strided_replay() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for k in [65, 257] {
            let mut graph = b.program();
            let a = graph.input_low(dtype, shape(&[17, k])).unwrap();
            let c = graph.input_low(dtype, shape(&[k, 19])).unwrap();
            let product = graph.matmul_low_f32(a, c).unwrap();
            let rounded = graph.matmul_low(a, c).unwrap();
            let program = graph.compile(&[product, rounded]).unwrap();
            for run in 0..2 {
                let mut av: Vec<_> = (0..17 * k)
                    .map(|i| ((i * 3 + run * 2) % 7 + 1) as f32 / 16.)
                    .collect();
                let mut bv: Vec<_> = (0..19 * k)
                    .map(|i| ((i * 5 + i / 11 + run) % 7 + 1) as f32 / 16.)
                    .collect();
                // An explicit midpoint witness survives in f32 and differs
                // from low rounding even for the shorter K=65 case.
                for inner in 0..k {
                    av[inner * 17] = 0.;
                    bv[inner] = 1.;
                }
                av[0] = 1.;
                av[17] = if dtype == LowDtype::F16 {
                    2f32.powi(-11)
                } else {
                    2f32.powi(-8)
                };
                let a = b
                    .upload_low(
                        dtype,
                        shape(&[k, 17]),
                        &av.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>(),
                    )
                    .unwrap();
                let c = b
                    .upload_low(
                        dtype,
                        shape(&[19, k]),
                        &bv.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>(),
                    )
                    .unwrap();
                let a = b.permute_low(&a, &[1, 0]).unwrap();
                let c = b.permute_low(&c, &[1, 0]).unwrap();
                // Warm a contiguous signature, then replay both changed
                // values and native transposed strides without retracing.
                let a = if run == 0 {
                    b.materialize_low(&a).unwrap()
                } else {
                    a
                };
                let c = if run == 0 {
                    b.materialize_low(&c).unwrap()
                } else {
                    c
                };
                let out = program.run_typed(&[(&a).into(), (&c).into()]).unwrap();
                let mut expected = Vec::new();
                for row in 0..17 {
                    for col in 0..19 {
                        // Independent physical indexing. Every product has
                        // denominator at most 32768 and magnitude <= 1, so every
                        // partial numerator is <= 257*32768 < 2^24: exact in f32.
                        expected.push(
                            (0..k)
                                .map(|inner| {
                                    f64::from(av[inner * 17 + row]) * f64::from(bv[col * k + inner])
                                })
                                .fold(0.0_f64, |sum, x| sum + x) as f32,
                        );
                    }
                }
                floats(&b, &out[0], &[17, 19], &expected);
                let rounded: Vec<_> = expected.iter().map(|&x| encode(dtype, x)).collect();
                assert!(
                    expected
                        .iter()
                        .zip(&rounded)
                        .any(|(&x, &low)| x != decode(dtype, low))
                );
                low_values(&b, &out[1], &[17, 19], dtype, &rounded);
                traces(&program, run + 1);
            }
        }
    }
}

#[test]
fn vector_promotion_batch_zero_strides_and_empty_products_remain_typed() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for k in [0, 3] {
            let mut graph = b.program();
            let vector = graph.input_low(dtype, shape(&[k])).unwrap();
            let matrix = graph.input_low(dtype, shape(&[2, k])).unwrap();
            let columns = graph.permute(matrix, &[1, 0]).unwrap();
            let dot = graph.matmul_low_f32(vector, vector).unwrap();
            let mv = graph.matmul_low_f32(matrix, vector).unwrap();
            let vm = graph.matmul_low(vector, columns).unwrap();
            let broadcast = graph.broadcast_to(matrix, shape(&[2, 1, 2, k])).unwrap();
            let batch = graph.matmul_low_f32(broadcast, columns).unwrap();
            let program = graph.compile(&[dot, mv, vm, batch]).unwrap();
            let x: Vec<_> = (0..k).map(|i| i as f32 - 1.).collect();
            let a: Vec<_> = (0..2 * k).map(|i| i as f32 + 1.).collect();
            let vector = b
                .upload_low(
                    dtype,
                    shape(&[k]),
                    &x.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>(),
                )
                .unwrap();
            let matrix = b
                .upload_low(
                    dtype,
                    shape(&[2, k]),
                    &a.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>(),
                )
                .unwrap();
            let out = program
                .run_typed(&[(&vector).into(), (&matrix).into()])
                .unwrap();
            floats(&b, &out[0], &[], &[if k == 0 { 0. } else { 2. }]);
            let expected = if k == 0 { [0., 0.] } else { [2., 2.] };
            floats(&b, &out[1], &[2], &expected);
            low_values(
                &b,
                &out[2],
                &[2],
                dtype,
                &expected.map(|x| encode(dtype, x)),
            );
            let mut expected = Vec::new();
            for _ in 0..2 {
                for row in 0..2 {
                    for col in 0..2 {
                        expected.push(
                            (0..k)
                                .map(|j| f64::from(a[row * k + j]) * f64::from(a[col * k + j]))
                                .fold(0.0_f64, |sum, x| sum + x) as f32,
                        );
                    }
                }
            }
            floats(&b, &out[3], &[2, 1, 2, 2], &expected);
        }
        let mut graph = b.program();
        let a = graph.input_low(dtype, shape(&[0, 2, 3])).unwrap();
        let c = graph.input_low(dtype, shape(&[1, 3, 4])).unwrap();
        let y = graph.matmul_low_f32(a, c).unwrap();
        let low = graph.matmul_low(a, c).unwrap();
        let program = graph.compile(&[y, low]).unwrap();
        let a = b.upload_low(dtype, shape(&[0, 2, 3]), &[]).unwrap();
        let c = b
            .upload_low(dtype, shape(&[1, 3, 4]), &[encode(dtype, 1.); 12])
            .unwrap();
        let out = program.run_typed(&[(&a).into(), (&c).into()]).unwrap();
        floats(&b, &out[0], &[0, 2, 4], &[]);
        low_values(&b, &out[1], &[0, 2, 4], dtype, &[]);
    }
}

#[test]
fn compiled_bf16_tiny_products_survive_matrix_and_vector_paths() {
    let Some(b) = backend() else { return };
    let dtype = LowDtype::Bf16;
    for (m, k, n) in [(17, 65, 19), (1, 257, 3), (3, 257, 1)] {
        let mut graph = b.program();
        let a = graph.input_low(dtype, shape(&[m, k])).unwrap();
        let c = graph.input_low(dtype, shape(&[k, n])).unwrap();
        let y = graph.matmul_low_f32(a, c).unwrap();
        let program = graph.compile(&[y]).unwrap();
        for (run, (left, right)) in [(0x0001, 0x7f7f), (0x7f7f, 0x0001)].into_iter().enumerate() {
            let mut av = vec![0; m * k];
            let mut bv = vec![0; k * n];
            for row in 0..m {
                av[row * k + k - 1] = left;
            }
            for col in 0..n {
                bv[(k - 1) * n + col] = right;
            }
            let a = b.upload_low(dtype, shape(&[m, k]), &av).unwrap();
            let c = b.upload_low(dtype, shape(&[k, n]), &bv).unwrap();
            let out = program.run_typed(&[(&a).into(), (&c).into()]).unwrap();
            let expected =
                (f64::from(decode(dtype, left)) * f64::from(decode(dtype, right))) as f32;
            assert!(expected.is_normal());
            floats(&b, &out[0], &[m, n], &vec![expected; m * n]);
            traces(&program, run + 1);
        }
    }
}
