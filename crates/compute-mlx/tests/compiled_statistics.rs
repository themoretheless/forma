#![cfg(not(target_arch = "wasm32"))]
use compute_mlx::{
    MlxBackend, MlxCompiledProgram, MlxError, MlxProgramBuilder, MlxProgramOutput, MlxValue,
};
use half::{bf16, f16};
use tensor_core::{HasShape, LowDtype, Shape, TensorError, TensorLowBackend};

fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn backend() -> Option<MlxBackend> {
    match MlxBackend::new_gpu().and_then(|b| {
        if b.compile_available() {
            Ok(b)
        } else {
            Err(MlxError::CompileUnavailable)
        }
    }) {
        Ok(b) => Some(b),
        Err(error) => {
            eprintln!("SKIP compiled MLX statistics unavailable: {error}");
            assert!(std::env::var_os("COMPUTE_REQUIRE_MLX").is_none(), "{error}");
            None
        }
    }
}
fn traces(program: &MlxCompiledProgram, runs: usize) {
    let expected = if std::env::var_os("MLX_DISABLE_COMPILE").is_some() {
        runs
    } else {
        usize::from(runs != 0)
    };
    assert_eq!(program.trace_count(), expected);
}
fn encode(dtype: LowDtype, value: f32) -> u16 {
    match dtype {
        LowDtype::F16 => f16::from_f32(value).to_bits(),
        LowDtype::Bf16 => bf16::from_f32(value).to_bits(),
    }
}
fn decode(dtype: LowDtype, bits: u16) -> f32 {
    match dtype {
        LowDtype::F16 => f16::from_bits(bits).to_f32(),
        LowDtype::Bf16 => bf16::from_bits(bits).to_f32(),
    }
}
fn outputs(
    graph: &mut MlxProgramBuilder,
    input: MlxValue,
    axes: &[usize],
    keep: bool,
    epsilon: f32,
    low: bool,
) -> Vec<MlxValue> {
    if low {
        let pair = graph.moments_low_f32(input, axes, keep).unwrap();
        vec![
            graph.softmax_low_f32(input, axes).unwrap(),
            graph.log_softmax_low_f32(input, axes).unwrap(),
            graph.logsumexp_low_f32(input, axes, keep).unwrap(),
            pair.mean,
            pair.variance,
            graph.layer_norm_low_f32(input, axes, epsilon).unwrap(),
        ]
    } else {
        let pair = graph.moments(input, axes, keep).unwrap();
        vec![
            graph.softmax(input, axes).unwrap(),
            graph.log_softmax(input, axes).unwrap(),
            graph.logsumexp(input, axes, keep).unwrap(),
            pair.mean,
            pair.variance,
            graph.layer_norm(input, axes, epsilon).unwrap(),
        ]
    }
}
fn low_outputs(
    graph: &mut MlxProgramBuilder,
    input: MlxValue,
    axes: &[usize],
    keep: bool,
    epsilon: f32,
) -> Vec<MlxValue> {
    let pair = graph.moments_low(input, axes, keep).unwrap();
    vec![
        graph.softmax_low(input, axes).unwrap(),
        graph.log_softmax_low(input, axes).unwrap(),
        graph.logsumexp_low(input, axes, keep).unwrap(),
        pair.mean,
        pair.variance,
        graph.layer_norm_low(input, axes, epsilon).unwrap(),
    ]
}
fn close(actual: f32, expected: f64, floor: f64) {
    let rounded = expected as f32;
    if rounded.is_infinite() {
        assert_eq!(actual, rounded);
        return;
    }
    assert!(
        actual.is_finite()
            && (f64::from(actual) - expected).abs() <= 2e-4 * expected.abs().max(floor),
        "{actual:?} != {expected:?}; floor {floor}"
    );
}
fn check_rows(
    b: &MlxBackend,
    out: &[MlxProgramOutput],
    rows: &[Vec<f32>],
    epsilon: f32,
    output_shape: &[usize],
    reduced_shape: &[usize],
) {
    assert_eq!(out.len(), 6);
    for i in [0, 1, 5] {
        assert_eq!(out[i].shape(), &shape(output_shape));
    }
    for i in [2, 3, 4] {
        assert_eq!(out[i].shape(), &shape(reduced_shape));
    }
    let arrays: Vec<_> = out
        .iter()
        .map(|v| b.read_f32(v.as_tensor().unwrap()).unwrap())
        .collect();
    let count = rows[0].len();
    for (r, data) in rows.iter().enumerate() {
        let data: Vec<_> = data.iter().map(|&x| f64::from(x)).collect();
        let mean = data.iter().sum::<f64>() / count as f64;
        let variance = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / count as f64;
        let max = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let sum = data.iter().map(|x| (x - max).exp()).sum::<f64>();
        let scale = data.iter().map(|x| x.abs()).fold(1e-38, f64::max);
        close(arrays[2][r], max + sum.ln(), 1e-6);
        close(arrays[3][r], mean, scale);
        // True underflow can flush, but normal outputs are checked relatively.
        close(arrays[4][r], variance, f64::from(f32::MIN_POSITIVE));
        for (i, &x) in data.iter().enumerate() {
            let j = r * count + i;
            close(arrays[0][j], (x - max).exp() / sum, 1e-30);
            close(arrays[1][j], x - max - sum.ln(), 1e-6);
            close(
                arrays[5][j],
                (x - mean) / (variance + f64::from(epsilon)).sqrt(),
                if scale < f64::from(f32::MIN_POSITIVE) {
                    1e-30
                } else {
                    1.
                },
            );
        }
    }
}

#[test]
fn compiled_statistics_replays_f32_and_low_strided_hierarchies() {
    let Some(b) = backend() else { return };
    for count in [255, 256, 257, 513, 131077] {
        for dtype in [None, Some(LowDtype::F16), Some(LowDtype::Bf16)] {
            let mut graph = b.program();
            let input = match dtype {
                None => graph.input(shape(&[2, count])).unwrap(),
                Some(t) => graph.input_low(t, shape(&[2, count])).unwrap(),
            };
            let values = outputs(&mut graph, input, &[1], true, 1e-5, dtype.is_some());
            let program = graph.compile(&values).unwrap();
            let mut old: Option<(Vec<MlxProgramOutput>, Vec<Vec<f32>>)> = None;
            for replay in 0..2 {
                let rows: Vec<Vec<f32>> = (0..2)
                    .map(|r| {
                        (0..count)
                            .map(|i| ((i + replay * 3) % 7) as f32 * 0.25 - 0.75 + r as f32)
                            .collect()
                    })
                    .collect();
                let logical: Vec<_> = rows.iter().flatten().copied().collect();
                let physical: Vec<_> = if replay == 0 {
                    logical.clone()
                } else {
                    (0..count)
                        .flat_map(|i| rows.iter().map(move |r| r[i]))
                        .collect()
                };
                let source_shape = if replay == 0 {
                    shape(&[2, count])
                } else {
                    shape(&[count, 2])
                };
                let out = if let Some(dtype) = dtype {
                    let bits: Vec<_> = physical.iter().map(|&x| encode(dtype, x)).collect();
                    let source = b.upload_low(dtype, source_shape, &bits).unwrap();
                    let input = if replay == 0 {
                        source
                    } else {
                        b.permute_low(&source, &[1, 0]).unwrap()
                    };
                    program.run_typed(&[(&input).into()]).unwrap()
                } else {
                    let source = b.upload_f32(source_shape, &physical).unwrap();
                    let input = if replay == 0 {
                        source
                    } else {
                        b.permute(&source, &[1, 0]).unwrap()
                    };
                    program.run_typed(&[(&input).into()]).unwrap()
                };
                check_rows(&b, &out, &rows, 1e-5, &[2, count], &[2, 1]);
                if let Some((old_out, old_rows)) = &old {
                    check_rows(&b, old_out, old_rows, 1e-5, &[2, count], &[2, 1]);
                }
                old = Some((out, rows));
                traces(&program, replay + 1);
            }
        }
    }
}

#[test]
fn compiled_low_statistics_preserve_extremes_tiny_normalization_and_final_rounding() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for count in [5, 513] {
            let max = if dtype == LowDtype::F16 {
                0x7bff
            } else {
                0x7f7f
            };
            let patterns = [
                vec![1, 0x8001, 0x007f, 0x807f, 1],
                vec![max, max - 1, max - 2, max - 4, max - 8],
                vec![
                    max | 0x8000,
                    (max - 1) | 0x8000,
                    (max - 2) | 0x8000,
                    (max - 4) | 0x8000,
                    (max - 8) | 0x8000,
                ],
            ];
            for pattern in patterns {
                let bits: Vec<_> = (0..count).map(|i| pattern[i % pattern.len()]).collect();
                let rows = vec![bits.iter().map(|&x| decode(dtype, x)).collect()];
                let mut graph = b.program();
                let x = graph.input_low(dtype, shape(&[1, count])).unwrap();
                let mut values = outputs(&mut graph, x, &[1], false, f32::from_bits(1), true);
                values.extend(low_outputs(&mut graph, x, &[1], false, f32::from_bits(1)));
                let program = graph.compile(&values).unwrap();
                let x = b.upload_low(dtype, shape(&[1, count]), &bits).unwrap();
                let out = program.run_typed(&[(&x).into()]).unwrap();
                check_rows(&b, &out[..6], &rows, f32::from_bits(1), &[1, count], &[1]);
                for i in 0..6 {
                    let wide = b.read_f32(out[i].as_tensor().unwrap()).unwrap();
                    let expected: Vec<_> = wide.iter().map(|&x| encode(dtype, x)).collect();
                    assert_eq!(
                        b.read_low_bits(out[i + 6].as_low().unwrap()).unwrap(),
                        expected,
                        "single final rounding dtype={dtype:?} operation={i}"
                    );
                }
                traces(&program, 1);
            }
        }
    }
}

#[test]
fn compiled_f32_statistics_near_maximum_are_stable() {
    let Some(b) = backend() else { return };
    for sign in [0, 0x8000_0000] {
        let data: Vec<_> = [0, 1, 2, 4, 8]
            .map(|step| f32::from_bits((f32::MAX.to_bits() - step * 65536) | sign))
            .to_vec();
        let mut graph = b.program();
        let x = graph.input(shape(&[1, 5])).unwrap();
        let values = outputs(&mut graph, x, &[1], false, f32::from_bits(1), false);
        let program = graph.compile(&values).unwrap();
        let input = b.upload_f32(shape(&[1, 5]), &data).unwrap();
        let out = program.run_typed(&[(&input).into()]).unwrap();
        check_rows(&b, &out, &[data], f32::from_bits(1), &[1, 5], &[1]);
    }
}

#[test]
fn compiled_singleton_low_means_and_logsumexp_preserve_every_finite_pattern() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let bits: Vec<_> = (0..=u16::MAX)
            .filter(|&x| decode(dtype, x).is_finite())
            .collect();
        for axes in [vec![], vec![1]] {
            let mut graph = b.program();
            let x = graph.input_low(dtype, shape(&[bits.len(), 1])).unwrap();
            let mean = graph.moments_low_f32(x, &axes, true).unwrap();
            let lse = graph.logsumexp_low_f32(x, &axes, true).unwrap();
            let low = graph.moments_low(x, &axes, true).unwrap();
            let low_lse = graph.logsumexp_low(x, &axes, true).unwrap();
            let program = graph
                .compile(&[
                    mean.mean,
                    mean.variance,
                    lse,
                    low.mean,
                    low.variance,
                    low_lse,
                ])
                .unwrap();
            let input = b.upload_low(dtype, shape(&[bits.len(), 1]), &bits).unwrap();
            let out = program.run_typed(&[(&input).into()]).unwrap();
            for i in [0, 2] {
                let actual = b.read_f32(out[i].as_tensor().unwrap()).unwrap();
                assert_eq!(
                    actual.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                    bits.iter()
                        .map(|&x| decode(dtype, x).to_bits())
                        .collect::<Vec<_>>()
                );
            }
            for i in [3, 5] {
                assert_eq!(b.read_low_bits(out[i].as_low().unwrap()).unwrap(), bits);
            }
            assert!(
                b.read_f32(out[1].as_tensor().unwrap())
                    .unwrap()
                    .iter()
                    .all(|&x| x == 0.)
            );
            assert!(
                b.read_low_bits(out[4].as_low().unwrap())
                    .unwrap()
                    .iter()
                    .all(|&x| x == 0)
            );
            traces(&program, 1);
        }
    }
}

#[test]
fn compiled_statistics_validate_before_appending_and_handle_empty_outputs() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    let x = graph.input(shape(&[2, 0])).unwrap();
    let low = graph.input_low(LowDtype::Bf16, shape(&[2, 0])).unwrap();
    let u = graph.input_u32(shape(&[2, 0])).unwrap();
    let mut other = b.program();
    let foreign = other.input(shape(&[2, 0])).unwrap();
    assert!(matches!(
        graph.moments(foreign, &[0], false),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        graph.moments(u, &[0], false),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        graph.moments_low(x, &[0], false),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        graph.moments(x, &[1], false),
        Err(MlxError::Contract(TensorError::EmptyReduction))
    ));
    assert!(graph.moments_low(low, &[0, 0], false).is_err());
    assert!(graph.logsumexp(x, &[2], false).is_err());
    for epsilon in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(graph.layer_norm(x, &[0], epsilon).is_err());
        assert!(graph.layer_norm_low(low, &[0], epsilon).is_err());
    }
    let mut values = outputs(&mut graph, x, &[0], false, 1e-5, false);
    values.extend(outputs(&mut graph, low, &[0], false, 1e-5, true));
    values.extend(low_outputs(&mut graph, low, &[0], false, 1e-5));
    let program = graph.compile(&values).unwrap();
    let x = b.upload_f32(shape(&[2, 0]), &[]).unwrap();
    let low = b.upload_low(LowDtype::Bf16, shape(&[2, 0]), &[]).unwrap();
    let u = b.upload_u32(shape(&[2, 0]), &[]).unwrap();
    let out = program
        .run_typed(&[(&x).into(), (&low).into(), (&u).into()])
        .unwrap();
    for value in &out[..12] {
        assert!(b.read_f32(value.as_tensor().unwrap()).unwrap().is_empty());
    }
    for value in &out[12..] {
        assert!(b.read_low_bits(value.as_low().unwrap()).unwrap().is_empty());
    }
    traces(&program, 1);
}

#[test]
fn compiled_statistics_outputs_compose_without_host_intermediates() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    let x = graph.input_low(LowDtype::Bf16, shape(&[2, 3, 5])).unwrap();
    let input = graph.permute(x, &[1, 0, 2]).unwrap();
    let norm = graph.layer_norm_low(input, &[2, 1], 1e-5).unwrap();
    let probabilities = graph.softmax_low_f32(norm, &[1, 2]).unwrap();
    let pair = graph.moments(probabilities, &[1, 2], false).unwrap();
    let program = graph
        .compile(&[probabilities, pair.mean, pair.variance])
        .unwrap();
    for replay in 0..2 {
        // A row-constant broadcast view must normalize to zero, then to 1/10.
        let raw = b
            .upload_low(
                LowDtype::Bf16,
                shape(&[1, 3, 1]),
                &[encode(LowDtype::Bf16, replay as f32), 0x4000, 0xc000],
            )
            .unwrap();
        let input = b.broadcast_low(&raw, shape(&[2, 3, 5])).unwrap();
        let out = program.run_typed(&[(&input).into()]).unwrap();
        assert_eq!(
            b.read_f32(out[0].as_tensor().unwrap()).unwrap(),
            vec![0.1; 30]
        );
        for value in b.read_f32(out[1].as_tensor().unwrap()).unwrap() {
            close(value, 0.1, 1e-30);
        }
        assert_eq!(
            b.read_f32(out[2].as_tensor().unwrap()).unwrap(),
            vec![0.; 3]
        );
        traces(&program, replay + 1);
    }
}

#[test]
fn compiled_statistics_multiple_unsorted_axes_keep_logical_output_order() {
    let Some(b) = backend() else { return };
    for dtype in [None, Some(LowDtype::F16), Some(LowDtype::Bf16)] {
        let mut graph = b.program();
        let source = match dtype {
            None => graph.input(shape(&[3, 5, 2])).unwrap(),
            Some(dtype) => graph.input_low(dtype, shape(&[3, 5, 2])).unwrap(),
        };
        let input = graph.permute(source, &[2, 0, 1]).unwrap();
        let mut values = outputs(&mut graph, input, &[2, 0], false, 0.125, dtype.is_some());
        // Normalize the three elementwise outputs to kept-axis-first order.
        // This is part of the resident graph, not a host output permutation.
        for i in [0, 1, 5] {
            let view = graph.permute(values[i], &[1, 2, 0]).unwrap();
            values[i] = graph.reshape(view, shape(&[3, 10])).unwrap();
        }
        let program = graph.compile(&values).unwrap();
        for replay in 0..2 {
            let rows: Vec<Vec<f32>> = (0..3)
                .map(|row| {
                    (0..10)
                        .map(|i| ((i * 3 + replay * 2) % 11) as f32 * 0.25 + row as f32 - 1.25)
                        .collect()
                })
                .collect();
            let data: Vec<_> = rows.iter().flatten().copied().collect();
            let out = match dtype {
                None => {
                    let input = b.upload_f32(shape(&[3, 5, 2]), &data).unwrap();
                    program.run_typed(&[(&input).into()]).unwrap()
                }
                Some(dtype) => {
                    let bits: Vec<_> = data.iter().map(|&x| encode(dtype, x)).collect();
                    let input = b.upload_low(dtype, shape(&[3, 5, 2]), &bits).unwrap();
                    program.run_typed(&[(&input).into()]).unwrap()
                }
            };
            check_rows(&b, &out, &rows, 0.125, &[3, 10], &[3]);
            traces(&program, replay + 1);
        }
    }
}
