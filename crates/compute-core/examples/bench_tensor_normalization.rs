//! Resident softmax: composition of public tensor primitives vs fused row stats.
use compute_core::{
    BinaryOp, ComputeRuntime, GpuTensor, UnaryOp,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{ReduceOp, Shape},
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
        "backend={};31rotated samples;200msheld warmup;residentinputs;reusedprograms;separate unprofiled host+readback and GPUsharedpass timings;f64reference everyrun",
        context.backend_label()
    );
    for (name, dims, transpose) in [
        ("small_rows", [32, 33], false),
        ("long_row", [1, 1_048_579], false),
        ("many_rows", [4096, 257], false),
        ("strided_rows", [1024, 1025], true),
    ] {
        let values: Vec<f32> = (0..dims[0] * dims[1])
            .map(|i| ((i * 17 + i / 257) % 127) as f32 / 16. - 4.)
            .collect();
        let mut input = GpuTensor::from_array(rt.upload(&values)?, Shape::new(dims.to_vec())?)?;
        if transpose {
            input = input.permute(&[1, 0])?;
        }
        let rows = input.shape().dims()[0];
        let columns = input.shape().dims()[1];
        let mut expected = Vec::with_capacity(values.len());
        for row in 0..rows {
            let data: Vec<f64> = (0..columns)
                .map(|col| {
                    f64::from(values[input.layout().element_offset(row * columns + col).unwrap()])
                })
                .collect();
            let max = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let sum: f64 = data.iter().map(|&x| (x - max).exp()).sum();
            expected.extend(data.iter().map(|&x| (x - max).exp() / sum));
        }
        let mut composed = rt.program();
        let max = composed.tensor_reduce(ReduceOp::Max, &input, &[1], true)?;
        let shifted = composed.tensor_binary(BinaryOp::Subtract, &input, &max)?;
        let exp = composed.tensor_unary(UnaryOp::Exp, &shifted)?;
        let sum = composed.tensor_sum(&exp, &[1], true)?;
        let old_output = composed.tensor_binary(BinaryOp::Divide, &exp, &sum)?;
        let mut fused = rt.program();
        let new_output = fused.tensor_softmax(&input, &[1])?;
        let programs = [composed, fused];
        let outputs = [old_output, new_output];
        let execute = |path: usize, profiled: bool| -> Result<f64> {
            let start = Instant::now();
            let mut encoder = rt.device().create_command_encoder(&Default::default());
            let mut timestamp = if profiled {
                Some(
                    timer.record_compute(&mut encoder, "tensor softmax", |pass| {
                        programs[path].record_in_pass(pass)
                    })?,
                )
            } else {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                programs[path].record_in_pass(&mut pass);
                None
            };
            let mut readback = rt.record_read(&mut encoder, outputs[path].values())?;
            let submitted = rt.queue().submit([if profiled {
                timer.finish(encoder)?
            } else {
                encoder.finish()
            }]);
            readback.submitted(submitted.clone());
            if let Some(t) = timestamp.as_mut() {
                t.submitted(submitted);
            }
            let actual = readback.wait(Duration::from_secs(30))?;
            let gpu = if let Some(t) = timestamp {
                Some(t.wait(Duration::from_secs(30))?.elapsed_ns / 1e6)
            } else {
                None
            };
            let host = start.elapsed().as_secs_f64() * 1000.;
            assert_eq!(actual.len(), expected.len());
            for (i, (&a, &e)) in actual.iter().zip(&expected).enumerate() {
                assert!(
                    a.is_finite() && (f64::from(a) - e).abs() <= 7e-5 * e.max(1e-30),
                    "{name} path{path} element{i}: {a} != {e}"
                );
            }
            if let Some(gpu) = gpu {
                assert!(gpu <= host + 0.1);
                Ok(gpu)
            } else {
                Ok(host)
            }
        };
        for path in 0..2 {
            execute(path, false)?;
            execute(path, true)?;
        }
        let warm = Instant::now();
        while warm.elapsed() < Duration::from_millis(200) {
            for path in 0..2 {
                execute(path, false)?;
            }
        }
        let mut host = [Vec::new(), Vec::new()];
        let mut gpu = [Vec::new(), Vec::new()];
        for iteration in 0..31 {
            for offset in 0..2 {
                let path = (iteration + offset) % 2;
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
            "case={name} shape={:?} strides={:?} axis=1 values={}",
            input.shape().dims(),
            input.layout().strides(),
            expected.len()
        );
        for (path, label) in ["composition", "row_stats"].iter().enumerate() {
            report(label, "host", &host[path]);
            report(label, "gpu", &gpu[path]);
        }
    }
    Ok(())
}
