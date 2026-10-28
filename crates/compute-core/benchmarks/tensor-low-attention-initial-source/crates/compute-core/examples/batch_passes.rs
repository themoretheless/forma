//! Compare separate compute passes with a shared pass over identical dispatches.
//! cargo run --release --offline -p compute-core --example batch_passes
use compute_core::{
    Binding, ComputeBatch, Kernel,
    gpu_compute::{ByteReadback, GpuContext},
    shaders, storage_f32_zeroed, uniform_f32, wgpu,
};
use std::time::{Duration, Instant};

fn median_us(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2] * 1e6
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = GpuContext::new().ok_or("GPU adapter required")?;
    let device = &context.device;
    let queue = &context.queue;
    let kernel = Kernel::new(
        device,
        "pass benchmark affine",
        shaders::AFFINE_WGSL,
        "main",
        &[
            Binding::Uniform,
            Binding::StorageRead,
            Binding::StorageReadWrite,
        ],
    )?;
    println!(
        "backend={}; median of 21 paired samples after 3 warmups; no uploads in timed runs",
        context.backend_label()
    );
    println!(
        "record=host encoder creation + dispatch encoding + finish; total=record + one submit + wait + 4-byte readback; no GPU timestamps"
    );
    for (count, steps) in [(4096u32, 4usize), (4096, 32), (4096, 128), (1_000_003, 32)] {
        let first = storage_f32_zeroed(device, queue, count as usize);
        let second = storage_f32_zeroed(device, queue, count as usize);
        let params = uniform_f32(device, queue, &[f32::from_bits(count), 1.0, 1.0, 0.0]);
        let forward = kernel.create_bind_group(device, &[&params, &first, &second]);
        let backward = kernel.create_bind_group(device, &[&params, &second, &first]);
        let mut batch = ComputeBatch::new();
        let groups = kernel.workgroup_count(count);
        let bindings: Vec<_> = (0..steps)
            .map(|i| if i % 2 == 0 { &forward } else { &backward })
            .collect();
        for bind in &bindings {
            batch.push(&kernel, bind, groups);
        }
        // Flush uniform uploads before timing and wait for initialization work.
        let initialized = queue.submit([]);
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(initialized),
            timeout: Some(Duration::from_secs(10)),
        })?;
        let record = |encoder: &mut wgpu::CommandEncoder, shared: bool| {
            if shared {
                batch.record(encoder);
            } else {
                for bind in &bindings {
                    kernel.record_dispatch(encoder, bind, groups);
                }
            }
        };
        let mut expected = 0.0f32;
        let mut encoding = [Vec::new(), Vec::new()];
        let mut total = [Vec::new(), Vec::new()];
        for iteration in 0..24 {
            for offset in 0..2 {
                let path = (iteration + offset) % 2;
                let start = Instant::now();
                let mut encoder = device.create_command_encoder(&Default::default());
                record(&mut encoder, path == 1);
                let commands = encoder.finish();
                let recorded = start.elapsed().as_secs_f64();
                drop(commands); // This host-only recording is never submitted.

                let start = Instant::now();
                let mut encoder = device.create_command_encoder(&Default::default());
                record(&mut encoder, path == 1);
                let mut readback = ByteReadback::copy_buffer(device, &mut encoder, &first, 0, 4)?;
                readback.submitted(queue.submit([encoder.finish()]));
                let bytes = readback.wait(Duration::from_secs(10))?;
                let elapsed = start.elapsed().as_secs_f64();
                expected += steps as f32;
                assert_eq!(f32::from_le_bytes(bytes.try_into().unwrap()), expected);
                if iteration >= 3 {
                    encoding[path].push(recorded);
                    total[path].push(elapsed);
                }
            }
        }
        let [separate_encode, shared_encode] = encoding;
        let [separate_total, shared_total] = total;
        println!(
            "n={count}, steps={steps}: record separate={:.2} us shared={:.2} us; total separate={:.2} us shared={:.2} us",
            median_us(separate_encode),
            median_us(shared_encode),
            median_us(separate_total),
            median_us(shared_total)
        );
    }
    Ok(())
}
