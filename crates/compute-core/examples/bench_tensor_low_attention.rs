//! Direct packed Q/K/V attention versus three casts plus the same f32 planner.
use compute_core::{
    ComputeRuntime, GpuLowTensor, GpuTensor,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{AttentionMask, AttentionOptions, LowDtype, Shape, TensorLowBackend},
};
#[path = "support/low_bench.rs"]
mod low_bench;
use low_bench::{Result, measure_outputs};

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    hq: usize,
    hkv: usize,
    m: usize,
    n: usize,
    d: usize,
    dv: usize,
    strided_masked: bool,
}
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn data(n: usize, seed: usize) -> Vec<f32> {
    (0..n)
        .map(|i| ((i * 17 + i / 19 + seed * 7) % 9) as f32 / 4. - 1.)
        .collect()
}
fn input(
    rt: &ComputeRuntime,
    dtype: LowDtype,
    heads: usize,
    len: usize,
    depth: usize,
    strided: bool,
    values: &[f32],
) -> Result<GpuLowTensor> {
    let encodings = match dtype {
        LowDtype::F16 => [
            0xbc00, 0xba00, 0xb800, 0xb400, 0, 0x3400, 0x3800, 0x3a00, 0x3c00,
        ],
        LowDtype::Bf16 => [
            0xbf80, 0xbf40, 0xbf00, 0xbe80, 0, 0x3e80, 0x3f00, 0x3f40, 0x3f80,
        ],
    };
    let bits = (0..values.len())
        .map(|i| {
            let logical = if strided {
                let head = i / (len * depth);
                let channel = i / len % depth;
                let row = i % len;
                (head * len + row) * depth + channel
            } else {
                i
            };
            encodings[((values[logical] + 1.) * 4.) as usize]
        })
        .collect::<Vec<_>>();
    let input = rt.upload_low(
        dtype,
        shape(&if strided {
            [heads, depth, len]
        } else {
            [heads, len, depth]
        }),
        &bits,
    )?;
    Ok(if strided {
        input.permute(&[0, 2, 1])?
    } else {
        input
    })
}
fn kept(c: Case, row: usize, key: usize) -> bool {
    !c.strided_masked || (row != 0 && !key.is_multiple_of(11) && key < row + c.n - c.m)
}
fn reference(c: Case, q: &[f32], k: &[f32], v: &[f32]) -> Vec<f32> {
    let scale = f64::from((c.d as f32).sqrt().recip());
    let mut output = Vec::with_capacity(c.hq * c.m * c.dv);
    for head in 0..c.hq {
        let kh = head / (c.hq / c.hkv);
        for row in 0..c.m {
            let logits = (0..c.n)
                .map(|key| {
                    if !kept(c, row, key) {
                        return f64::NEG_INFINITY;
                    }
                    (0..c.d)
                        .map(|d| {
                            f64::from(q[(head * c.m + row) * c.d + d])
                                * f64::from(k[(kh * c.n + key) * c.d + d])
                        })
                        .sum::<f64>()
                        * scale
                })
                .collect::<Vec<_>>();
            let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if max == f64::NEG_INFINITY {
                output.extend(vec![0.; c.dv]);
                continue;
            }
            let weights = logits.iter().map(|x| (x - max).exp()).collect::<Vec<_>>();
            let sum = weights.iter().sum::<f64>();
            for col in 0..c.dv {
                output.push(
                    weights
                        .iter()
                        .enumerate()
                        .map(|(key, p)| p / sum * f64::from(v[(kh * c.n + key) * c.dv + col]))
                        .sum::<f64>() as f32,
                );
            }
        }
    }
    output
}
fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={};31 rotated samples;200ms warmup;resident low QKV;f32 output;reused programs;separate unprofiled host+readback and GPU shared-pass timings;independent dense f64 reference;every result checked after timing",
        context.backend_label()
    );
    let cases = [
        Case {
            name: "small",
            hq: 1,
            hkv: 1,
            m: 17,
            n: 65,
            d: 17,
            dv: 33,
            strided_masked: false,
        },
        Case {
            name: "decode",
            hq: 4,
            hkv: 2,
            m: 1,
            n: 4097,
            d: 64,
            dv: 64,
            strided_masked: false,
        },
        Case {
            name: "prefill",
            hq: 2,
            hkv: 2,
            m: 128,
            n: 257,
            d: 64,
            dv: 64,
            strided_masked: false,
        },
        Case {
            name: "wide_value_gqa",
            hq: 4,
            hkv: 2,
            m: 65,
            n: 257,
            d: 32,
            dv: 129,
            strided_masked: false,
        },
        Case {
            name: "strided_masked",
            hq: 6,
            hkv: 2,
            m: 7,
            n: 65,
            d: 17,
            dv: 33,
            strided_masked: true,
        },
    ];
    for c in cases {
        let qv = data(c.hq * c.m * c.d, 1);
        let kv = data(c.hkv * c.n * c.d, 2);
        let vv = data(c.hkv * c.n * c.dv, 3);
        let expected = reference(c, &qv, &kv, &vv);
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            let q = input(&rt, dtype, c.hq, c.m, c.d, c.strided_masked, &qv)?;
            let k = input(&rt, dtype, c.hkv, c.n, c.d, c.strided_masked, &kv)?;
            let v = input(&rt, dtype, c.hkv, c.n, c.dv, c.strided_masked, &vv)?;
            let mask = if c.strided_masked {
                let bits = (0..c.n * c.m)
                    .map(|i| {
                        let key = i / c.m;
                        let row = i % c.m;
                        if row != 0 && !key.is_multiple_of(11) {
                            u32::MAX
                        } else {
                            0
                        }
                    })
                    .collect::<Vec<_>>();
                Some(
                    GpuTensor::from_array(rt.upload(&bits)?, shape(&[c.n, c.m]))?
                        .permute(&[1, 0])?,
                )
            } else {
                None
            };
            let options = AttentionOptions {
                scale: None,
                causal: c.strided_masked.then_some((c.n - c.m - 1) as i32),
            };
            let attention_mask = || match &mask {
                Some(mask) => AttentionMask::Keep(mask),
                None => AttentionMask::None,
            };
            let mut baseline = rt.program();
            let qf = baseline.tensor_cast_to_f32(&q)?;
            let kf = baseline.tensor_cast_to_f32(&k)?;
            let vf = baseline.tensor_cast_to_f32(&v)?;
            let old = baseline.tensor_attention(&qf, &kf, &vf, attention_mask(), options)?;
            let mut direct = rt.program();
            let new = direct.tensor_attention_low_f32(&q, &k, &v, attention_mask(), options)?;
            for output in [&old, &new] {
                assert_eq!(output.shape(), &shape(&[c.hq, c.m, c.dv]));
            }
            println!(
                "case={} dtype={dtype:?} Hq={} Hkv={} Lq={} Lk={} D={} Dv={} strided_masked={} removed_f32_input_bytes={}",
                c.name,
                c.hq,
                c.hkv,
                c.m,
                c.n,
                c.d,
                c.dv,
                c.strided_masked,
                (qv.len() + kv.len() + vv.len()) * 4
            );
            measure_outputs(
                &rt,
                &timer,
                &format!("{}_{dtype:?}", c.name),
                &[baseline, direct],
                &[vec![old.values().clone()], vec![new.values().clone()]],
                &[&expected],
                None,
                |_, &a, &e| a.is_finite() && (a - e).abs() <= 3e-4 * e.abs().max(1.),
            )?;
        }
    }
    Ok(())
}
