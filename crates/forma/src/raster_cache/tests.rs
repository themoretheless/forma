
use super::*;
#[test]
fn frame_run_search_matches_every_scalar_sample_at_viewport_and_aa_boundaries() {
    let mut model = Button::from_sources(
        "component App { Frame { width:100; height:80; Button { width:160; height:120; } } }",
        "component Button { Rectangle { background:#abcdef; } }",
    )
    .unwrap();
    let mut seed = 7u32;
    let mut random = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        seed as f32 / u32::MAX as f32
    };
    for case in 0..240 {
        model.scene.width = 0.125 + random() * 260.;
        model.scene.height = 0.125 + random() * 120.;
        model.scene.radius = if case % 3 == 0 { 0. } else { random() * 200. };
        model.scene.scroll = case % 2 == 0;
        model.scene.padding = [
            random() * 20.,
            random() * 20.,
            random() * 20.,
            random() * 20.,
        ];
        model.scene.button.width = random() * 400.;
        model.scene.button.height = random() * 300.;
        model.scroll_x = random() * 200.;
        model.scroll_y = random() * 200.;
        let width = [1, 16, 63, 64, 79, 128, 257][case % 7];
        let height = [1, 17, 47, 96][case % 4];
        let scale = [
            f32::MIN_POSITIVE,
            0.000001,
            0.125,
            0.75,
            1.,
            1.25,
            2.,
            3.5,
            1_000_000.,
            f32::MAX,
        ][case % 10];
        let key = GeometryKey {
            width,
            height,
            scale,
            scroll: [model.scroll_x, model.scroll_y],
            sparse: false,
        };
        let runs = frame_runs(&model, key);
        let v = model.viewport_rect();
        let mut start = 0;
        let mut previous = None;
        for run in runs {
            assert!(run.end > start && run.end <= width * height);
            assert_ne!(previous, Some(run.value));
            for i in start..run.end {
                assert_eq!(
                    run.value,
                    frame_value(&model, key, v, i % width, i / width),
                    "case={case},pixel={i},scale={scale}"
                );
            }
            start = run.end;
            previous = Some(run.value);
        }
        assert_eq!(start, width * height);
    }
    // Nonfinite/degenerate geometry uses the original scalar path.
    for radius in [-1., f32::NAN, f32::INFINITY] {
        model.scene.radius = radius;
        let key = GeometryKey {
            width: 80,
            height: 20,
            scale: 1.25,
            scroll: [0.; 2],
            sparse: false,
        };
        let runs = frame_runs(&model, key);
        let v = model.viewport_rect();
        let mut expected = Vec::new();
        for y in 0..20 {
            for x in 0..80 {
                append_frame_run(
                    &mut expected,
                    y * 80 + x + 1,
                    frame_value(&model, key, v, x, y),
                );
            }
        }
        assert_eq!(runs, expected);
    }
}

#[test]
fn uniform_frame_storage_is_constant_size_even_with_offscreen_geometry() {
    let component = "component Button { Rectangle { background:#abcdef; } }";
    for x in [30, 3000] {
        let source = format!(
            "component App {{ Frame {{ width:800; height:600; Button {{ x:{x}; y:40; width:160; height:80; }} }} }}"
        );
        let model = Button::from_sources(&source, component).unwrap();
        let key = GeometryKey {
            width: 1600,
            height: 1200,
            scale: 2.,
            scroll: [0.; 2],
            sparse: false,
        };
        FRAME_SAMPLES.with(|samples| samples.set(0));
        let cache = Cache::build(&model, key);
        let samples = FRAME_SAMPLES.with(|samples| samples.get());
        eprintln!("uniform1600x1200 frame: {samples} scalar samples instead of1,920,000");
        assert!(
            samples < 100_000,
            "uniform frame reverted to per-pixel evaluation: {samples}"
        );
        assert_eq!(
            cache.frame,
            vec![FrameRun {
                end: 1_920_000,
                value: 48
            }]
        );
        if x == 30 {
            FRAME_SAMPLES.with(|samples| samples.set(0));
            let mut scalar = Vec::new();
            let v = model.viewport_rect();
            for py in 0..key.height {
                for px in 0..key.width {
                    append_frame_run(
                        &mut scalar,
                        py * key.width + px + 1,
                        frame_value(&model, key, v, px, py),
                    );
                }
            }
            let scalar_samples = FRAME_SAMPLES.with(|samples| samples.get());
            eprintln!("scalar reference frame: {scalar_samples} samples");
            assert_eq!(scalar_samples, 1_920_000);
            assert_eq!(scalar, cache.frame);
        }
        assert!(cache.frame.capacity() * std::mem::size_of::<FrameRun>() <= 64);
        assert_eq!(cache.pixels.len(), 1_920_000 * 4);
    }
}

#[test]
fn cropped_content_matches_full_canvas_storage_exactly() {
    let source = "component App { Frame { width:180; height:140; radius:7; background:#12345680; Button { x:31.3; y:23.7; width:68; height:49; } } }";
    let components = [
        "component Button { Rectangle { radius:4; background:#abcdef80; Text { text:'你好 мир🙂'; fontSize:12; color:#ffffff; } } }",
        "component Button { Rectangle { background:#abcdef80; ContentText { x:-20; y:-12; width:110; height:21; text:'Яg🙂'; fontSize:16; color:#ffccaa80; } ContentClip { x:-6; y:-8; width:85; height:74; radius:6; } ContentShape { points:'-18 -14 80 -14 80 66 -18 66'; color:#ff000080; } ContentClip { x:12.3; y:8.7; width:40; height:30; radius:3; } ContentShape { points:'-2 -2 60 0 45 44'; color:#00ff0080; } ContentClipEnd {} ContentClipEnd {} } }",
        "component Button { Rectangle { background:#abcdef80; ContentShape { points:'-130 -100 -110 -100 -110 -80'; color:#ffffff; } } }",
        "component Button { Rectangle { ContentClip { x:0; y:0; width:20; height:30; } ContentClipEnd {} } }",
    ];
    for component in components {
        let mut model = Button::from_sources(source, component).unwrap();
        for scroll in [[0., 0.], [27.4, 19.8], [300., 300.]] {
            model.scroll_x = scroll[0];
            model.scroll_y = scroll[1];
            for scale in [0.75, 1., 1.25, 1.5, 2.] {
                for sparse in [false, true] {
                    for background in [false, true] {
                        let key = GeometryKey {
                            width: 240,
                            height: 190,
                            scale,
                            scroll,
                            sparse,
                        };
                        let mut actual = Cache::build(&model, key);
                        let mut full =
                            Cache::build_with_content_region(&model, key, [0, 0, 240, 190]);
                        let paint = PaintKey {
                            fill: model.fill.color(),
                            border: model.border.color(),
                            reveal: model.reveal_paint(),
                            background,
                        };
                        actual.paint(&model, paint);
                        full.paint(&model, paint);
                        assert!(
                            actual.pixels == full.pixels,
                            "scale={scale}, sparse={sparse}, scroll={scroll:?}"
                        );
                        assert_eq!(actual.content_spans, full.content_spans);
                        assert_eq!(actual.spans, full.spans);
                        // An offscreen sparse cache returns before compaction;
                        // its unused scratch storage is intentionally different.
                        if !actual.spans.is_empty() {
                            assert!(actual.content == full.content);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn geometry_only_content_matches_transparent_layer_reference() {
    let source = "component App { Frame { width:160; height:120; Button { x:11; y:7; width:64; height:48; } } }";
    let plain = "component Button { Rectangle { background:#abcdef; } }";
    for scale in [0.75, 1., 1.25, 2.] {
        for sparse in [false, true] {
            let model = Button::from_sources(source, plain).unwrap();
            let key = GeometryKey {
                width: 200,
                height: 150,
                scale,
                scroll: [0.; 2],
                sparse,
            };
            let mut empty = Cache::build(&model, key);
            assert_eq!(empty.content.capacity(), 0);
            // Force the original RGBA-layer path with a zero-alpha shape.
            let reference = Button::from_sources(source,
                    "component Button { Rectangle { background:#abcdef; ContentShape { points:'0 0 10 0 10 10'; color:#00000000; } } }"
                ).unwrap();
            let mut full = Cache::build(&reference, key);
            let paint = PaintKey {
                fill: [0xab, 0xcd, 0xef, 255],
                border: [0; 4],
                reveal: model.reveal_paint(),
                background: false,
            };
            empty.paint(&model, paint);
            full.paint(&reference, paint);
            assert_eq!(empty.pixels, full.pixels);
            assert_eq!(empty.spans, full.spans);
            assert_eq!(empty.frame, full.frame);
        }
    }
}

#[test]
fn offscreen_sparse_cache_has_no_canvas_storage_and_preserves_overflow() {
    let source = "component App { Frame { width:200; height:100; Button { x:500; y:300; width:40; height:30; } } }";
    let plain = "component Button { Rectangle { background:#abcdef; } }";
    for scale in [0.75, 1., 1.25, 2.] {
        let mut model = Button::from_sources(source, plain).unwrap();
        let mut out = vec![0; 200 * 100 * 4];
        model.composite_content(&mut out, 200, 100, scale);
        assert!(out.iter().all(|&x| x == 0));
        {
            let cache = model.raster_cache.borrow();
            let cache = cache.as_ref().unwrap();
            assert_eq!(
                cache.coverage.capacity()
                    + cache.content.capacity()
                    + cache.pixels.capacity()
                    + cache.frame.capacity(),
                0
            );
        }
        model.scroll_x = 500.;
        model.scroll_y = 300.;
        out.fill(0);
        model.composite_content(&mut out, 200, 100, scale);
        assert!(
            out.chunks_exact(4).any(|p| p[3] > 0),
            "scroll must rebuild the previously empty cache"
        );
        // Shape reaches the canvas despite the offscreen parent bounds.
        let overflowing = "component Button { Rectangle { background:#abcdef; ContentShape { points:'-490 -290 -470 -290 -470 -270 -490 -270'; color:#ff0000; } } }";
        let model = Button::from_sources(source, overflowing).unwrap();
        out.fill(0);
        model.composite_content(&mut out, 200, 100, scale);
        assert_eq!(out, model.content_pixels(200, 100, scale));
        assert!(
            out.chunks_exact(4).any(|p| p[3] > 0),
            "overflow must remain visible"
        );
    }
}

#[test]
fn packed_leaf_retains_only_local_rows_and_empty_offscreen_leaf_is_valid() {
    let source = "component Demo { Frame { width:1000; height:600; padding:0; Button { x:700.3; y:400.7; width:40; height:24; } } }";
    let component = "component Button { Rectangle { radius:4; background:#abcdef80; Reveal { color:#ffaa44; } ContentText { x:-6; y:-14; width:60; height:12; text:'Я'; fontSize:11; color:#ffffff; } ContentShape { points:'-8 3 6 3 6 8 -8 8'; color:#334455; } } }";
    let model = Button::from_sources(source, component).unwrap();
    let expected = model.content_pixels(1000, 600, 1.25);
    let mut actual = vec![0; expected.len()];
    model.composite_content(&mut actual, 1000, 600, 1.25);
    assert_eq!(actual, expected);
    {
        let stored = model.raster_cache.borrow();
        let cache = stored.as_ref().unwrap();
        assert!(cache.key.sparse && cache.frame.is_empty());
        assert_eq!(cache.pixels.len(), cache.coverage.len() * 4);
        assert_eq!(
            cache.coverage.len(),
            cache.spans.iter().map(|span| span.len()).sum::<usize>()
        );
        assert!(
            cache.coverage.capacity() < 3000,
            "packed geometry must not retain a full canvas"
        );
        assert!(
            cache.pixels.capacity() < 12_000,
            "packed RGBA must not retain a full canvas"
        );
        assert!(cache.content.capacity() < 2000);
        assert!(
            cache
                .spans
                .windows(2)
                .all(|pair| pair[0].end <= pair[1].start)
        );
        assert!(
            cache
                .spans
                .iter()
                .all(|span| span.start < span.end && span.start / 1000 == (span.end - 1) / 1000)
        );
    }
    let mut outside = vec![0; 20 * 20 * 4];
    model.composite_content(&mut outside, 20, 20, 1.25);
    assert!(outside.iter().all(|&b| b == 0));
    let stored = model.raster_cache.borrow();
    let cache = stored.as_ref().unwrap();
    assert!(cache.spans.is_empty() && cache.pixels.is_empty() && cache.coverage.is_empty());
}

#[test]
fn sparse_content_retains_only_occupied_rows_without_changing_pixels() {
    let source = "component Demo { Frame { width:160; height:120; padding:0; Button { width:64; height:48; } } }";
    let component = "component Button { Rectangle { background:#00000000; ContentShape { points:'20 10 25 10 25 15 20 15'; color:#abcdef; } } }";
    let model = Button::from_sources(source, component).unwrap();
    let pixels = model.content_pixels(160, 120, 1.);
    for y in 0..120 {
        for x in 0..160 {
            let expected = if (20..25).contains(&x) && (10..15).contains(&y) {
                [0xab, 0xcd, 0xef, 255]
            } else {
                [0; 4]
            };
            assert_eq!(&pixels[(y * 160 + x) * 4..(y * 160 + x + 1) * 4], &expected);
        }
    }
    let cache = model.raster_cache.borrow();
    let cache = cache.as_ref().unwrap();
    let retained = cache.content.capacity()
        + cache.content_spans.capacity() * std::mem::size_of::<std::ops::Range<usize>>();
    assert!(
        retained < 1024,
        "a 25-pixel shape must not retain a full-canvas RGBA layer: {retained}"
    );
}

#[test]
fn clip_support_contains_all_fractional_coverage_samples() {
    for scale in [0.75, 1., 1.25, 2.] {
        for bounds in [
            [-4.3, -2.1, 13.7, 9.2],
            [7.1, 5.6, 0.2, 0.4],
            [3.3, 4.8, 18.1, 12.5],
            [100., 100., 10., 10.],
            [5., 5., 0., 8.],
        ] {
            let [x0, y0, x1, y1] = clip_pixels(bounds, scale, 48, 40);
            for radius in [0., 2.5, 50.] {
                for y in 0..40 {
                    for x in 0..48 {
                        if x < x0 || x >= x1 || y < y0 || y >= y1 {
                            assert_eq!(round_coverage(x, y, scale, bounds, radius), 0.);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn pooled_sibling_and_nested_clips_preserve_reference_coverage() {
    let source = "component Demo { Frame { width:48; height:40; padding:0; radius:4; background:#10203080; Button { width:48; height:40; } } }";
    let component = "component Button { Rectangle { background:#12345670; ContentClip { x:1.3; y:2.8; width:16.5; height:22.7; radius:3.2; } ContentShape { points:'-3 -2 45 -2 45 38 -3 38'; color:#ff000080; } ContentClip { x:5.4; y:8.2; width:18; height:11.8; radius:2; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#00ff0080; } ContentClipEnd {} ContentClipEnd {} ContentClip { x:26.2; y:1.4; width:12.6; height:16.3; radius:4; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#0000ff80; } ContentClipEnd {} ContentClip { x:0; y:0; width:0; height:30; } ContentShape { points:'0 0 48 0 48 40 0 40'; color:#ffffff; } ContentClipEnd {} } }";
    let model = Button::from_sources(source, component).unwrap();
    for scale in [1., 1.25, 2.] {
        for background in [false, true] {
            let reference = model.raster_reference(96, 80, scale, background);
            let actual = model.raster_cached(96, 80, scale, background);
            for (a, b) in reference.chunks_exact(4).zip(actual.chunks_exact(4)) {
                for k in 0..4 {
                    let premul = |p: &[u8]| {
                        if k == 3 {
                            p[3] as f32
                        } else {
                            p[k] as f32 * p[3] as f32 / 255.
                        }
                    };
                    assert!((premul(a) - premul(b)).abs() <= 2., "{a:?} != {b:?}");
                }
            }
        }
    }
}
