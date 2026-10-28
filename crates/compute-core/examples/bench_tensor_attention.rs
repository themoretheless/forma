//! Resident attention: public matmul/softmax composition versus streaming WGSL.
use compute_core::{
    BinaryOp, ComputeRuntime, GpuTensor,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{AttentionMask, AttentionOptions, Shape},
};
use std::time::{Duration, Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn shape(dims: &[usize]) -> Shape {
    Shape::new(dims.to_vec()).unwrap()
}
fn data(n: usize, seed: usize) -> Vec<f32> {
    (0..n)
        .map(|i| ((i * 17 + i / 19 + seed * 7) % 43) as f32 / 16. - 1.25)
        .collect()
}
fn report(path: &str, mode: &str, values: &[f64]) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{path} {mode} median_ms={:.6} p90_ms={:.6} samples_ms={values:?}",
        sorted[sorted.len() / 2],
        sorted[(sorted.len() * 9).div_ceil(10) - 1]
    );
}
fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={}; 31 rotating samples; 200 ms warmup; resident inputs and reused programs; separate host/readback and GPU timings; independent f64 reference",
        context.backend_label()
    );
    // Hq, Hkv, Lq, Lk, D, Dv. GQA composition uses broadcast views, not copies.
    for (name, hq, hkv, m, n, d, dv) in [
        ("small", 1, 1, 17, 65, 17, 33),
        ("decode", 4, 2, 1, 4097, 64, 64),
        ("prefill", 2, 2, 128, 257, 64, 64),
        ("wide_value_gqa", 4, 2, 65, 257, 32, 129),
    ] {
        let qv = data(hq * m * d, 1);
        let kv = data(hkv * n * d, 2);
        let vv = data(hkv * n * dv, 3);
        let query = GpuTensor::from_array(rt.upload(&qv)?, shape(&[hq, m, d]))?;
        let key = GpuTensor::from_array(rt.upload(&kv)?, shape(&[hkv, n, d]))?;
        let value = GpuTensor::from_array(rt.upload(&vv)?, shape(&[hkv, n, dv]))?;
        let scale = (d as f32).sqrt().recip();
        let scale_tensor = GpuTensor::from_array(rt.upload(&[scale])?, shape(&[]))?;
        let mut expected = Vec::with_capacity(hq * m * dv);
        for head in 0..hq {
            let kh = head / (hq / hkv);
            for row in 0..m {
                let logits: Vec<f64> = (0..n)
                    .map(|key| {
                        (0..d)
                            .map(|col| {
                                f64::from(qv[(head * m + row) * d + col])
                                    * f64::from(kv[(kh * n + key) * d + col])
                            })
                            .sum::<f64>()
                            * f64::from(scale)
                    })
                    .collect();
                let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let weights: Vec<f64> = logits.iter().map(|&x| (x - max).exp()).collect();
                let sum: f64 = weights.iter().sum();
                for col in 0..dv {
                    expected.push(
                        weights
                            .iter()
                            .enumerate()
                            .map(|(key, &p)| p / sum * f64::from(vv[(kh * n + key) * dv + col]))
                            .sum::<f64>(),
                    );
                }
            }
        }
        let qg = query.reshape(shape(&[hkv, hq / hkv, m, d]))?;
        let kg = key
            .reshape(shape(&[hkv, 1, n, d]))?
            .permute(&[0, 1, 3, 2])?;
        let vg = value.reshape(shape(&[hkv, 1, n, dv]))?;
        let mut composition = rt.program();
        let scores = composition.tensor_matmul(&qg, &kg)?;
        let scaled = composition.tensor_binary(BinaryOp::Multiply, &scores, &scale_tensor)?;
        let probabilities = composition.tensor_softmax(&scaled, &[3])?;
        let composed = composition
            .tensor_matmul(&probabilities, &vg)?
            .reshape(shape(&[hq, m, dv]))?;
        let mut streaming = rt.program();
        let fused = streaming.tensor_attention(
            &query,
            &key,
            &value,
            AttentionMask::None,
            AttentionOptions::default(),
        )?;
        let programs = [composition, streaming];
        let outputs = [composed, fused];
        let run = |path: usize, profiled: bool| -> Result<f64> {
            let started = Instant::now();
            let mut encoder = rt.device().create_command_encoder(&Default::default());
            let mut timestamp = if profiled {
                Some(timer.record_compute(&mut encoder, "attention", |pass| {
                    programs[path].record_in_pass(pass)
                })?)
            } else {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                programs[path].record_in_pass(&mut pass);
                None
            };
            let mut readback = rt.record_read(&mut encoder, outputs[path].values())?;
            let submission = rt.queue().submit([if profiled {
                timer.finish(encoder)?
            } else {
                encoder.finish()
            }]);
            readback.submitted(submission.clone());
            if let Some(t) = timestamp.as_mut() {
                t.submitted(submission);
            }
            let actual = readback.wait(Duration::from_secs(30))?;
            let gpu = if let Some(t) = timestamp {
                Some(t.wait(Duration::from_secs(30))?.elapsed_ns / 1e6)
            } else {
                None
            };
            let host = started.elapsed().as_secs_f64() * 1000.;
            for (i, (&a, &e)) in actual.iter().zip(&expected).enumerate() {
                assert!(
                    a.is_finite() && (f64::from(a) - e).abs() <= 3e-4 * e.abs().max(1.0),
                    "{name} path{path} [{i}]: {a} != {e}"
                );
            }
            assert_eq!(actual.len(), expected.len());
            if let Some(gpu) = gpu {
                assert!(gpu <= host + 0.1);
                Ok(gpu)
            } else {
                Ok(host)
            }
        };
        for path in 0..2 {
            run(path, false)?;
            run(path, true)?;
        }
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_millis(200) {
            for path in 0..2 {
                run(path, false)?;
            }
        }
        let mut host = [Vec::new(), Vec::new()];
        let mut gpu = [Vec::new(), Vec::new()];
        for iteration in 0..31 {
            for offset in 0..2 {
                let path = (iteration + offset) % 2;
                if iteration % 2 == 0 {
                    host[path].push(run(path, false)?);
                    gpu[path].push(run(path, true)?);
                } else {
                    gpu[path].push(run(path, true)?);
                    host[path].push(run(path, false)?);
                }
            }
        }
        println!(
            "case={name} Hq={hq} Hkv={hkv} Lq={m} Lk={n} D={d} Dv={dv} composition_score_arrays_bytes={} streaming_score_arrays_bytes=0",
            3 * hq * m * n * 4
        );
        for (path, label) in ["composition", "streaming"].iter().enumerate() {
            report(label, "host", &host[path]);
            report(label, "gpu", &gpu[path]);
        }
    }
    Ok(())
}
