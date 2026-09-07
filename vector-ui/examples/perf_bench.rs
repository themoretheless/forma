//! Isolated-process measurement. No OS window/compositor; throughput is NOT display FPS.
mod bench_support;
use bench_support::{CountingAllocator, ProcessSnapshot};
use std::{hint::black_box, time::Instant};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

fn stats(values: &[f64]) -> (f64, f64, f64, f64) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let p = |q: f64| {
        sorted[((sorted.len() as f64 * q).ceil() as usize)
            .saturating_sub(1)
            .min(sorted.len() - 1)]
    };
    (
        values.iter().sum::<f64>() / values.len() as f64,
        p(0.5),
        p(0.95),
        p(0.99),
    )
}

#[cfg(not(feature = "gpu"))]
fn main() {
    panic!("Build perf_bench with --features gpu");
}

#[cfg(feature = "gpu")]
fn main() {
    pollster::block_on(run());
}

#[cfg(feature = "gpu")]
async fn run() {
    use forma::{gpu::Renderer, Runtime as Button};
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 8,
        "perf_bench source template gpu|cpu animation|resize|forced|idle width height frames"
    );
    let source = std::fs::read_to_string(&args[1]).unwrap();
    let template = std::fs::read_to_string(&args[2]).unwrap();
    let backend = &args[3];
    let mode = &args[4];
    assert!(["gpu", "cpu"].contains(&backend.as_str()));
    assert!(["animation", "resize", "forced", "idle"].contains(&mode.as_str()));
    let w: u32 = args[5].parse().unwrap();
    let h: u32 = args[6].parse().unwrap();
    let frames: usize = args[7].parse().unwrap();
    assert!(frames >= 20 && frames <= 10000 && w >= 320 && h >= 200 && w <= 4096 && h <= 2160);
    let process_initial = ProcessSnapshot::capture().unwrap();
    let init = Instant::now();
    let mut model = Button::from_sources(&source, &template).unwrap();
    let bounds = model.bounds();
    let mut renderer = None;
    let mut target = None;
    let mut gpu_adapter = None;
    let mut adapter_name = String::from("CPU retained XRGB");
    if backend == "gpu" {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&Default::default())
            .await
            .expect("GPU required, no silent fallback");
        adapter_name = adapter.get_info().name;
        let r = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        target = Some(make_target(&r, w, h));
        renderer = Some(r);
        gpu_adapter = Some(adapter);
    }
    let mut host = if backend == "cpu" {
        vec![0u32; (w * h) as usize]
    } else {
        Vec::new()
    };
    let tick = |i: usize, model: &mut Button| {
        if mode == "animation" {
            // Reverse every 4 frames: the entire measured phase stays animated.
            if i % 4 == 0 {
                model.pointer(
                    if i % 8 == 0 {
                        bounds[0] + bounds[2] * 0.5
                    } else {
                        -1.
                    },
                    bounds[1] + bounds[3] * 0.5,
                    0,
                );
            }
            model.tick(16.);
            assert!(
                model.is_animating(),
                "Animation fixture has no active transition"
            );
        }
    };
    let draw = |i: usize,
                model: &Button,
                renderer: &mut Option<Renderer>,
                target: &mut Option<(wgpu::Texture, wgpu::TextureView)>,
                host: &mut Vec<u32>| {
        let (cw, ch) = if mode == "resize" {
            (w - (i % 12) as u32 * 7, h - (i % 12) as u32 * 3)
        } else {
            (w, h)
        };
        if let Some(r) = renderer {
            if mode == "resize" {
                *target = Some(make_target(r, cw, ch));
            }
            r.draw(model, &target.as_ref().unwrap().1, cw, ch, 2., true, true)
                .unwrap();
        } else {
            model
                .paint_native(&mut host[..(cw * ch) as usize], cw, ch, 2.)
                .unwrap();
            black_box(&host);
        }
    };
    for i in 0..20 {
        tick(i, &mut model);
        draw(i, &model, &mut renderer, &mut target, &mut host);
        if let Some(r) = &renderer {
            r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
    }
    let init_ms = init.elapsed().as_secs_f64() * 1000.;
    let mut submit = vec![0.; frames];
    let mut completed = vec![0.; frames];
    let process_before = ProcessSnapshot::capture().unwrap();
    ALLOCATOR.reset_peak();
    let allocation_before = ALLOCATOR.snapshot();
    let uploads_before = renderer.as_ref().map_or(0, |r| r.uploads);
    let revision_before = model.visual_revision();
    let resource_before = renderer.as_ref().map(|r| r.resource_stats());
    let wall = Instant::now();
    if mode == "idle" {
        std::thread::sleep(std::time::Duration::from_secs(1));
    } else {
        for i in 0..frames {
            let frame = Instant::now();
            tick(i, &mut model);
            draw(i, &model, &mut renderer, &mut target, &mut host);
            submit[i] = frame.elapsed().as_secs_f64() * 1000.;
            if let Some(r) = &renderer {
                r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            }
            completed[i] = frame.elapsed().as_secs_f64() * 1000.;
        }
    }
    let wall_seconds = wall.elapsed().as_secs_f64();
    let allocation_after = ALLOCATOR.snapshot();
    let process_after = ProcessSnapshot::capture().unwrap();
    if mode == "animation" {
        // 8-bit color rounding can leave the first eased step unchanged.
        assert!(
            model.visual_revision().wrapping_sub(revision_before) >= frames as u32 / 4,
            "Animation fixture did not visibly progress"
        );
    }
    let a = allocation_after.delta_since(allocation_before);
    let cpu_seconds = process_after.cpu_seconds - process_before.cpu_seconds;
    let actual_frames = if mode == "idle" { 0 } else { frames };
    let (mean, p50, p95, p99) = stats(&completed);
    let (submit_mean, _, submit_p95, _) = stats(&submit);
    // Timing readback is deliberately outside the measured CPU/allocator phase.
    let mut gpu_ms = Vec::new();
    let mut timestamp_supported = false;
    if mode != "idle" {
        if let Some(adapter) = &gpu_adapter {
            let mut r = Renderer::new_profiled(adapter, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .unwrap();
            let size = target.as_ref().unwrap().0.size();
            let timing_target = make_target(&r, size.width, size.height);
            // A different profiled renderer: primary measurements match normal app mode.
            r.draw(
                &model,
                &timing_target.1,
                size.width,
                size.height,
                2.,
                true,
                true,
            )
            .unwrap();
            r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            if r.gpu_timestamps_enabled() {
                timestamp_supported = true;
                for _ in 0..30 {
                    r.draw(
                        &model,
                        &timing_target.1,
                        size.width,
                        size.height,
                        2.,
                        true,
                        true,
                    )
                    .unwrap();
                    if let Some(ns) = r.read_gpu_duration_ns().unwrap() {
                        gpu_ms.push(ns / 1e6);
                    }
                }
            }
        }
    }
    let gpu = if gpu_ms.len() != 30 {
        "null".into()
    } else {
        let (m, p50, p95, p99) = stats(&gpu_ms);
        format!(
            "{{\"mean_ms\":{m},\"p50_ms\":{p50},\"p95_ms\":{p95},\"p99_ms\":{p99},\"samples\":{}}}",
            gpu_ms.len()
        )
    };
    let resource = renderer.as_ref().map(|r| r.resource_stats());
    let buffers = resource.as_ref().map_or(0, |r| r.buffer_bytes);
    let buffer_count = resource.as_ref().map_or(0, |r| r.buffer_count);
    let target_bytes = target.as_ref().map_or(0, |t| {
        let s = t.0.size();
        s.width as u64 * s.height as u64 * 4
    });
    let uploads = renderer.as_ref().map_or(0, |r| r.uploads) - uploads_before;
    let resource_delta = match (resource_before, resource) {
        (Some(before), Some(after)) => format!("{{\"buffer_allocations\":{},\"buffer_allocation_bytes\":{},\"uploaded_bytes\":{},\"tile_uploads\":{},\"cpu_geometry_cache_bytes\":{},\"cpu_shared_geometry_bytes\":{},\"cpu_paint_scratch_bytes\":{}}}",after.buffer_allocations_total-before.buffer_allocations_total,after.buffer_allocation_bytes_total-before.buffer_allocation_bytes_total,after.uploaded_bytes_total-before.uploaded_bytes_total,after.tile_uploads_total-before.tile_uploads_total,after.cpu_geometry_cache_bytes,after.cpu_shared_geometry_bytes,after.cpu_paint_scratch_bytes),
        _ => "null".into(),
    };
    let diagnostics=format!("{{\"timestamp_supported\":{timestamp_supported},\"valid_gpu_samples\":{},\"rss_lifetime_peak_bytes\":{},\"rust_deallocations\":{},\"gpu_phase\":{resource_delta}}}",gpu_ms.len(),process_after.peak_resident_bytes,a.deallocations);
    println!("{{\"backend\":{backend:?},\"adapter\":{adapter_name:?},\"mode\":{mode:?},\"width\":{w},\"height\":{h},\"scale\":2,\"frames\":{actual_frames},\"init_and_warmup_ms\":{init_ms},\"wall_seconds\":{wall_seconds},\"render_throughput_fps\":{},\"completed_ms\":{{\"mean\":{mean},\"p50\":{p50},\"p95\":{p95},\"p99\":{p99}}},\"cpu_submit_ms\":{{\"mean\":{submit_mean},\"p95\":{submit_p95}}},\"cpu_seconds\":{cpu_seconds},\"cpu_percent_one_core\":{},\"rss_initial_bytes\":{},\"rss_before_bytes\":{},\"rss_after_bytes\":{},\"rust_allocations\":{},\"rust_reallocations\":{},\"rust_requested_bytes\":{},\"rust_live_before_bytes\":{},\"rust_live_after_bytes\":{},\"rust_peak_live_bytes\":{},\"owned_gpu_buffer_bytes\":{buffers},\"owned_gpu_buffer_count\":{buffer_count},\"target_payload_bytes\":{target_bytes},\"geometry_uploads\":{uploads},\"gpu_pass\":{gpu},\"diagnostics\":{diagnostics}}}",actual_frames as f64/wall_seconds,cpu_seconds/wall_seconds*100.,process_initial.resident_bytes,process_before.resident_bytes,process_after.resident_bytes,a.allocations,a.reallocations,a.requested_bytes,allocation_before.live_bytes,allocation_after.live_bytes,allocation_after.peak_live_bytes);
}

#[cfg(feature = "gpu")]
fn make_target(
    r: &forma::gpu::Renderer,
    w: u32,
    h: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("benchmark target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    (texture, view)
}
