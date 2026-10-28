//! Frozen one-workgroup-per-output tensor reduction versus the resident API.
use compute_core::{
    Binding, ComputeRuntime, GpuTensor, Kernel,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::Shape,
    wgpu,
};
use std::time::{Duration, Instant};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn report(name: &str, mode: &str, values: &[f64]) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{name} {mode} median_ms={:.6} p90_ms={:.6} samples_ms={values:?}",
        sorted[sorted.len() / 2],
        sorted[(sorted.len() * 9).div_ceil(10) - 1]
    );
}
fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    let baseline = Kernel::new(
        rt.device(),
        "frozen tensor sum",
        include_str!("tensor_reduction_candidates/baseline.wgsl"),
        "main",
        &[
            Binding::StorageRead,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )?;
    println!(
        "backend={};31rotated samples;200msheld warmup;separate unprofiled host+finalreadback and GPUsharedpass timing;residentinput;f64checks everyrun",
        context.backend_label()
    );
    for (name, dims, permutation, axes, broadcast) in [
        ("small_rows", vec![16, 17], vec![1, 0], vec![0], None),
        (
            "transposed_full",
            vec![1024, 1025],
            vec![1, 0],
            vec![0, 1],
            None,
        ),
        (
            "two_axes",
            vec![8, 1025, 129],
            vec![1, 0, 2],
            vec![0, 2],
            None,
        ),
        (
            "sixtyfive_rows",
            vec![65, 257, 65],
            vec![0, 2, 1],
            vec![1, 2],
            None,
        ),
        (
            "broadcast_cap",
            vec![2, 1],
            vec![0, 1],
            vec![1],
            Some(vec![2, 2_000_003]),
        ),
    ] {
        let original = Shape::new(dims)?;
        let values: Vec<f32> = (0..original.numel())
            .map(|i| (i % 127) as f32 / 128.)
            .collect();
        let mut input =
            GpuTensor::from_array(rt.upload(&values)?, original)?.permute(&permutation)?;
        if let Some(dims) = broadcast {
            input = input.broadcast_to(Shape::new(dims)?)?;
        }
        let output_shape = input.shape().reduce(&axes, false)?;
        let count = output_shape.numel();
        let mut expected = vec![0.0f64; count];
        for flat in 0..input.shape().numel() {
            let mut remaining = flat;
            let mut index = 0;
            let mut stride = 1;
            for (axis, &dim) in input.shape().dims().iter().enumerate().rev() {
                let c = remaining % dim;
                remaining /= dim;
                if !axes.contains(&axis) {
                    index += c * stride;
                    stride *= dim;
                }
            }
            expected[index] += f64::from(values[input.layout().element_offset(flat)?]);
        }
        let old_output = rt.zeros::<f32>(count)?;
        let groups = (count as u32).min(65535);
        let mut remaining_axes = Vec::new();
        let mut reduced_axes = Vec::new();
        let mut reduction_count = 1usize;
        for (axis, (&dim, &stride)) in input
            .shape()
            .dims()
            .iter()
            .zip(input.layout().strides())
            .enumerate()
        {
            if axes.contains(&axis) {
                reduced_axes.extend([dim as u32, stride as u32]);
                reduction_count *= dim;
            } else {
                remaining_axes.extend([dim as u32, stride as u32]);
            }
        }
        let mut metadata = vec![
            count as u32,
            output_shape.rank() as u32,
            input.layout().offset() as u32,
            reduction_count as u32,
            axes.len() as u32,
            0,
            groups,
            0,
        ];
        metadata.extend(remaining_axes);
        metadata.extend(reduced_axes);
        let metadata = rt.upload(&metadata)?;
        let bindings = baseline.create_bind_group(
            rt.device(),
            &[
                metadata.view().raw(),
                input.values().view().raw(),
                old_output.view().raw(),
            ],
        );
        let mut program = rt.program();
        let output = program.tensor_sum(&input, &axes, false)?;
        let execute = |path: usize, profiled: bool| -> Result<f64> {
            let start = Instant::now();
            let mut encoder = rt.device().create_command_encoder(&Default::default());
            let record = |pass: &mut wgpu::ComputePass<'_>| {
                if path == 0 {
                    baseline.record_in_pass(pass, &bindings, groups)
                } else {
                    program.record_in_pass(pass)
                }
            };
            let mut timestamp = if profiled {
                Some(timer.record_compute(&mut encoder, "tensor reduction", record)?)
            } else {
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    record(&mut pass);
                }
                None
            };
            let mut ticket = rt.record_read(
                &mut encoder,
                if path == 0 {
                    &old_output
                } else {
                    output.values()
                },
            )?;
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
            for (&a, &e) in actual.iter().zip(&expected) {
                assert!(
                    (f64::from(a) - e).abs() <= 3e-5 * e.abs().max(1.0),
                    "{name} path{path}: {a} != {e}"
                );
            }
            if let Some(gpu) = elapsed {
                assert!(gpu <= wall + 0.1);
                Ok(gpu)
            } else {
                Ok(wall)
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
            "case={name} shape={:?} strides={:?} axes={axes:?} output_values={count} reduction_values={reduction_count}",
            input.shape().dims(),
            input.layout().strides()
        );
        for (path, label) in ["baseline", "production"].iter().enumerate() {
            report(label, "host", &host[path]);
            report(label, "gpu", &gpu[path]);
        }
    }
    Ok(())
}
