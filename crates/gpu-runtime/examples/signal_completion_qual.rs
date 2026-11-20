//! Signal-driven completion latency and RSS qualification over 2,000 real Metal submissions.

use gpu_runtime as wgpu;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn sample_rss_kb() -> Option<u64> {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .ok()
}

fn block_on_scope(
    device: &wgpu::Device,
    fut: impl Future<Output = Option<wgpu::Error>>,
) -> Option<wgpu::Error> {
    struct ThreadWaker(std::thread::Thread);
    impl std::task::Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut pinned = std::pin::pin!(fut);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let std::task::Poll::Ready(out) = pinned.as_mut().poll(&mut cx) {
            return out;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        assert!(Instant::now() < deadline, "scope completion timed out");
        std::thread::park_timeout(Duration::from_millis(2));
    }
}

fn percentile_us(sorted_us: &[u64], pct: f64) -> u64 {
    if sorted_us.is_empty() {
        return 0;
    }
    let idx = ((sorted_us.len() - 1) as f64 * pct).round() as usize;
    sorted_us[idx.min(sorted_us.len() - 1)]
}

fn main() {
    let instance = wgpu::Instance::default();
    let mut noop_cx = std::task::Context::from_waker(std::task::Waker::noop());
    let adapter = match std::pin::pin!(instance.request_adapter(&Default::default()))
        .as_mut()
        .poll(&mut noop_cx)
    {
        std::task::Poll::Ready(Ok(adapter)) => adapter,
        _ => {
            println!("{{\"status\":\"skipped\",\"reason\":\"no_metal_adapter\"}}");
            return;
        }
    };
    let std::task::Poll::Ready(Ok((device, queue))) =
        std::pin::pin!(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .as_mut()
            .poll(&mut noop_cx)
    else {
        panic!("request_device failed");
    };

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("signal_qual_buf"),
        size: 4096,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    const N: usize = 2000;
    let mut latencies_us = Vec::with_capacity(N);
    let rss_before_kb = sample_rss_kb().unwrap_or(0);
    let mut rss_mid_kb = rss_before_kb;

    for i in 0..N {
        let start = Instant::now();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.clear_buffer(&buffer, 0, None);
        queue.submit([encoder.finish()]);

        let work_done = Arc::new(AtomicBool::new(false));
        let work_flag = work_done.clone();
        queue.on_submitted_work_done(move || work_flag.store(true, Ordering::Release));

        let mapped = Arc::new(AtomicBool::new(false));
        let map_flag = mapped.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |res| {
            res.expect("map_async failed");
            map_flag.store(true, Ordering::Release);
        });

        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert!(work_done.load(Ordering::Acquire));
        assert!(mapped.load(Ordering::Acquire));
        assert!(block_on_scope(&device, scope.pop()).is_none());
        latencies_us.push(start.elapsed().as_micros() as u64);

        if i == (N / 2) - 1 {
            rss_mid_kb = sample_rss_kb().unwrap_or(rss_before_kb);
        }
    }
    let rss_after_kb = sample_rss_kb().unwrap_or(rss_mid_kb);

    latencies_us.sort_unstable();
    let p50 = percentile_us(&latencies_us, 0.50);
    let p95 = percentile_us(&latencies_us, 0.95);
    let p99 = percentile_us(&latencies_us, 0.99);
    println!(
        "{{\"status\":\"ok\",\"submissions\":{N},\"p50_us\":{p50},\"p95_us\":{p95},\"p99_us\":{p99},\"rss_before_kb\":{rss_before_kb},\"rss_mid_kb\":{rss_mid_kb},\"rss_after_kb\":{rss_after_kb}}}"
    );
}
