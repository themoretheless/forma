//! Matched resident low-storage benchmarks with validation after timing.
use compute_core::{ComputeProgram, ComputeRuntime, GpuArray, GpuElement, gpu_compute::GpuTimer};
use std::{
    fmt::Debug,
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// This shared module is compiled separately by examples using either wrapper.
#[allow(dead_code)]
pub fn measure<T: GpuElement + PartialEq + Debug>(
    rt: &ComputeRuntime,
    timer: &GpuTimer,
    name: &str,
    programs: &[ComputeProgram<'_>; 2],
    outputs: &[GpuArray<T>; 2],
    expected: &[T],
    counts: Option<(&[GpuArray<u32>; 2], u32)>,
) -> Result<()> {
    measure_outputs(
        rt,
        timer,
        name,
        programs,
        &[vec![outputs[0].clone()], vec![outputs[1].clone()]],
        &[expected],
        counts,
        |_, actual, expected| actual == expected,
    )
}

/// Read and validate every result buffer with identical work in both paths.
/// Statistical benchmarks provide a tolerance predicate; raw-storage fixtures
/// use `measure` above to retain exact equality. Validation remains untimed.
#[allow(clippy::too_many_arguments)]
pub fn measure_outputs<T: GpuElement + Debug>(
    rt: &ComputeRuntime,
    timer: &GpuTimer,
    name: &str,
    programs: &[ComputeProgram<'_>; 2],
    outputs: &[Vec<GpuArray<T>>; 2],
    expected: &[&[T]],
    counts: Option<(&[GpuArray<u32>; 2], u32)>,
    matches: impl Fn(usize, &T, &T) -> bool,
) -> Result<()> {
    assert_eq!(outputs[0].len(), expected.len());
    assert_eq!(outputs[1].len(), expected.len());
    let execute = |path: usize, profiled: bool| -> Result<f64> {
        let start = Instant::now();
        let mut encoder = rt.device().create_command_encoder(&Default::default());
        let mut timestamp = if profiled {
            Some(
                timer.record_compute(&mut encoder, "resident low computation", |pass| {
                    programs[path].record_in_pass(pass)
                })?,
            )
        } else {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            programs[path].record_in_pass(&mut pass);
            None
        };
        let mut tickets = outputs[path]
            .iter()
            .map(|output| rt.record_read(&mut encoder, output))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut count_ticket = counts
            .map(|(buffers, _)| rt.record_read(&mut encoder, &buffers[path]))
            .transpose()?;
        let submission = rt.queue().submit([if profiled {
            timer.finish(encoder)?
        } else {
            encoder.finish()
        }]);
        for ticket in &mut tickets {
            ticket.submitted(submission.clone());
        }
        if let Some(t) = count_ticket.as_mut() {
            t.submitted(submission.clone());
        }
        if let Some(t) = timestamp.as_mut() {
            t.submitted(submission);
        }
        let actuals = tickets
            .into_iter()
            .map(|ticket| ticket.wait(Duration::from_secs(30)))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let actual_count = count_ticket
            .map(|t| t.wait(Duration::from_secs(30)))
            .transpose()?;
        let gpu = if let Some(t) = timestamp {
            Some(t.wait(Duration::from_secs(30))?.elapsed_ns / 1e6)
        } else {
            None
        };
        let wall = start.elapsed().as_secs_f64() * 1000.;
        if let Some((_, count)) = counts {
            assert_eq!(actual_count.unwrap(), [count]);
        }
        for (buffer, (actual, expected)) in actuals.iter().zip(expected).enumerate() {
            assert_eq!(actual.len(), expected.len());
            for (i, (a, e)) in actual.iter().zip(*expected).enumerate() {
                assert!(
                    matches(buffer, a, e),
                    "{name} path{path} buffer{buffer} element{i}: {a:?} != {e:?}"
                );
            }
        }
        if let Some(gpu) = gpu {
            assert!(gpu <= wall + 0.1);
            Ok(gpu)
        } else {
            Ok(wall)
        }
    };
    for path in 0..2 {
        execute(path, true)?;
        execute(path, false)?;
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
    for (path, label) in ["cast_f32", "direct_low"].iter().enumerate() {
        for (mode, samples) in [("host", &host[path]), ("gpu", &gpu[path])] {
            let mut sorted = samples.clone();
            sorted.sort_by(f64::total_cmp);
            println!(
                "{name} {label} {mode} median_ms={:.6} p90_ms={:.6} samples_ms={samples:?}",
                sorted[15], sorted[27]
            );
        }
    }
    Ok(())
}
