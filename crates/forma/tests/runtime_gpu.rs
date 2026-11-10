#![cfg(feature = "gpu")]
use forma::{Runtime, gpu::Renderer, gpu_runtime as wgpu};

#[test]
#[ignore = "requires a real GPU"]
fn retained_scroll_cpu_gpu_pixels_agree_including_fractional_edges() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        println!("Scroll CPU/GPU adapter: {:?}", adapter.get_info());
        let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let mut failures = Vec::new();
        for (name, source) in [
            (
                "integer",
                include_str!("fixtures/svet-scroll-viewport/App.ui"),
            ),
            (
                "fractional",
                include_str!("fixtures/svet-scroll-viewport/Fractional.ui"),
            ),
            (
                "integer-alpha",
                include_str!("fixtures/svet-scroll-viewport/App.ui"),
            ),
            (
                "fractional-alpha",
                include_str!("fixtures/svet-scroll-viewport/Fractional.ui"),
            ),
            (
                "integer-border",
                include_str!("fixtures/svet-scroll-viewport/App.ui"),
            ),
            (
                "fractional-border",
                include_str!("fixtures/svet-scroll-viewport/Fractional.ui"),
            ),
            (
                "integer-wide-border",
                include_str!("fixtures/svet-scroll-viewport/App.ui"),
            ),
            (
                "fractional-wide-border",
                include_str!("fixtures/svet-scroll-viewport/Fractional.ui"),
            ),
        ] {
            let translucent = name.ends_with("-alpha");
            let mut red = include_str!("fixtures/svet-scroll-viewport/Red.component.ui")
                .replace("#ff0000", if translucent { "#ff000080" } else { "#ff0000" });
            let mut green = include_str!("fixtures/svet-scroll-viewport/Green.component.ui")
                .replace("#00ff00", if translucent { "#00ff0080" } else { "#00ff00" });
            if name.ends_with("-border") {
                let border = if name.contains("wide-border") {
                    "radius:0; Border { width:40; background:#0000ff80; }"
                } else {
                    "radius:0; Border { width:1.25; background:#0000ff80; }"
                };
                red = red.replace("radius:0;", border);
                green = green.replace("radius:0;", border);
            }
            let files = std::collections::BTreeMap::from([
                ("App.ui".into(), source.into()),
                ("Red.component.ui".into(), red),
                ("Green.component.ui".into(), green),
            ]);
            for scale in [1., 1.25, 2.] {
                let mut model =
                    Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
                let max_scroll = 128. - model.viewport()[3];
                for offset in [0., 32., max_scroll] {
                    model.scroll(0., offset - model.scroll_offset()[1]);
                    let gpu = read(&mut renderer, &model, scale);
                    let width = (128. * scale) as u32;
                    let cpu = model.pixels(width, width, scale);
                    assert_eq!(gpu.len(), cpu.len());
                    // Closed-form area references, independent of either
                    // renderer: quarter-covered corner; then a 7/8 red and
                    // 1/8 green source-over seam above the opaque background.
                    let area_reference = if name == "fractional" && scale == 1. && offset == 0. {
                        Some((32usize, 33usize, [77u8, 26, 38, 255]))
                    } else if name == "fractional" && scale == 1.25 && offset == 32. {
                        Some((41, 81, [197, 36, 6, 255]))
                    } else if name == "fractional-alpha" && scale == 1. && offset == 0. {
                        Some((32, 33, [47, 30, 45, 255]))
                    } else if name == "fractional-alpha" && scale == 1.25 && offset == 32. {
                        Some((41, 81, [114, 34, 27, 255]))
                    } else if name.starts_with("fractional-")
                        && name.ends_with("-border")
                        && scale == 1.
                        && offset == 0.
                    {
                        Some((32, 33, [45, 26, 70, 255]))
                    } else {
                        None
                    };
                    if let Some((x, y, expected)) = area_reference {
                        let start = (y * width as usize + x) * 4;
                        for k in 0..4 {
                            assert!(
                                gpu[start + k].abs_diff(expected[k]) <= 1,
                                "GPU area reference {name} DPI{scale} offset{offset}"
                            );
                            assert!(
                                cpu[start + k].abs_diff(expected[k]) <= 1,
                                "CPU area reference {name} DPI{scale} offset{offset}"
                            );
                        }
                    }
                    let mut worst = (0u8, 0usize);
                    let mut sum = 0u64;
                    for (i, (a, b)) in gpu.iter().zip(&cpu).enumerate() {
                        let difference = a.abs_diff(*b);
                        if difference > worst.0 {
                            worst = (difference, i);
                        }
                        sum += difference as u64;
                    }
                    let mean = sum as f64 / gpu.len() as f64;
                    println!(
                        "{name} DPI{scale} offset{offset}: CPU/GPU mean{mean:.6} max{} at({},{}), channel{}",
                        worst.0,
                        worst.1 / 4 % width as usize,
                        worst.1 / 4 / width as usize,
                        worst.1 % 4
                    );
                    if worst.0 > 4 {
                        let start = worst.1 / 4 * 4;
                        println!(
                            "Worst pixel CPU {:?}, GPU {:?}",
                            &cpu[start..start + 4],
                            &gpu[start..start + 4]
                        );
                    }
                    if worst.0 > 4 || mean >= 0.1 {
                        failures.push(format!(
                            "{name} DPI{scale} offset{offset}: max{} mean{mean:.6}",
                            worst.0
                        ));
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "CPU/GPU parity failures:\n{}",
            failures.join("\n")
        );
    });
}

#[test]
#[ignore = "requires a real GPU"]
fn fractional_scroll_viewport_clips_at_physical_pixel_centers() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        println!("Fractional Scroll adapter: {:?}", adapter.get_info());
        let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let files = std::collections::BTreeMap::from([
            (
                "App.ui".into(),
                include_str!("fixtures/svet-scroll-viewport/Fractional.ui").into(),
            ),
            (
                "Red.component.ui".into(),
                include_str!("fixtures/svet-scroll-viewport/Red.component.ui").into(),
            ),
            (
                "Green.component.ui".into(),
                include_str!("fixtures/svet-scroll-viewport/Green.component.ui").into(),
            ),
        ]);
        for scale in [1., 1.25, 2.] {
            let mut model = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
            assert_eq!(model.viewport(), vec![32.5, 33.5, 63., 61.]);
            for offset in [0., 32., 67.] {
                model.scroll(0., offset - model.scroll_offset()[1]);
                let actual = read(&mut renderer, &model, scale);
                let width = (128. * scale) as usize;
                let mut compared = 0;
                for (index, pixel) in actual.chunks_exact(4).enumerate() {
                    let x = ((index % width) as f32 + 0.5) / scale;
                    let y = ((index / width) as f32 + 0.5) / scale;
                    let inside = (32.5..95.5).contains(&x) && (33.5..94.5).contains(&y);
                    if inside {
                        // Isolate viewport clipping from scrollbar paint and
                        // primitive antialiasing at tile edges/seams. Every
                        // outside pixel remains checked, including AA spill.
                        let near_edge = (x - 32.5).abs() < 1. / scale
                            || [33.5 - offset, 97.5 - offset, 161.5 - offset]
                                .iter()
                                .any(|edge| (y - edge).abs() < 1. / scale);
                        if x >= 87.5 || near_edge {
                            continue;
                        }
                    }
                    let expected = if !inside {
                        [17, 34, 51, 255]
                    } else if y - 33.5 + offset < 64. {
                        [255, 0, 0, 255]
                    } else {
                        [0, 255, 0, 255]
                    };
                    assert_eq!(
                        pixel, expected,
                        "fractional DPI{scale}, scroll{offset}, pixel({x},{y})"
                    );
                    compared += 1;
                }
                assert!(compared as f32 > (width * width) as f32 * 0.9);
                println!(
                    "Fractional Scroll DPI{scale} offset{offset}: {compared} pixels match center-clip reference"
                );
            }
        }
    });
}

#[test]
#[ignore = "requires a real GPU"]
fn retained_scroll_viewport_matches_independent_rect_crop_reference() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        println!("Scroll crop adapter: {:?}", adapter.get_info());
        let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let files = std::collections::BTreeMap::from([
            (
                "App.ui".into(),
                include_str!("fixtures/svet-scroll-viewport/App.ui").into(),
            ),
            (
                "Red.component.ui".into(),
                include_str!("fixtures/svet-scroll-viewport/Red.component.ui").into(),
            ),
            (
                "Green.component.ui".into(),
                include_str!("fixtures/svet-scroll-viewport/Green.component.ui").into(),
            ),
        ]);
        for scale in [1., 1.25, 2.] {
            let mut model = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
            assert_eq!(model.viewport(), vec![32., 32., 64., 64.]);
            for offset in [0., 32., 64.] {
                model.scroll(0., offset - model.scroll_offset()[1]);
                let actual = read(&mut renderer, &model, scale);
                let width = (128. * scale) as usize;
                if let Ok(directory) = std::env::var("SVET_GPU_EVIDENCE_DIR") {
                    std::fs::create_dir_all(&directory).unwrap();
                    std::fs::write(
                        std::path::Path::new(&directory).join(format!(
                            "scroll-dpi-{scale}-offset-{offset}-size-{width}.rgba"
                        )),
                        &actual,
                    )
                    .unwrap();
                }
                let mut compared = 0;
                for (index, pixel) in actual.chunks_exact(4).enumerate() {
                    let x = (index % width) as f32 / scale + 0.5 / scale;
                    let y = (index / width) as f32 / scale + 0.5 / scale;
                    let inside = (32. ..96.).contains(&x) && (32. ..96.).contains(&y);
                    // Scrollbar painting is a separate feature. Its reserved
                    // strip is excluded; clipping outside the viewport is not.
                    if inside && x >= 88. {
                        continue;
                    }
                    let expected = if !inside {
                        [17, 34, 51, 255]
                    } else if y - 32. + offset < 64. {
                        [255, 0, 0, 255]
                    } else {
                        [0, 255, 0, 255]
                    };
                    assert_eq!(
                        pixel, expected,
                        "DPI{scale}, scroll{offset}, pixel({x},{y})"
                    );
                    compared += 1;
                }
                assert!(compared as f32 > (width * width) as f32 * 0.95);
                println!(
                    "Scroll crop DPI{scale} offset{offset}: {compared} pixels match independent reference"
                );
            }
        }
    });
}

fn read(renderer: &mut Renderer, model: &Runtime, scale: f32) -> Vec<u8> {
    let w = (128. * scale) as u32;
    let h = (128. * scale) as u32;
    let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Independent controls"),
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
    renderer
        .draw(
            model,
            &target.create_view(&Default::default()),
            w,
            h,
            scale,
            true,
            false,
        )
        .unwrap();
    let stride = (w * 4).div_ceil(256) * 256;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: stride as u64 * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
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
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    renderer
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let mut out = Vec::new();
    for row in mapped.chunks_exact(stride as usize) {
        out.extend_from_slice(&row[..w as usize * 4]);
    }
    out
}
#[test]
#[ignore = "requires a real GPU"]
fn animations_change_only_their_own_pixels_at_integer_and_fractional_dpi() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let source = "component Demo { Frame { width:128; height:128; padding:8; gap:12; clip:true; radius:12; background:#112233; Button { key:'one'; width:100; height:40; background:#ff0000; } Button { key:'two'; width:100; height:40; background:#00ff00; } } }";
        let template = "component Button { Rectangle { radius:8; background:Brush { color:props.background; hover:#0000ff; pressed:#ffffff; transition:0ms; }; Border { width:1; background:#aabbcc; } PointerArea { clicked -> events.clicked(); } } }";
        for scale in [1., 1.25, 2.] {
            let mut model = Runtime::from_sources(source, template).unwrap();
            let before = read(&mut renderer, &model, scale);
            let uploads = renderer.uploads;
            let bytes_before = renderer.resource_stats().uploaded_bytes_total;
            let full_paint_bytes = model.gpu_paints().len() as u64 * 4;
            model.pointer(30., 25., 0);
            let hovered = read(&mut renderer, &model, scale);
            let sent = renderer.resource_stats().uploaded_bytes_total - bytes_before;
            assert!(
                sent < full_paint_bytes,
                "localized hover uploaded {sent} bytes; full paints {full_paint_bytes}"
            );
            println!("DPI {scale}: localized upload {sent} vs full paint {full_paint_bytes} bytes");
            assert_eq!(
                renderer.uploads, uploads,
                "paint-only changes must not upload geometry"
            );
            let w = (128. * scale) as usize;
            let pixel = |data: &[u8], x: usize, y: usize| -> Vec<u8> {
                let i = (((y as f32 * scale) as usize) * w + (x as f32 * scale) as usize) * 4;
                data[i..i + 4].to_vec()
            };
            assert_eq!(pixel(&before, 30, 25), vec![255, 0, 0, 255]);
            assert_eq!(pixel(&hovered, 30, 25), vec![0, 0, 255, 255]);
            assert_eq!(pixel(&hovered, 30, 75), vec![0, 255, 0, 255]);
            let lower = (55. * scale) as usize * w * 4;
            assert_eq!(
                &before[lower..],
                &hovered[lower..],
                "the entire second control including AA/text/clip must remain byte-identical"
            );
            assert_eq!(hovered[3], 0, "rounded root clips all children");
            model.pointer(30., 75., 1);
            let pressed = read(&mut renderer, &model, scale);
            assert_eq!(pixel(&pressed, 30, 25), vec![255, 0, 0, 255]);
            assert_eq!(pixel(&pressed, 30, 75), vec![255; 4]);
        }
    });
}

// Independent reference for the analytic glyph coverage: a scanline rasterizer
// over the same edges with exact x spans and 64 sub-rows per pixel (y error at
// most 1/128 of coverage). It does not share the shader's area formula.
#[test]
#[ignore = "requires a real GPU"]
fn analytic_text_coverage_matches_supersampled_reference() {
    use forma::display_list::STRIDE;
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let source = "component Demo { Frame { width:128; height:128; padding:0; background:#203040; Button { width:128; height:128; } } }";
        let template = "component Button { Rectangle { background:#203040; ContentText { x:2; y:4; width:124; height:40; text:'Forma Жg'; fontSize:17.5; color:#ffffff; } ContentText { x:1; y:50; width:126; height:30; text:'ilj ОБ 0.5'; fontSize:11; color:#ffffff; } ContentShape { points:'10 90 100 95 60 120'; color:#ffffff; } } }";
        let background = [0x20 as f32 / 255., 0x30 as f32 / 255., 0x40 as f32 / 255.];
        for scale in [1., 1.25, 2.] {
            let model = Runtime::from_sources(source, template).unwrap();
            let actual = read(&mut renderer, &model, scale);
            let commands = model.gpu_commands(scale, true);
            let edges = model.gpu_edges(scale, true);
            let w = (128. * scale) as usize;
            let h = (128. * scale) as usize;
            // Scanline reference: 64 sub-rows per pixel row, exact span lengths in x,
            // nonzero winding; glyphs composite sequentially like the shader.
            const ROWS: usize = 64;
            let mut value = vec![background; w * h];
            let mut intersections = Vec::new();
            for c in commands.chunks_exact(STRIDE).filter(|c| c[0] == 1.) {
                let (start, count) = (c[2] as usize, c[3] as usize);
                let contour = &edges[start * 4..(start + count) * 4];
                let mut coverage = vec![0f32; w * h];
                for row in 0..h * ROWS {
                    let y = (row as f32 + 0.5) / ROWS as f32 / scale;
                    intersections.clear();
                    for e in contour.chunks_exact(4) {
                        if (e[1] <= y && e[3] > y) || (e[3] <= y && e[1] > y) {
                            intersections.push((
                                (e[0] + (y - e[1]) * (e[2] - e[0]) / (e[3] - e[1])) * scale,
                                if e[3] > e[1] { 1i32 } else { -1 },
                            ));
                        }
                    }
                    intersections.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let mut winding = 0;
                    let mut previous = 0f32;
                    for &(x, direction) in &intersections {
                        if winding != 0 {
                            let (x0, x1) = (previous.max(0.), x.min(w as f32));
                            let mut px = x0.floor() as usize;
                            while (px as f32) < x1 && px < w {
                                let overlap = (x1.min(px as f32 + 1.) - x0.max(px as f32)).max(0.);
                                coverage[(row / ROWS) * w + px] += overlap / ROWS as f32;
                                px += 1;
                            }
                        }
                        winding += direction;
                        previous = x;
                    }
                }
                for (&cov, px) in coverage.iter().zip(value.iter_mut()) {
                    let cov = cov.min(1.);
                    for chan in px.iter_mut() {
                        *chan = cov + (1. - cov) * *chan;
                    }
                }
            }
            let (mut total, mut max, mut bad, mut covered) = (0u64, 0u8, 0usize, 0usize);
            for (pixel, expected) in value.iter().enumerate() {
                if *expected != background {
                    covered += 1;
                }
                for k in 0..3 {
                    let d = ((expected[k] * 255.).round() as u8).abs_diff(actual[pixel * 4 + k]);
                    total += d as u64;
                    max = max.max(d);
                    if d > 4 {
                        bad += 1;
                    }
                }
                assert_eq!(actual[pixel * 4 + 3], 255, "opaque scene at pixel {pixel}");
            }
            let mean = total as f64 / (w * h * 3) as f64;
            println!(
                "scale {scale}: covered={covered} mean={mean:.4}/255 max={max} channels>4={bad}"
            );
            assert!(covered > 200, "fixture must rasterize text");
            assert!(
                mean < 0.1,
                "mean error {mean} against the scanline reference"
            );
            assert!(max <= 4, "max error {max} against the scanline reference");
            assert_eq!(bad, 0, "{bad} channels differ by more than 4/255");
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a real GPU"]
fn retained_ui_editor_and_summary_render_at_their_scene_bounds() {
    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();
        let files=std::collections::BTreeMap::from([("App.ui".to_string(),
            "component App { state query: String = 'seed'; Column { width:128; height:128; TextInput { width:110; height:30; value <-> state.query; background:#27334a; } Text { width:110; height:30; text:'Value: ${state.query}'; } } }".to_string())]);
        let mut model = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        model.set_svet_document_scheduling(true);
        for scale in [1., 1.25, 2.] {
            let actual = read(&mut renderer, &model, scale);
            let w = (128. * scale) as usize;
            let bounds = model.control_bounds(0);
            let x = ((bounds[0] + bounds[2] - 5.) * scale) as usize;
            let y = ((bounds[1] + 15.) * scale) as usize;
            assert_eq!(
                &actual[(y * w + x) * 4..(y * w + x) * 4 + 4],
                &[0x27, 0x33, 0x4a, 255],
                "editor background at DPI {scale}"
            );
            // A blank patch strictly inside the editor avoids comparing the
            // independent CPU/GPU glyph antialiasing formulas at fractional DPI.
            for dy in 5..25 {
                for dx in 90..100 {
                    let x = ((bounds[0] + dx as f32) * scale) as usize;
                    let y = ((bounds[1] + dy as f32) * scale) as usize;
                    assert_eq!(
                        &actual[(y * w + x) * 4..(y * w + x) * 4 + 4],
                        &[0x27, 0x33, 0x4a, 255],
                        "blank editor patch DPI {scale}"
                    );
                }
            }
        }
    });
}
