//! Alternating pairs: previous queue upload vs fresh mapped upload, followed
//! by the same GPU affine kernel and blocking completion. No CPU fallback.
//! cargo run --release -p compute-core --example bench_shared_upload
use compute_core::{
    ComputeRuntime,
    gpu_compute::{GpuContext, shared_memory_uploads, wgpu},
};
use std::time::{Duration, Instant};

fn main() {
    let context = GpuContext::new().expect("GPU required");
    let info = context.device.adapter_info();
    println!(
        "# adapter={} backend={:?} shared_uploads={}",
        info.name,
        info.backend,
        shared_memory_uploads(&context.device)
    );
    let runtime = ComputeRuntime::new(&context).unwrap();
    println!("bytes,pair,order,queue_upload_us,shared_upload_us,queue_total_us,shared_total_us");
    for len in [1024, 262144, 4194304] {
        let data: Vec<f32> = (0..len).map(|i| (i % 1024) as f32 * 0.25).collect();
        let output = runtime.zeros::<f32>(len).unwrap();
        let sample = |shared: bool| {
            let start = Instant::now();
            let input = if shared {
                runtime.upload(&data).unwrap()
            } else {
                // Exact previous upload implementation, including allocation validation.
                let array = runtime.zeros::<f32>(len).unwrap();
                runtime.write(&array, 0, &data).unwrap();
                array
            };
            let upload_us = start.elapsed().as_secs_f64() * 1e6;
            let mut program = runtime.program();
            program.affine_into(&input, 2.0, -1.0, &output).unwrap();
            let submission = program.submit();
            context
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(Duration::from_secs(10)),
                })
                .unwrap();
            let total_us = start.elapsed().as_secs_f64() * 1e6;
            // Full correctness check outside timing; also ensure every pair is drained.
            let actual = runtime
                .read(&output)
                .unwrap()
                .wait(Duration::from_secs(10))
                .unwrap();
            assert!(actual.iter().zip(&data).all(|(v, x)| *v == *x * 2.0 - 1.0));
            (upload_us, total_us)
        };
        for _ in 0..3 {
            sample(false);
            sample(true);
        }
        for pair in 0..12 {
            let (old, new) = if pair % 2 == 0 {
                (sample(false), sample(true))
            } else {
                let new = sample(true);
                (sample(false), new)
            };
            println!(
                "{},{},{},{:.3},{:.3},{:.3},{:.3}",
                len * 4,
                pair,
                if pair % 2 == 0 {
                    "queue-first"
                } else {
                    "shared-first"
                },
                old.0,
                new.0,
                old.1,
                new.1
            );
        }
    }
}
