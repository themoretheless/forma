//! Direct packed statistics versus conversion into the stable f32 pipeline.
use compute_core::{
    ComputeRuntime,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{LowDtype, Shape, TensorLowBackend},
};
#[path = "support/low_bench.rs"]
mod low_bench;
use low_bench::{Result, measure_outputs};

#[derive(Clone, Copy, Debug)]
enum Op {
    Softmax,
    LogSoftmax,
    LogSumExp,
    Moments,
    LayerNorm,
}

fn reference(rows: usize, cols: usize, axis: usize, op: Op) -> Vec<Vec<f32>> {
    let groups = if axis == 1 { rows } else { cols };
    let count = if axis == 1 { cols } else { rows };
    let reduced = matches!(op, Op::LogSumExp | Op::Moments);
    let mut out = vec![vec![0.; if reduced { groups } else { rows * cols }]];
    if matches!(op, Op::Moments) {
        out.push(vec![0.; groups]);
    }
    for group in 0..groups {
        let index = |k| {
            if axis == 1 {
                group * cols + k
            } else {
                k * cols + group
            }
        };
        let values = (0..count)
            .map(|k| (index(k) % 9) as f64 / 4. - 1.)
            .collect::<Vec<_>>();
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let sum = values.iter().map(|&x| (x - max).exp()).sum::<f64>();
        let anchor = values[0];
        let mean_delta = values.iter().map(|x| x - anchor).sum::<f64>() / count as f64;
        let variance = values
            .iter()
            .map(|x| (x - anchor - mean_delta).powi(2))
            .sum::<f64>()
            / count as f64;
        match op {
            Op::LogSumExp => out[0][group] = (max + sum.ln()) as f32,
            Op::Moments => {
                out[0][group] = (anchor + mean_delta) as f32;
                out[1][group] = variance as f32;
            }
            _ => {
                for (k, &x) in values.iter().enumerate() {
                    out[0][index(k)] = match op {
                        Op::Softmax => ((x - max).exp() / sum) as f32,
                        Op::LogSoftmax => (x - max - sum.ln()) as f32,
                        Op::LayerNorm => {
                            ((x - anchor - mean_delta) / (variance + 0.125).sqrt()) as f32
                        }
                        _ => unreachable!(),
                    };
                }
            }
        }
    }
    out
}

fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={};31 rotated samples;200ms warmup;resident low inputs;f32 outputs;reused programs;separate unprofiled host+readback and GPU shared-pass timings;independent f64 reference and every result buffer checked after timing;epsilon=0.125",
        context.backend_label()
    );
    for (name, rows, cols, axis, transpose) in [
        ("small", 32usize, 33usize, 1usize, false),
        ("rows", 1024, 257, 1, false),
        ("long", 1, 131_077, 1, false),
        ("strided", 257, 1025, 1, true),
        ("nonlast", 17, 4099, 0, false),
    ] {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            let encodings = match dtype {
                LowDtype::F16 => [
                    0xbc00, 0xba00, 0xb800, 0xb400, 0, 0x3400, 0x3800, 0x3a00, 0x3c00,
                ],
                LowDtype::Bf16 => [
                    0xbf80, 0xbf40, 0xbf00, 0xbe80, 0, 0x3e80, 0x3f00, 0x3f40, 0x3f80,
                ],
            };
            let raw = (0..rows * cols)
                .map(|i| {
                    let logical = if transpose {
                        (i % rows) * cols + i / rows
                    } else {
                        i
                    };
                    encodings[logical % 9]
                })
                .collect::<Vec<_>>();
            let shape = Shape::new(if transpose {
                vec![cols, rows]
            } else {
                vec![rows, cols]
            })?;
            let mut input = rt.upload_low(dtype, shape, &raw)?;
            if transpose {
                input = input.permute(&[1, 0])?;
            }
            println!(
                "case={name} dtype={dtype:?} rows={rows} cols={cols} axis={axis} transpose={transpose} input_numel={} removed_f32_input_bytes={}",
                raw.len(),
                raw.len() * 4
            );
            for op in [
                Op::Softmax,
                Op::LogSoftmax,
                Op::LogSumExp,
                Op::Moments,
                Op::LayerNorm,
            ] {
                let mut baseline = rt.program();
                let expanded = baseline.tensor_cast_to_f32(&input)?;
                let mut direct = rt.program();
                let (old, new) = match op {
                    Op::Softmax => (
                        vec![baseline.tensor_softmax(&expanded, &[axis])?],
                        vec![direct.tensor_softmax_low_f32(&input, &[axis])?],
                    ),
                    Op::LogSoftmax => (
                        vec![baseline.tensor_log_softmax(&expanded, &[axis])?],
                        vec![direct.tensor_log_softmax_low_f32(&input, &[axis])?],
                    ),
                    Op::LogSumExp => (
                        vec![baseline.tensor_logsumexp(&expanded, &[axis], false)?],
                        vec![direct.tensor_logsumexp_low_f32(&input, &[axis], false)?],
                    ),
                    Op::Moments => {
                        let old = baseline.tensor_moments(&expanded, &[axis], false)?;
                        let new = direct.tensor_moments_low_f32(&input, &[axis], false)?;
                        (vec![old.mean, old.variance], vec![new.mean, new.variance])
                    }
                    Op::LayerNorm => (
                        vec![baseline.tensor_layer_norm(&expanded, &[axis], 0.125)?],
                        vec![direct.tensor_layer_norm_low_f32(&input, &[axis], 0.125)?],
                    ),
                };
                let expected = reference(rows, cols, axis, op);
                for outputs in [&old, &new] {
                    for tensor in outputs {
                        let shape = if matches!(op, Op::Moments | Op::LogSumExp) {
                            input.shape().reduce(&[axis], false)?
                        } else {
                            input.shape().clone()
                        };
                        assert_eq!(tensor.shape(), &shape);
                    }
                }
                measure_outputs(
                    &rt,
                    &timer,
                    &format!("{name}_{dtype:?}_{op:?}"),
                    &[baseline, direct],
                    &[
                        old.iter().map(|t| t.values().clone()).collect(),
                        new.iter().map(|t| t.values().clone()).collect(),
                    ],
                    &expected.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                    None,
                    |buffer, &actual, &expected| {
                        let relative =
                            matches!(op, Op::Softmax) || (matches!(op, Op::Moments) && buffer == 1);
                        let scale = expected.abs().max(if relative {
                            f32::MIN_POSITIVE / 7e-5
                        } else {
                            1.
                        });
                        actual.is_finite() && (actual - expected).abs() <= 7e-5 * scale
                    },
                )?;
            }
        }
    }
    Ok(())
}
