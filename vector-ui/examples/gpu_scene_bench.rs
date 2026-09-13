//! Large-scene localized paint benchmark; synchronized frames, no OS presentation.
//! FORMA_BENCH_FRAMES=20..100000 (default 200); FORMA_BENCH_NO_TIMESTAMPS=1
//! disables profiling; FORMA_BENCH_STATIC=1 disables measured paint changes.
//! Missing process metrics print None, never zero. Rust allocation traffic
//! includes wgpu and the harness; RSS is not additive with unified GPU memory.
#[cfg(feature = "gpu")]
mod bench_support;
#[cfg(feature = "gpu")]
#[global_allocator]
static ALLOCATOR: bench_support::CountingAllocator = bench_support::CountingAllocator::new();
#[cfg(not(feature = "gpu"))]
fn main() {
    panic!("Use --features gpu");
}
#[cfg(feature = "gpu")]
fn main() {
    pollster::block_on(async {
        let frames: usize = std::env::var("FORMA_BENCH_FRAMES")
            .unwrap_or_else(|_| "200".into())
            .parse()
            .expect("FORMA_BENCH_FRAMES must be an integer");
        assert!(
            (20..=100_000).contains(&frames),
            "frames must be 20..100000"
        );
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        println!("adapter={:?}", adapter.get_info());
        let profiled = std::env::var_os("FORMA_BENCH_NO_TIMESTAMPS").is_none();
        let mut renderer = if profiled {
            forma::gpu::Renderer::new_profiled(&adapter, wgpu::TextureFormat::Rgba8Unorm).await
        } else {
            forma::gpu::Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm).await
        }
        .unwrap();
        let animate = std::env::var_os("FORMA_BENCH_STATIC").is_none();
        println!("timestamp profiling requested={profiled}; animate={animate}");
        for count in [100, 256] {
            let mut source = String::from(
                "component Scene { Frame { width:1000; height:2560; padding:0; gap:0;",
            );
            for _ in 0..count {
                source.push_str("Button { width:1000; height:10; }");
            }
            source.push_str("} }");
            let template = "component Button { Rectangle { background:Brush { color:#112233; hover:#aabbcc; transition:0ms; }; PointerArea { clicked -> events.clicked(); } } }";
            let mut model = forma::Runtime::from_sources(&source, template).unwrap();
            let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("large scene benchmark"),
                size: wgpu::Extent3d {
                    width: 1000,
                    height: 2560,
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
            let mut cpu = Vec::with_capacity(frames);
            let mut gpu = Vec::with_capacity(frames);
            for i in 0..20 {
                model.pointer(if i % 2 == 0 { 50. } else { -1. }, 5., 0);
                renderer
                    .draw(&model, &view, 1000, 2560, 1., true, false)
                    .unwrap();
                renderer
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
            }
            // Isolate warmed scene preparation from wgpu command encoding.
            use forma::display_list::RenderScene;
            let mut paint_scratch = Vec::new();
            let mut param_scratch = Vec::new();
            model.gpu_paints_into(&mut paint_scratch);
            model.gpu_params_into(1000, 2560, 1., false, &mut param_scratch);
            let prep_start = ALLOCATOR.snapshot();
            for i in 0..frames {
                model.pointer(if i % 2 == 0 { 50. } else { -1. }, 5., 0);
                std::hint::black_box(model.vector_snapshot(1., true));
                model.gpu_paints_into(&mut paint_scratch);
                model.gpu_params_into(1000, 2560, 1., false, &mut param_scratch);
                std::hint::black_box((&paint_scratch, &param_scratch));
            }
            let prep = ALLOCATOR.snapshot().delta_since(prep_start);
            println!("controls={count} warmed scene preparation allocations={} reallocations={} requested_bytes={}",prep.allocations,prep.reallocations,prep.requested_bytes);
            let before = renderer.resource_stats();
            let full_bytes = model.gpu_paints().len() * 4;
            let mut phase_allocations = [0u64; 3];
            let mut rss_samples = Vec::with_capacity(frames / 1000 + 1);
            let process_before = bench_support::ProcessSnapshot::capture().ok();
            rss_samples.push(process_before.map(|p| p.resident_bytes));
            let memory_before = ALLOCATOR.snapshot();
            for i in 0..frames {
                let phase_start = ALLOCATOR.snapshot();
                if animate {
                    model.pointer(if i % 2 == 0 { 50. } else { -1. }, 5., 0);
                }
                let after_pointer = ALLOCATOR.snapshot();
                phase_allocations[0] += after_pointer.allocations - phase_start.allocations;
                let started = std::time::Instant::now();
                renderer
                    .draw(&model, &view, 1000, 2560, 1., true, false)
                    .unwrap();
                let after_draw = ALLOCATOR.snapshot();
                phase_allocations[1] += after_draw.allocations - after_pointer.allocations;
                cpu.push(started.elapsed().as_secs_f64() * 1000.);
                renderer
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                if let Some(ns) = renderer.read_gpu_duration_ns().unwrap() {
                    gpu.push(ns / 1e6);
                }
                phase_allocations[2] += ALLOCATOR.snapshot().allocations - after_draw.allocations;
                if (i + 1) % 1000 == 0 {
                    rss_samples.push(
                        bench_support::ProcessSnapshot::capture()
                            .ok()
                            .map(|p| p.resident_bytes),
                    );
                }
            }
            let memory_after = ALLOCATOR.snapshot();
            let process_after = bench_support::ProcessSnapshot::capture().ok();
            let allocation_delta = memory_after.delta_since(memory_before);
            println!("controls={count} RSS_before={:?} RSS_after={:?} process_lifetime_peak_RSS={:?} (whole process; not additive with unified GPU memory)",process_before.map(|p|p.resident_bytes),process_after.map(|p|p.resident_bytes),process_after.map(|p|p.peak_resident_bytes));
            println!("controls={count} RSS samples start/every1000frames={rss_samples:?}");
            let after = renderer.resource_stats();
            assert_eq!(before.geometry_uploads_total, after.geometry_uploads_total);
            assert_eq!(
                before.buffer_allocations_total,
                after.buffer_allocations_total
            );
            let sent = after.uploaded_bytes_total - before.uploaded_bytes_total;
            assert!(
                if animate {
                    sent > 0 && sent < full_bytes as u64 * frames as u64
                } else {
                    sent == 0
                },
                "sent={sent} full={full_bytes}"
            );
            println!("controls={count} Rust allocations={} reallocations={} requested_bytes={} live_before={} live_after={} live_delta={} (includes wgpu and timestamp readback; excludes Metal/ObjC allocations)",allocation_delta.allocations,allocation_delta.reallocations,allocation_delta.requested_bytes,memory_before.live_bytes,memory_after.live_bytes,allocation_delta.live_bytes_change);
            println!("controls={count} allocations by phase pointer/draw-submit/wait-readback={phase_allocations:?}");
            cpu.sort_by(f64::total_cmp);
            gpu.sort_by(f64::total_cmp);
            println!("controls={count} frames={frames} paint_bytes/frame={} full_paint_bytes={full_bytes} CPU_submit_p50/p95={:.3}/{:.3}ms GPU_samples={} GPU_p50/p95={:?}/{:?}ms buffer_allocations=0 geometry_uploads=0",sent/frames as u64,cpu[frames/2],cpu[frames*95/100],gpu.len(),gpu.get(gpu.len()/2),gpu.get(gpu.len()*95/100));
        }
    });
}
