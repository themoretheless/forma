//! CPU encode+submit and synchronized GPU completion, excluding OS presentation.
#[cfg(not(feature = "gpu"))]
fn main() {
    panic!("Use --features gpu");
}
#[cfg(feature = "gpu")]
fn main() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let mut renderer =
            forma::gpu::Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .unwrap();
        let args: Vec<_> = std::env::args().collect();
        let source = args
            .get(1)
            .map(|p| std::fs::read_to_string(p).unwrap())
            .unwrap_or(forma::EXAMPLE.into());
        let template = args
            .get(2)
            .map(|p| std::fs::read_to_string(p).unwrap())
            .unwrap_or(forma::BUTTON_COMPONENT.into());
        let mut model = forma::Button::from_sources(&source, &template).unwrap();
        for (w, h) in [(800, 400), (3840, 2160)] {
            let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
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
            renderer.draw(&model, &view, w, h, 2., true, true).unwrap();
            renderer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            let mut submit = 0.;
            let mut completed = 0.;
            let uploads = renderer.uploads;
            for i in 0..30 {
                if i % 10 == 0 {
                    model.pointer(if i % 20 == 0 { 100. } else { -1. }, 90., 0);
                }
                model.tick(16.);
                let t = std::time::Instant::now();
                renderer.draw(&model, &view, w, h, 2., true, true).unwrap();
                submit += t.elapsed().as_secs_f64();
                renderer
                    .device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                completed += t.elapsed().as_secs_f64();
            }
            assert_eq!(uploads, renderer.uploads);
            println!(
                "{w}x{h}: CPU submit={:.3}ms; submit+GPU wait={:.3}ms; geometry reuploads=0",
                submit * 1000. / 30.,
                completed * 1000. / 30.
            );
        }
    });
}
