//! Large-scene localized paint benchmark; synchronized frames, no OS presentation.
#[cfg(not(feature = "gpu"))]
fn main() {
    panic!("Use --features gpu");
}
#[cfg(feature = "gpu")]
fn main() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        println!("adapter={:?}", adapter.get_info());
        let mut renderer =
            forma::gpu::Renderer::new_profiled(&adapter, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .unwrap();
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
            let mut cpu = Vec::with_capacity(200);
            let mut gpu = Vec::with_capacity(200);
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
            let before = renderer.resource_stats();
            let full_bytes = model.gpu_paints().len() * 4;
            for i in 0..200 {
                model.pointer(if i % 2 == 0 { 50. } else { -1. }, 5., 0);
                let started = std::time::Instant::now();
                renderer
                    .draw(&model, &view, 1000, 2560, 1., true, false)
                    .unwrap();
                cpu.push(started.elapsed().as_secs_f64() * 1000.);
                renderer
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                if let Some(ns) = renderer.read_gpu_duration_ns().unwrap() {
                    gpu.push(ns / 1e6);
                }
            }
            let after = renderer.resource_stats();
            assert_eq!(before.geometry_uploads_total, after.geometry_uploads_total);
            assert_eq!(
                before.buffer_allocations_total,
                after.buffer_allocations_total
            );
            let sent = after.uploaded_bytes_total - before.uploaded_bytes_total;
            assert!(
                sent > 0 && sent < full_bytes as u64 * 200,
                "sent={sent} full={full_bytes}"
            );
            cpu.sort_by(f64::total_cmp);
            gpu.sort_by(f64::total_cmp);
            println!("controls={count} frames=200 paint_bytes/frame={} full_paint_bytes={full_bytes} CPU_submit_p50/p95={:.3}/{:.3}ms GPU_samples={} GPU_p50/p95={:?}/{:?}ms buffer_allocations=0 geometry_uploads=0",sent/200,cpu[100],cpu[190],gpu.len(),gpu.get(gpu.len()/2),gpu.get(gpu.len()*95/100));
        }
    });
}
