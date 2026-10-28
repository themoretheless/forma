//! Resident f32 versus packed f16/bf16 inputs, with the same f32 result.
//! Run on an otherwise idle GPU. Uploads, casts and recording are outside timing.
use compute_core::{
    ComputeRuntime, GpuTensor,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{LowDtype, Shape, TensorLowBackend},
};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn report(name: &str, mode: &str, samples: &[f64]) {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{name} {mode} median_ms={:.6} p90_ms={:.6} samples_ms={samples:?}",
        sorted[sorted.len() / 2],
        sorted[(sorted.len() * 9).div_ceil(10) - 1]
    );
}

fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={};31rotated samples;200msheld warmup;residentinputs;reusedprogram;f32output;separate unprofiled host+readback and GPUsharedpass timings;f64 reference everyrun",
        context.backend_label()
    );
    for (name, m, k, n, transpose) in [
        ("small_odd", 17, 19, 21, false),
        ("medium_odd", 128, 129, 127, false),
        ("strided_odd", 256, 257, 255, true),
    ] {
        let a: Vec<f32> = (0..m * k)
            .map(|i| ((i * 7 + i / k) % 17) as f32 / 8. - 1.)
            .collect();
        let b: Vec<f32> = (0..k * n)
            .map(|i| ((i * 11 + i / n) % 17) as f32 / 8. - 1.)
            .collect();
        let mut left = GpuTensor::from_array(rt.upload(&a)?, Shape::new(vec![m, k])?)?;
        if transpose {
            let physical: Vec<f32> = (0..k)
                .flat_map(|col| {
                    let a = &a;
                    (0..m).map(move |row| a[row * k + col])
                })
                .collect();
            left = GpuTensor::from_array(rt.upload(&physical)?, Shape::new(vec![k, m])?)?
                .permute(&[1, 0])?;
        }
        let right = GpuTensor::from_array(rt.upload(&b)?, Shape::new(vec![k, n])?)?;
        let mut expected = Vec::with_capacity(m * n);
        for row in 0..m {
            for col in 0..n {
                expected.push(
                    (0..k)
                        .map(|j| f64::from(a[row * k + j]) * f64::from(b[j * n + col]))
                        .sum::<f64>(),
                );
            }
        }
        let mut programs = Vec::new();
        let mut outputs = Vec::new();
        let mut input_bytes = vec![((a.len() + b.len()) * 4) as u64];
        let mut program = rt.program();
        outputs.push(program.tensor_matmul(&left, &right)?);
        programs.push(program);
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            // Preserve the physical transpose for all paths; casting a strided
            // input would materialize it and invalidate the layout comparison.
            let original = GpuTensor::from_array(
                left.values().clone(),
                if transpose {
                    Shape::new(vec![k, m])?
                } else {
                    left.shape().clone()
                },
            )?;
            let mut low_left = rt.cast_to_low(&original, dtype)?;
            if transpose {
                low_left = low_left.permute(&[1, 0])?;
            }
            let low_right = rt.cast_to_low(&right, dtype)?;
            input_bytes.push(low_left.allocation_bytes() + low_right.allocation_bytes());
            let mut program = rt.program();
            outputs.push(program.tensor_matmul_low_f32(&low_left, &low_right)?);
            programs.push(program);
        }
        let execute = |path: usize, profiled: bool| -> Result<f64> {
            let start = Instant::now();
            let mut encoder = rt.device().create_command_encoder(&Default::default());
            let mut timestamp = if profiled {
                Some(
                    timer.record_compute(&mut encoder, "resident tensor matmul", |pass| {
                        programs[path].record_in_pass(pass)
                    })?,
                )
            } else {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                programs[path].record_in_pass(&mut pass);
                None
            };
            let mut ticket = rt.record_read(&mut encoder, outputs[path].values())?;
            let submission = rt.queue().submit([if profiled {
                timer.finish(encoder)?
            } else {
                encoder.finish()
            }]);
            ticket.submitted(submission.clone());
            if let Some(t) = timestamp.as_mut() {
                t.submitted(submission);
            }
            let actual = ticket.wait(Duration::from_secs(30))?;
            let elapsed = if let Some(t) = timestamp {
                Some(t.wait(Duration::from_secs(30))?.elapsed_ns / 1e6)
            } else {
                None
            };
            let wall = start.elapsed().as_secs_f64() * 1000.;
            assert_eq!(actual.len(), expected.len());
            for (i, (&a, &e)) in actual.iter().zip(&expected).enumerate() {
                assert_eq!(f64::from(a), e, "{name} path{path} element{i}");
            }
            if let Some(gpu) = elapsed {
                assert!(gpu <= wall + 0.1);
                Ok(gpu)
            } else {
                Ok(wall)
            }
        };
        for path in 0..3 {
            execute(path, false)?;
            execute(path, true)?;
        }
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_millis(200) {
            for path in 0..3 {
                execute(path, false)?;
            }
        }
        let mut host = [Vec::new(), Vec::new(), Vec::new()];
        let mut gpu = [Vec::new(), Vec::new(), Vec::new()];
        for iteration in 0..31 {
            for offset in 0..3 {
                let path = (iteration + offset) % 3;
                if iteration % 2 == 0 {
                    host[path].push(execute(path, false)?);
                    gpu[path].push(execute(path, true)?);
                } else {
                    gpu[path].push(execute(path, true)?);
                    host[path].push(execute(path, false)?);
                }
            }
        }
        println!(
            "case={name} m={m} k={k} n={n} transpose_left={transpose} input_bytes={input_bytes:?}"
        );
        for (path, label) in ["f32", "packed_f16", "packed_bf16"].iter().enumerate() {
            report(label, "host", &host[path]);
            report(label, "gpu", &gpu[path]);
        }
    }
    Ok(())
}
