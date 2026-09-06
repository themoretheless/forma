//! Requires a real GPU. Fails (does not silently skip) if adapter/validation fails.
#[cfg(feature = "gpu")]
fn main() {
    pollster::block_on(run());
}
#[cfg(not(feature = "gpu"))]
fn main() {
    panic!("Run with --features gpu");
}
#[cfg(feature = "gpu")]
async fn run() {
    use forma_vector::{gpu::Renderer, Button};
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&Default::default())
        .await
        .expect("GPU adapter required");
    println!("GPU: {:?}", adapter.get_info());
    let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
        .await
        .expect("valid vector shader");
    let args: Vec<_> = std::env::args().collect();
    let source = args
        .get(1)
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or(forma_vector::EXAMPLE.into());
    let template = args
        .get(2)
        .map(|p| std::fs::read_to_string(p).unwrap())
        .unwrap_or(forma_vector::BUTTON_COMPONENT.into());
    let nested="component Demo { Frame { width:64; height:48; radius:12; clip:true; padding:2; background:#12345680; Scroll { Button { width:100; height:60; } } } }";
    let nested_template="component Button { Rectangle { radius:12; background:#ff000080; Border { width:2; background:#aabbcc80; } ContentClip { x:5; y:2; width:45; height:40; radius:5; } ContentShape { points:'0 0 70 0 70 50 0 50'; color:#abcdef80; } ContentText { x:1; y:2; width:55; height:30; text:'ОБ'; color:#ffffffa0; fontSize:16; } ContentClipEnd {} } }";
    for (name, source, template) in [
        ("input", source.as_str(), template.as_str()),
        ("nested-scroll", nested, nested_template),
        ("smooth-corner", "component Demo { Frame { width:64; height:64; padding:0; background:#000000; Button { width:64; height:64; } } }", "component Button { Rectangle { radius:24; background:#ffffff; } }"),
    ] {
        let mut model = Button::from_sources(source, template).unwrap();
        if model.scrollable() {
            model.scroll(10., 4.);
        }
        for (case_id, (w, h, s)) in [
            (400u32, 200u32, 1f32),
            (800, 400, 2.),
            (1200, 600, 2.),
            (300, 150, 2.),
            (800, 400, 2.),
            (500, 250, 1.25),
        ].into_iter().enumerate() {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let stride = (w * 4).div_ceil(256) * 256;
            let readback = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: stride as u64 * h as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            renderer
                .draw(
                    &model,
                    &texture.create_view(&Default::default()),
                    w,
                    h,
                    s,
                    true,
                    false,
                )
                .unwrap();
            let mut encoder = renderer.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            renderer.queue.submit(Some(encoder.finish()));
            let (send, recv) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| send.send(r).unwrap());
            renderer
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            recv.recv().unwrap().unwrap();
            let mapped = readback.slice(..).get_mapped_range();
            // Optional exact before/after GPU regression, independent of CPU AA.
            // Records are test artifacts, never consumed by the application.
            if let Ok(dir) = std::env::var("FORMA_GPU_GOLDEN_DIR") {
                let path=std::path::Path::new(&dir).join(format!("{name}-{case_id}-{w}x{h}.rgba"));
                let mut packed=Vec::with_capacity((w*h*4)as usize);
                for y in 0..h as usize {packed.extend_from_slice(&mapped[y*stride as usize..y*stride as usize+w as usize*4]);}
                if std::env::var("FORMA_GPU_GOLDEN_MODE").as_deref()==Ok("record") {
                    std::fs::create_dir_all(&dir).unwrap();
                    assert!(!path.exists(), "Refusing to overwrite golden {path:?}");
                    std::fs::write(path,&packed).unwrap();
                } else {
                    let reference=std::fs::read(&path).expect("Recorded GPU golden required");
                    assert!(packed==reference, "GPU pixels changed against {path:?}");
                }
            }
            if name == "smooth-corner" && s == 1. {
                let mut shades = std::collections::BTreeSet::new();
                for y in 0..24usize {
                    for x in 0..24usize {
                        let red = mapped[y * stride as usize + x * 4];
                        if red > 0 && red < 255 { shades.insert(red); }
                    }
                }
                assert!(shades.len() > 8, "Rounded edge regressed to quantized coverage: {shades:?}");
            }
            let expected = model.pixels(w, h, s);
            let mut total = 0u64;
            let mut bad = 0;
            let mut max = 0;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    for k in 0..4 {
                        let p = &expected[(y * w as usize + x) * 4..][..4];
                        let a = if k == 3 {
                            p[3]
                        } else {
                            ((p[k] as u32 * p[3] as u32 + 127) / 255) as u8
                        };
                        let b = mapped[y * stride as usize + x * 4 + k];
                        let d = a.abs_diff(b);
                        total += d as u64;
                        max = max.max(d);
                        if d > 4 {
                            bad += 1;
                        }
                    }
                }
            }
            let mean = total as f64 / (w * h * 4) as f64;
            println!("{name} {w}x{h}: mean_error={mean:.4}/255 max={max} channels>4={bad}");
            assert!(mean < 1., "GPU image diverged from CPU");
            assert!(
                bad < (w * h / 50) as usize,
                "Too many visibly different channels"
            );
            drop(mapped);
            readback.unmap();
            let uploads = renderer.uploads;
            model.pointer(100., 90., 0);
            model.tick(20.);
            renderer
                .draw(
                    &model,
                    &texture.create_view(&Default::default()),
                    w,
                    h,
                    s,
                    true,
                    false,
                )
                .unwrap();
            assert_eq!(
                uploads, renderer.uploads,
                "Animation must update uniforms, not geometry"
            );
        }
    }
    println!(
        "GPU vector checks passed; geometry uploads={} frames={}",
        renderer.uploads, renderer.frames
    );
}
