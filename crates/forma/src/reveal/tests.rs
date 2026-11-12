use super::*;
#[test]
fn proximity_expands_reverses_and_obeys_reduced_motion() {
    let spec = Reveal::default();
    let bounds = [20., 20., 200., 40.];
    let mut state = State::default();
    state.pointer(Some([10., 40.]), bounds, &spec);
    let near = state.paint(&spec, bounds, false, false);
    assert_eq!(near.reach, 72.);
    assert!(near.color_at([20., 40.])[3] > 0);
    assert_eq!(near.color_at([220., 40.])[3], 0);
    assert!(!state.active());
    state.pointer(Some([30., 40.]), bounds, &spec);
    assert!(state.active());
    state.tick(140.);
    let mid = state.paint(&spec, bounds, false, false).reach;
    assert!(mid > 72. && mid < 1188.);
    state.tick(140.);
    assert!(!state.active());
    assert!((state.paint(&spec, bounds, false, false).reach - 1188.).abs() < 0.001);
    state.pointer(Some([10., 40.]), bounds, &spec);
    assert!(state.active());
    state.reduced_motion(true);
    assert!(!state.active());
    assert_eq!(state.paint(&spec, bounds, false, false).reach, 72.);
    state.pointer(Some([30., 40.]), bounds, &spec);
    assert!(!state.active());
    assert!((state.paint(&spec, bounds, false, false).reach - 1188.).abs() < 0.001);
    state.pointer(None, bounds, &spec);
    assert_eq!(state.paint(&spec, bounds, false, false), Paint::default());
    assert!(state.paint(&spec, bounds, false, true).full);
    assert_eq!(state.paint(&spec, bounds, true, true), Paint::default());
}
#[test]
fn pointer_motion_repaints_the_border_without_rebuilding_cached_text() {
    let source = "component Demo { Frame { width:260; height:100; padding:20; background:#00000000; Button { width:200; height:40; text:'Cached'; } } }";
    let template = "component Button { Rectangle { radius:8; background:#102030; Reveal { color:#ff8800; } Text { text:props.text; color:#ffffff; fontSize:14; } PointerArea { clicked -> events.clicked(); } } }";
    let mut b = crate::Button::from_sources(source, template).unwrap();
    let geometry = b.vector_snapshot(1., false);
    let before = b.content_pixels(260, 100, 1.);
    assert_eq!(b.raster_stats(), vec![1, 1]);
    b.reveal_pointer(-10., 40., true);
    assert!(!b.hover);
    assert!(!b.is_animating());
    let near = b.content_pixels(260, 100, 1.);
    let pixel = |x: usize, y: usize| (y * 260 + x) * 4;
    assert_ne!(
        &near[pixel(20, 40)..pixel(20, 40) + 4],
        &before[pixel(20, 40)..pixel(20, 40) + 4]
    );
    assert_eq!(
        &near[pixel(219, 40)..pixel(219, 40) + 4],
        &before[pixel(219, 40)..pixel(219, 40) + 4]
    );
    assert_eq!(b.raster_stats(), vec![1, 2]);
    assert!(std::sync::Arc::ptr_eq(
        &geometry,
        &b.vector_snapshot(1., false)
    ));
    let revision = b.visual_revision();
    b.reveal_pointer(-1000., 40., true);
    let far_revision = b.visual_revision();
    assert_ne!(far_revision, revision);
    b.reveal_pointer(-1001., 41., true);
    assert_eq!(
        b.visual_revision(),
        far_revision,
        "invisible proximity does not dirty the control"
    );
    b.pointer(30., 40., 0);
    assert!(b.is_animating());
    b.set_reduced_motion(true);
    assert!(!b.is_animating());
    b.reveal_pointer(0., 0., false);
    b.focus(true);
    let focused = b.content_pixels(260, 100, 1.);
    assert_eq!(
        &focused[pixel(20, 40)..pixel(20, 40) + 4],
        &focused[pixel(219, 40)..pixel(219, 40) + 4]
    );
    let mut disabled = crate::Button::from_sources(
        &source.replace("width:200;", "width:200; disabled:true;"),
        template,
    )
    .unwrap();
    let before = disabled.content_pixels(260, 100, 1.);
    disabled.reveal_pointer(30., 40., true);
    disabled.focus(true);
    assert!(!disabled.is_animating());
    assert_eq!(disabled.content_pixels(260, 100, 1.), before);
}

#[cfg(feature = "gpu")]
#[test]
#[ignore = "requires a real GPU; run explicitly with --features gpu -- --ignored"]
fn reveal_cpu_gpu_agree_across_states_and_dpi_without_geometry_uploads() {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .expect("real GPU required");
        let adapter_info = adapter.get_info();
        crate::gpu::assert_gpu_test_adapter(&adapter_info);
        eprintln!(
            "GPU_TEST_ADAPTER {}",
            serde_json::json!({"test":"reveal-agreement","name":adapter_info.name,"vendor":adapter_info.vendor,"device":adapter_info.device,
            "device_type":format!("{:?}",adapter_info.device_type),"backend":format!("{:?}",adapter_info.backend),"driver":adapter_info.driver,"driver_info":adapter_info.driver_info})
        );
        let mut renderer = crate::gpu::Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let mut comparisons = 0usize;
        let mut max_observed_error = 0u8;
        for target in [
            "",
            "targetX:10; targetWidth:18; targetHeight:18; targetRadius:5;",
            "targetX:10; targetWidth:18; targetHeight:18; targetRadius:9;",
        ] {
            let template = format!(
                "component Button {{ Rectangle {{ radius:8; Reveal {{ width:2; color:#e6782080; {target} }} PointerArea {{ clicked -> events.clicked(); }} }} }}"
            );
            for multi in [false, true] {
                let second = if multi {
                    "Button { key:'second'; width:140; height:35; }"
                } else {
                    ""
                };
                let source = format!(
                    "component Demo {{ Frame {{ width:180; height:130; padding:20; gap:10; background:#00000000; Button {{ key:'first'; width:140; height:35; }} {second} }} }}"
                );
                for scale in [1., 1.25, 2.] {
                    let (w, h) = ((180. * scale) as u32, (130. * scale) as u32);
                    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("Reveal CPU/GPU agreement"),
                        size: wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&Default::default());
                    let stride = (w * 4).div_ceil(256) * 256;
                    let readback = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Reveal readback"),
                        size: stride as u64 * h as u64,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    });
                    for state in [
                        "hidden",
                        "outside",
                        "inside-half",
                        "inside-full",
                        "focus",
                        "disabled",
                        "reduced",
                    ] {
                        let source = if state == "disabled" {
                            source.replace("width:140;", "width:140; disabled:true;")
                        } else {
                            source.clone()
                        };
                        let mut model = crate::Runtime::from_sources(&source, &template).unwrap();
                        renderer
                            .draw(
                                &model,
                                &view,
                                crate::gpu::Viewport::new(w, h, scale),
                                false,
                                false,
                            )
                            .unwrap();
                        let uploads = renderer.uploads;
                        match state {
                            "outside" => model.reveal_pointer(-10., 40., true),
                            "inside-half" => {
                                model.reveal_pointer(30., 40., true);
                                model.tick(140.);
                            }
                            "inside-full" => {
                                model.reveal_pointer(30., 40., true);
                                model.tick(280.);
                            }
                            "focus" => {
                                model.focus_control(0);
                            }
                            "disabled" => {
                                model.reveal_pointer(30., 40., true);
                                model.tick(280.);
                            }
                            "reduced" => {
                                model.set_reduced_motion(true);
                                model.reveal_pointer(30., 40., true);
                            }
                            _ => {}
                        }
                        renderer
                            .draw(
                                &model,
                                &view,
                                crate::gpu::Viewport::new(w, h, scale),
                                false,
                                false,
                            )
                            .unwrap();
                        assert_eq!(
                            renderer.uploads, uploads,
                            "Reveal must change paint, never upload glyph geometry"
                        );
                        let mut encoder =
                            renderer.device.create_command_encoder(&Default::default());
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
                            .map_async(wgpu::MapMode::Read, move |result| {
                                send.send(result).unwrap()
                            });
                        renderer
                            .device
                            .poll(wgpu::PollType::wait_indefinitely())
                            .unwrap();
                        recv.recv().unwrap().unwrap();
                        let cpu = model.content_pixels(w, h, scale);
                        let mut max_error = 0;
                        {
                            let mapped = readback.slice(..).get_mapped_range();
                            for y in 0..h as usize {
                                for x in 0..w as usize {
                                    let p = &cpu[(y * w as usize + x) * 4..][..4];
                                    for k in 0..4 {
                                        let expected = if k == 3 {
                                            p[k]
                                        } else {
                                            ((p[k] as u32 * p[3] as u32 + 127) / 255) as u8
                                        };
                                        max_error = max_error.max(
                                            expected
                                                .abs_diff(mapped[y * stride as usize + x * 4 + k]),
                                        );
                                    }
                                }
                            }
                        }
                        assert!(
                            max_error <= 2,
                            "Reveal {state}, multi={multi}, scale={scale}: channel error {max_error}/255"
                        );
                        comparisons += 1;
                        max_observed_error = max_observed_error.max(max_error);
                        readback.unmap();
                    }
                }
            }
        }
        assert_eq!(comparisons, 126);
        eprintln!(
            "GPU_REVEAL_RESULT {}",
            serde_json::json!({"comparisons":comparisons,"max_channel_error":max_observed_error,"channel_error_budget":2,"scale_factors":[1.,1.25,2.]})
        );
    });
}
