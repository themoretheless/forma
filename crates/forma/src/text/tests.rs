use super::*;

// Pre-streaming reference: retain each flattened path before the consumer
// sees any of them. Keep the original physical-origin arithmetic, including
// its f32 rounding; translating a cached origin-free path is not equivalent.
fn collected_vector_glyphs(
    text: &str,
    font_size: f32,
    rect: [f32; 4],
    scale: f32,
) -> Vec<Vec<[f32; 4]>> {
    if rect[2] <= 0. || rect[3] <= 0. || font_size <= 0. {
        return Vec::new();
    }
    let face = font();
    let units_to_pixels = font_size * scale / face.units_per_em() as f32;
    let origin = Point {
        x: (rect[0] + rect[2] * 0.5 - measure_text(text, font_size) * 0.5) * scale,
        y: (rect[1] + rect[3] * 0.5) * scale
            + (face.ascender() as f32 + face.descender() as f32) * units_to_pixels * 0.5,
    };
    let mut contours = Contours {
        #[cfg(test)]
        tolerance: CURVE_TOLERANCE,
        edges: Vec::new(),
        current: origin,
        start: origin,
        origin,
        units_to_pixels,
    };
    let mut paths = Vec::new();
    for ch in text.chars() {
        let (resolved, adv) = resolve_glyph(ch);
        let resolved_face = glyph_face(resolved.face);
        contours.units_to_pixels = font_size * scale / resolved_face.units_per_em() as f32;
        resolved_face.outline_glyph(resolved.id, &mut contours);
        if !contours.edges.is_empty() {
            paths.push(
                contours
                    .edges
                    .drain(..)
                    .map(|(a, b)| [a.x / scale, a.y / scale, b.x / scale, b.y / scale])
                    .collect(),
            );
        }
        contours.origin.x += adv * units_to_pixels;
    }
    paths
}

#[test]
fn streamed_glyphs_preserve_exact_geometry_and_coverage() {
    let mut scratch = VectorScratch::default();
    for text in [
        "",
        "  ",
        "Forma ffi",
        "Привет ОяЖ",
        "A\u{301}Б🙂",
        "A你好🙂Б",
    ] {
        for font_size in [7., 15.5, 31.25] {
            for scale in [0.75, 1., 1.25, 2., 3.] {
                for rect in [
                    [0., 0., 120., 50.],
                    [-21.375, 3.125, 160.5, 45.25],
                    [0.125, -7.75, 100., 40.],
                ] {
                    let expected = collected_vector_glyphs(text, font_size, rect, scale);
                    let mut actual = Vec::new();
                    vector_glyphs(text, font_size, rect, scale, &mut scratch, |glyph| {
                        actual.push(glyph.collect::<Vec<_>>())
                    });
                    let bits = |paths: &[Vec<[f32; 4]>]| {
                        paths
                            .iter()
                            .map(|path| {
                                path.iter()
                                    .map(|edge| edge.map(f32::to_bits))
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(
                        bits(&actual),
                        bits(&expected),
                        "{text:?}, {font_size}, {scale}, {rect:?}"
                    );

                    let raster = |paths: &[Vec<[f32; 4]>]| {
                        let mut pixels = vec![0; 400 * 180 * 4];
                        for path in paths {
                            let edges: Vec<_> = path
                                .iter()
                                .map(|&[x, y, x2, y2]| {
                                    (
                                        Point {
                                            x: x * scale,
                                            y: y * scale,
                                        },
                                        Point {
                                            x: x2 * scale,
                                            y: y2 * scale,
                                        },
                                    )
                                })
                                .collect();
                            rasterize(
                                &mut pixels,
                                400,
                                &edges,
                                [0., 0., 400., 180.],
                                [80, 160, 240, 173],
                            );
                        }
                        pixels
                    };
                    assert_eq!(raster(&actual), raster(&expected));
                }
            }
        }
    }
}

#[test]
fn linear_caret_hit_matches_prefix_measurements_at_unicode_boundaries() {
    for value in ["", "Forma", "Привет🙂 О", "A\u{301}Б", "A你好🙂Б"] {
        for size in [9., 15.5, 32.] {
            let reference = |x| {
                let mut previous = 0.;
                for (offset, ch) in value.char_indices() {
                    let next = measure_text(&value[..offset + ch.len_utf8()], size);
                    if x < (previous + next) * 0.5 {
                        return offset;
                    }
                    previous = next;
                }
                value.len()
            };
            for i in -10..1000 {
                let x = i as f32 * 0.125;
                assert_eq!(hit_character(value, size, x), reference(x));
            }
            for (offset, ch) in value.char_indices() {
                let midpoint = (measure_text(&value[..offset], size)
                    + measure_text(&value[..offset + ch.len_utf8()], size))
                    * 0.5;
                for x in [midpoint - 0.0001, midpoint, midpoint + 0.0001] {
                    assert_eq!(hit_character(value, size, x), reference(x));
                }
            }
        }
    }
}

#[test]
fn shaping_uses_real_fallback_faces_and_does_not_kern_across_fonts() {
    let text = "A你🙂V";
    let shaped = shape_text(text, 28.);
    assert_eq!(
        shaped.iter().map(|g| g.font_index).collect::<Vec<_>>(),
        vec![0, 1, 2, 0]
    );
    assert_eq!(
        shaped.iter().map(|g| g.cluster).collect::<Vec<_>>(),
        text.char_indices().map(|(i, _)| i).collect::<Vec<_>>()
    );
    for (glyph, ch) in shaped.iter().zip(text.chars()) {
        let face = font_face(glyph.font_index).unwrap();
        assert_eq!(Some(GlyphId(glyph.glyph_id)), face.glyph_index(ch));
        let expected = advance(face, GlyphId(glyph.glyph_id)) * 28. / face.units_per_em() as f32;
        assert!((glyph.x_advance - expected).abs() < 0.0001);
        assert_eq!(glyph.fallback, glyph.font_index != 0);
    }
    assert!((measure_shaped_text(text, 28.) - measure_text(text, 28.)).abs() < 0.0001);
    assert!(font_face(3).is_none());
}

#[test]
fn portable_faces_render_real_cjk_and_monochrome_emoji() {
    for (value, index) in [("你", 1), ("好", 1), ("🙂", 2), ("🦀", 2)] {
        let (glyph, units) = resolve_glyph(value.chars().next().unwrap());
        assert_eq!(glyph.face, index);
        let face = glyph_face(index);
        assert_eq!(
            Some(glyph.id),
            face.glyph_index(value.chars().next().unwrap())
        );
        assert!(units > 0.);
        let expected = advance(face, glyph.id) * 28. / face.units_per_em() as f32;
        assert!((measure_text(value, 28.) - expected).abs() < 0.0001);
        let render = |text| {
            let mut pixels = vec![0; 100 * 60 * 4];
            draw_text(
                &mut pixels,
                100,
                60,
                1.,
                text,
                28.,
                [0., 0., 100., 60.],
                [255; 4],
            );
            pixels
        };
        let pixels = render(value);
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
        assert_ne!(pixels, render("?"));
        let mut scratch = VectorScratch::prewarm();
        let mut count = 0;
        vector_glyphs(value, 28., [0., 0., 100., 60.], 1., &mut scratch, |glyph| {
            assert!(glyph.edges.count() > 0);
            count += 1;
        });
        assert_eq!(count, 1);
    }
}

#[test]
fn missing_unicode_scalars_have_visible_consistent_fallback_and_caret_width() {
    for value in ["\u{10ffff}"] {
        assert!(font().glyph_index(value.chars().next().unwrap()).is_none());
        let width = measure_text(value, 28.);
        assert!(width > 0., "missing scalar must not collapse to zero width");
        assert_eq!(width, measure_text("?", 28.));
        assert_eq!(hit_character(value, 28., width * 0.25), 0);
        assert_eq!(hit_character(value, 28., width * 0.75), value.len());
        let render = |text| {
            let mut pixels = vec![0; 100 * 60 * 4];
            draw_text(
                &mut pixels,
                100,
                60,
                1.,
                text,
                28.,
                [0., 0., 100., 60.],
                [255; 4],
            );
            pixels
        };
        let pixels = render(value);
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
        assert_eq!(pixels, render("?"));
        let mut scratch = VectorScratch::prewarm();
        let mut paths = 0;
        vector_glyphs(value, 28., [0., 0., 100., 60.], 1., &mut scratch, |glyph| {
            assert!(glyph.edges.count() > 0);
            paths += 1;
        });
        assert_eq!(paths, 1);
    }
}

#[test]
fn draws_cyrillic_font_outlines_with_antialiasing() {
    let face = Face::parse(FONT, 0).unwrap();
    assert!(face.glyph_index('П').is_some());
    assert!(face.glyph_index('я').is_some());
    let mut pixels = vec![0; 180 * 60 * 4];
    draw_text(
        &mut pixels,
        180,
        60,
        1.,
        "Привет",
        28.,
        [0., 0., 180., 60.],
        [120, 180, 240, 255],
    );
    let count = pixels.chunks_exact(4).filter(|p| p[3] > 0).count();
    assert!(
        count > 150,
        "Cyrillic label must produce real glyph contours"
    );
    assert!(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
    assert!(measure_text("Привет", 28.) > 60.);
    assert!((measure_text("Привет", 56.) - measure_text("Привет", 28.) * 2.).abs() < 0.01);
}

#[test]
fn clips_oversized_text_to_label_rectangle() {
    let mut pixels = vec![0; 40 * 30 * 4];
    draw_text(
        &mut pixels,
        40,
        30,
        1.,
        "ННННННН",
        35.,
        [10., 10., 20., 10.],
        [255; 4],
    );
    assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
    for y in 0..30 {
        for x in 0..40 {
            if !(10..30).contains(&x) || !(10..20).contains(&y) {
                assert_eq!(pixels[(y * 40 + x) * 4 + 3], 0, "escaped clip at {x},{y}");
            }
        }
    }
}

#[test]
fn handles_tiny_canvas_and_invalid_inputs_without_out_of_bounds_writes() {
    let mut pixels = [0; 4];
    draw_text(
        &mut pixels,
        1,
        1,
        2.,
        "Я",
        80.,
        [-10., -10., 30., 30.],
        [255; 4],
    );
    let before = pixels;
    draw_text(
        &mut pixels,
        1,
        1,
        1.,
        "Я",
        80.,
        [10., 10., 30., 30.],
        [255; 4],
    );
    assert_eq!(pixels, before);
    draw_text(
        &mut pixels,
        100,
        100,
        1.,
        "Я",
        80.,
        [0., 0., 30., 30.],
        [255; 4],
    );
    draw_text(
        &mut pixels,
        1,
        1,
        f32::NAN,
        "Я",
        80.,
        [0., 0., 30., 30.],
        [255; 4],
    );
    assert_eq!(pixels, before);
}

#[test]
fn honors_source_alpha_on_transparent_destination() {
    let mut pixels = vec![0; 100 * 50 * 4];
    draw_text(
        &mut pixels,
        100,
        50,
        1.,
        "Ж",
        38.,
        [0., 0., 100., 50.],
        [80, 160, 240, 128],
    );
    let visible: Vec<_> = pixels.chunks_exact(4).filter(|p| p[3] > 0).collect();
    assert!(!visible.is_empty());
    for pixel in visible {
        assert_eq!(&pixel[..3], &[80, 160, 240]);
        assert!(pixel[3] <= 128);
    }
}

#[test]
fn winding_fill_preserves_cyrillic_letter_counter() {
    let mut pixels = vec![0; 100 * 100 * 4];
    draw_text(
        &mut pixels,
        100,
        100,
        1.,
        "О",
        70.,
        [0., 0., 100., 100.],
        [255; 4],
    );
    assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
    assert_eq!(
        pixels[(50 * 100 + 50) * 4 + 3],
        0,
        "О must retain its empty center"
    );
}

#[test]
fn cubic_contours_are_flattened_into_segments() {
    let origin = Point { x: 0., y: 0. };
    let mut contours = Contours {
        #[cfg(test)]
        tolerance: CURVE_TOLERANCE,
        edges: Vec::new(),
        current: origin,
        start: origin,
        origin,
        units_to_pixels: 1.,
    };
    contours.move_to(0., 0.);
    contours.curve_to(0., 20., 20., 20., 20., 0.);
    contours.close();
    assert!(contours.edges.len() > 8);
    assert!(contours.edges.iter().any(|(a, b)| a.y.min(b.y) <= -14.));
}

// Scanline reference with exact x spans and 64 sub-rows: independent of the
// closed-form edge_area, so it validates the CPU rasterizer the same way
// tests/runtime_gpu.rs validates the shader.
fn scanline_reference(width: usize, height: usize, edges: &[(Point, Point)]) -> Vec<f32> {
    const ROWS: usize = 64;
    let mut coverage = vec![0f32; width * height];
    let mut hits = Vec::new();
    for row in 0..height * ROWS {
        let y = (row as f32 + 0.5) / ROWS as f32;
        hits.clear();
        for &(a, b) in edges {
            if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                hits.push((
                    a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y),
                    if b.y > a.y { 1i32 } else { -1 },
                ));
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut winding, mut previous) = (0, 0f32);
        for &(x, direction) in &hits {
            if winding != 0 {
                let (x0, x1) = (previous.max(0.), x.min(width as f32));
                let mut px = x0.floor().max(0.) as usize;
                while (px as f32) < x1 && px < width {
                    coverage[(row / ROWS) * width + px] +=
                        (x1.min(px as f32 + 1.) - x0.max(px as f32)).max(0.) / ROWS as f32;
                    px += 1;
                }
            }
            winding += direction;
            previous = x;
        }
    }
    coverage
}

#[test]
#[ignore = "manual release timing probe; not a CI latency assertion"]
fn font_flattening_cost_probe() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fallback");
    let candidate_root = std::env::var_os("FORMA_FONT_CANDIDATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.clone());
    let data = std::fs::read(candidate_root.join("FormaCJK-Regular.ttf")).unwrap();
    let face = Face::parse(&data, 0).unwrap();
    let glyphs: Vec<_> = "你好世界中文漢字测试"
        .chars()
        .map(|ch| face.glyph_index(ch).unwrap())
        .collect();
    let mut rows = Vec::new();
    for size in [14_f32, 48., 96.] {
        for pair in 0..8 {
            let order = if pair % 2 == 0 {
                [0.15_f32, 0.05, 0.01]
            } else {
                [0.01_f32, 0.05, 0.15]
            };
            for tolerance in order {
                let origin = Point { x: 12.5, y: 120.5 };
                let mut contours = Contours {
                    tolerance,
                    edges: Vec::new(),
                    current: origin,
                    start: origin,
                    origin,
                    units_to_pixels: size / face.units_per_em() as f32,
                };
                let mut pixels = vec![0_u8; 160 * 160 * 4];
                // Each case has its own warmup; buffers are reused in the timed section.
                for &id in &glyphs {
                    face.outline_glyph(id, &mut contours);
                }
                let started = std::time::Instant::now();
                let mut edges = 0_usize;
                for _ in 0..8 {
                    for &id in &glyphs {
                        contours.edges.clear();
                        contours.current = origin;
                        contours.start = origin;
                        face.outline_glyph(std::hint::black_box(id), &mut contours);
                        edges += contours.edges.len();
                        pixels.fill(0);
                        rasterize(
                            &mut pixels,
                            160,
                            &contours.edges,
                            [0., 0., 160., 160.],
                            [255; 4],
                        );
                        std::hint::black_box(&pixels);
                    }
                }
                rows.push(serde_json::json!({"size_px":size,"pair":pair,"warmup_pair":pair==0,"tolerance_px":tolerance,"glyph_renders":glyphs.len()*8,"edges":edges,"elapsed_ns":started.elapsed().as_nanos() as u64}));
            }
        }
    }
    println!(
        "FONT_FLATTENING_COST_REPORT={}",
        serde_json::json!({"scope":"Manual CPU outline plus raster cost, repeated CJK glyphs, reused buffers; not an application frame/startup benchmark","rows":rows})
    );
}

#[test]
fn static_font_candidates_raster_comparison() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fallback");
    let candidate_root = std::env::var_os("FORMA_FONT_CANDIDATE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.clone());
    let tolerance = std::env::var("FORMA_FONT_RASTER_TOLERANCE")
        .ok()
        .map(|v| v.parse::<f32>().expect("finite positive tolerance"))
        .unwrap_or(CURVE_TOLERANCE);
    assert!(tolerance.is_finite() && tolerance > 0.);
    let mut rows = Vec::new();
    for (source, candidate, sample) in [
        (
            "NotoSansSC.ttf",
            "FormaCJK-Regular.ttf",
            "你好世界中文漢字测试",
        ),
        ("NotoEmoji.ttf", "FormaEmoji-Regular.ttf", "🙂😀❤🚀🌍"),
    ] {
        let a = std::fs::read(root.join(source)).unwrap();
        let b = std::fs::read(candidate_root.join(candidate)).unwrap();
        let mut original = Face::parse(&a, 0).unwrap();
        original
            .set_variation(ttf_parser::Tag::from_bytes(b"wght"), 400.)
            .unwrap();
        let derived = Face::parse(&b, 0).unwrap();
        for size in [9_f32, 14., 24., 48., 96.] {
            for offset in [0_f32, 0.25, 0.5, 0.75] {
                let mut max_error = 0_u8;
                let mut changed = 0_usize;
                let mut total_error = 0_u64;
                let mut glyph_reports = Vec::new();
                for ch in sample.chars() {
                    let render = |face: &Face<'_>| {
                        let origin = Point {
                            x: 12. + offset,
                            y: 120. + offset,
                        };
                        let mut contours = Contours {
                            #[cfg(test)]
                            tolerance,
                            edges: Vec::new(),
                            current: origin,
                            start: origin,
                            origin,
                            units_to_pixels: size / face.units_per_em() as f32,
                        };
                        let id = face.glyph_index(ch).expect("sample coverage");
                        assert!(face.outline_glyph(id, &mut contours).is_some());
                        let mut pixels = vec![0_u8; 160 * 160 * 4];
                        rasterize(
                            &mut pixels,
                            160,
                            &contours.edges,
                            [0., 0., 160., 160.],
                            [255; 4],
                        );
                        (pixels, contours.edges.len())
                    };
                    let (before, original_edges) = render(&original);
                    let (after, candidate_edges) = render(&derived);
                    let (mut glyph_max, mut glyph_changed, mut glyph_error, mut coverage) =
                        (0_u8, 0_usize, 0_u64, 0_u64);
                    let mut worst = serde_json::Value::Null;
                    for (index, (x, y)) in before
                        .chunks_exact(4)
                        .zip(after.chunks_exact(4))
                        .enumerate()
                    {
                        let error = x[3].abs_diff(y[3]);
                        if error > glyph_max {
                            glyph_max = error;
                            worst = serde_json::json!({"x":index % 160,"y":index / 160,"original_alpha":x[3],"candidate_alpha":y[3]});
                        }
                        glyph_changed += usize::from(error != 0);
                        glyph_error += u64::from(error);
                        coverage += u64::from(x[3]);
                        max_error = max_error.max(error);
                        changed += usize::from(error != 0);
                        total_error += u64::from(error);
                    }
                    glyph_reports.push(serde_json::json!({"original_edges":original_edges,"candidate_edges":candidate_edges,"character":ch.to_string(),"codepoint":format!("U+{:04X}",ch as u32),"max_alpha_error":glyph_max,"changed_pixels":glyph_changed,"total_alpha_error":glyph_error,"original_alpha_sum":coverage,"relative_l1_error":glyph_error as f64 / coverage.max(1) as f64,"worst_pixel":worst}));
                }
                rows.push(serde_json::json!({"font":candidate,"units_per_em":derived.units_per_em(),"size_px":size,"offset":offset,"glyphs":sample.chars().count(),"max_alpha_error":max_error,"changed_pixels":changed,"total_alpha_error":total_error,"per_glyph":glyph_reports}));
                if source == "NotoEmoji.ttf" {
                    assert_eq!(max_error, 0, "emoji raster changed");
                }
            }
        }
    }
    println!(
        "STATIC_FONT_RASTER_REPORT={}",
        serde_json::json!({"curve_tolerance_px":tolerance,"scope":"Selected glyphs, five pixel sizes and four subpixel offsets through production CPU rasterizer; no full coverage or OS presentation claim","rows":rows})
    );
}

#[test]
fn region_raster_matches_full_canvas_for_text_shapes_and_blending() {
    let (width, height) = (240_u32, 140_u32);
    let regions = [
        [0, 0, 1, 1],
        [17, 9, 83, 40],
        [100, 30, 70, 75],
        [239, 139, 1, 1],
        [0, 0, 240, 140],
    ];
    for scale in [0.75, 1., 1.25, 2.] {
        for label in ["ОЯg", "你好 мир🙂", "ffi"] {
            let seed: Vec<u8> = (0..width * height)
                .flat_map(|i| [(i % 137) as u8, 60, 90, 111])
                .collect();
            let mut full = seed.clone();
            let rect = [-7.125, 3.75, 210.25, 83.5];
            let points = [
                [-40.25, -10.5],
                [180.5, 4.125],
                [70.25, 75.75],
                [31.5, 30.25],
                [-20., 80.],
            ];
            draw_text(
                &mut full,
                width,
                height,
                scale,
                label,
                28.5,
                rect,
                [80, 160, 240, 173],
            );
            crate::shape::draw(
                &mut full,
                width,
                height,
                scale,
                &points,
                [8.125, -3.75],
                [210, 70, 30, 97],
            );
            for region in regions {
                let [x, y, w, h] = region;
                let crop = |pixels: &[u8]| -> Vec<u8> {
                    (y..y + h)
                        .flat_map(|row| {
                            pixels[((row * width + x) * 4) as usize
                                ..((row * width + x + w) * 4) as usize]
                                .iter()
                                .copied()
                        })
                        .collect()
                };
                let mut actual = crop(&seed);
                draw_text_region(
                    &mut actual,
                    width,
                    height,
                    scale,
                    label,
                    28.5,
                    rect,
                    [80, 160, 240, 173],
                    region,
                );
                crate::shape::draw_region(
                    &mut actual,
                    width,
                    height,
                    scale,
                    &points,
                    [8.125, -3.75],
                    [210, 70, 30, 97],
                    region,
                );
                assert_eq!(
                    actual,
                    crop(&full),
                    "{label} scale={scale} region={region:?}"
                );
            }
        }
    }
    let mut untouched = vec![17_u8; 16];
    for region in [
        [u32::MAX, 0, 2, 2],
        [0, u32::MAX, 2, 2],
        [239, 139, 2, 2],
        [0, 0, 0, 0],
        [0, 0, 240, 140],
    ] {
        draw_text_region(
            &mut untouched,
            width,
            height,
            1.,
            "A",
            20.,
            [0., 0., 100., 30.],
            [255; 4],
            region,
        );
        assert_eq!(untouched, vec![17_u8; 16], "invalid region must not write");
    }
}

#[test]
fn exact_area_rasterizer_matches_scanline_reference() {
    let (width, height) = (160usize, 48usize);
    for (text, size, scale) in [
        ("Forma Жg", 17.5, 1.),
        ("ilj ОБ 0.5", 11., 1.25),
        ("Привет", 24., 2.),
    ] {
        let face = font();
        let units_to_pixels = size * scale / face.units_per_em() as f32;
        let origin = Point { x: 3.3, y: 30.7 };
        let mut contours = Contours {
            #[cfg(test)]
            tolerance: CURVE_TOLERANCE,
            edges: Vec::new(),
            current: origin,
            start: origin,
            origin,
            units_to_pixels,
        };
        for ch in text.chars() {
            let id = glyph(face, ch);
            face.outline_glyph(id, &mut contours);
            contours.origin.x += advance(face, id) * units_to_pixels;
        }
        let mut pixels = vec![0u8; width * height * 4];
        rasterize(
            &mut pixels,
            width as u32,
            &contours.edges,
            [0., 0., width as f32, height as f32],
            [255, 255, 255, 255],
        );
        let expected = scanline_reference(width, height, &contours.edges);
        let (mut max, mut covered) = (0u8, 0usize);
        for (i, cov) in expected.iter().enumerate() {
            let reference = (cov.min(1.) * 255.).round() as u8;
            if reference > 0 {
                covered += 1;
            }
            max = max.max(reference.abs_diff(pixels[i * 4 + 3]));
        }
        assert!(covered > 100, "{text}: fixture must rasterize text");
        assert!(
            max <= 2,
            "{text}: max coverage error {max}/255 against the scanline reference"
        );
    }
    // Polygons through the shape path: a concave even-odd-compatible shape.
    let mut pixels = vec![0u8; 64 * 64 * 4];
    crate::shape::draw(
        &mut pixels,
        64,
        64,
        1.25,
        &[[4., 4.], [40., 6.], [20., 20.], [44., 44.], [6., 40.]],
        [1.5, 2.25],
        [255, 255, 255, 255],
    );
    let points: Vec<Point> = [[4., 4.], [40., 6.], [20., 20.], [44., 44.], [6., 40.]]
        .iter()
        .map(|p| Point {
            x: (p[0] + 1.5) * 1.25,
            y: (p[1] + 2.25) * 1.25,
        })
        .collect();
    let edges: Vec<_> = (0..points.len())
        .map(|i| (points[i], points[(i + 1) % points.len()]))
        .collect();
    let expected = scanline_reference(64, 64, &edges);
    let max = expected
        .iter()
        .enumerate()
        .map(|(i, cov)| ((cov.min(1.) * 255.).round() as u8).abs_diff(pixels[i * 4 + 3]))
        .max()
        .unwrap();
    assert!(max <= 2, "shape: max coverage error {max}/255");
}

#[test]
fn shape_text_handles_ligatures_combining_marks_bidi_and_fallback_glyphs() {
    // 1. Combining diacritics: 'A' + U+0301 (combining acute accent) has zero advance on the mark.
    let shaped = shape_text("A\u{0301}B", 20.);
    assert_eq!(shaped.len(), 3);
    assert!(!shaped[0].combining && shaped[0].x_advance > 0.);
    assert!(shaped[1].combining);
    assert_eq!(shaped[1].x_advance, 0.);
    assert!(shaped[1].x_offset < 0. && shaped[1].y_offset > 0.);
    assert!(!shaped[2].combining);
    assert!((measure_shaped_text("A\u{0301}B", 20.) - measure_shaped_text("AB", 20.)).abs() < 0.01);

    // 2. BiDi RTL run reordering: Hebrew "אבג" reverses visual order while keeping logical clusters.
    let rtl = shape_text("אבג", 20.);
    assert_eq!(rtl.len(), 3);
    assert!(rtl.iter().all(|g| g.rtl));
    assert_eq!(
        rtl.iter().map(|g| g.cluster).collect::<Vec<_>>(),
        vec![4, 2, 0]
    );

    // 3. Mixed LTR + RTL + LTR preserves run boundaries and reverses only the RTL run.
    let mixed = shape_text("AאבB", 20.);
    assert_eq!(mixed.len(), 4);
    assert_eq!(
        mixed.iter().map(|g| (g.cluster, g.rtl)).collect::<Vec<_>>(),
        vec![(0, false), (3, true), (1, true), (5, false)]
    );

    // 4. Fallback glyph synthesis for out-of-font emoji/symbols.
    let fallback = shape_text("A🦀", 20.);
    assert_eq!(fallback.len(), 2);
    assert!(!fallback[0].fallback);
    assert!(fallback[1].fallback);
    assert_eq!(fallback[1].font_index, 2);
    assert_ne!(fallback[1].glyph_id, 0);
    let missing = shape_text("\u{10ffff}", 20.);
    assert!(missing[0].fallback);
    assert_eq!(missing[0].glyph_id, 0);
    assert!(fallback[1].x_advance > 0.);
}

#[test]
fn shape_text_handles_zwj_emoji_skin_tones_flags_and_bidi_neutrals() {
    // ZWJ family emoji cluster: "👨‍👩‍👧‍👦" collapses into a single base advance with zero-advance joined glyphs.
    let family = "👨‍👩‍👧‍👦";
    let single = "👨";
    let shaped_family = shape_text(family, 24.);
    assert!(!shaped_family[0].combining);
    assert!(
        shaped_family[1..]
            .iter()
            .all(|g| g.combining && g.x_advance == 0.)
    );
    assert!((measure_shaped_text(family, 24.) - measure_shaped_text(single, 24.)).abs() < 0.01);

    // Skin-tone modifier ("👍🏽") and keycap ("1\u{fe0f}\u{20e3}") and regional indicator flag ("🇬🇪"):
    for (cluster, base) in [("👍🏽", "👍"), ("1\u{fe0f}\u{20e3}", "1"), ("🇬🇪", "🇬")]
    {
        let sc = shape_text(cluster, 24.);
        assert!(!sc[0].combining);
        assert!(
            sc[1..].iter().all(|g| g.combining && g.x_advance == 0.),
            "{cluster}"
        );
        assert!(
            (measure_shaped_text(cluster, 24.) - measure_shaped_text(base, 24.)).abs() < 0.01,
            "{cluster}"
        );
    }

    // BiDi UAX #9 N1: neutral space between two Hebrew words stays inside the single RTL run
    // and reverses visually as one phrase ("אב גד" -> visual order "דג בא").
    let phrase = "אב גד";
    let bidi = shape_text(phrase, 20.);
    assert!(bidi.iter().all(|g| g.rtl));
    let visual_chars: String = bidi
        .iter()
        .map(|g| phrase[g.cluster..].chars().next().unwrap())
        .collect();
    assert_eq!(visual_chars, "דג בא");
}

#[test]
fn wrap_shaped_text_breaks_on_words_cjk_and_emergency_graphemes_with_hit_testing() {
    let text = "Hello world from Forma Studio\nSecond line";
    let single_word_w = measure_shaped_text("Hello world", 16.);
    let wrapped = wrap_shaped_text(text, 16., single_word_w + 2.);
    assert!(wrapped.len() >= 4);
    assert_eq!(&text[wrapped[0].byte_range.clone()], "Hello world ");
    assert_eq!(
        &text[wrapped.last().unwrap().byte_range.clone()],
        "Second line"
    );

    // Emergency grapheme wrap on a narrow width keeps extended grapheme clusters intact.
    let narrow = wrap_shaped_text("A👨‍👩‍👧‍👦B", 20., 5.);
    assert_eq!(narrow.len(), 3);
    assert_eq!(&"A👨‍👩‍👧‍👦B"[narrow[0].byte_range.clone()], "A");
    assert_eq!(&"A👨‍👩‍👧‍👦B"[narrow[1].byte_range.clone()], "👨‍👩‍👧‍👦");
    assert_eq!(&"A👨‍👩‍👧‍👦B"[narrow[2].byte_range.clone()], "B");

    // Hit testing on wrapped lines resolves back to original UTF-8 byte offsets.
    let hit_second_row = hit_wrapped_text(text, 16., single_word_w + 2., 24., 1., 28.);
    assert_eq!(hit_second_row, wrapped[1].byte_range.start);

    // Zero-allocation `measure_shaped_text` agrees with full `shape_text` sum across scripts.
    for sample in [
        "ffi ffl AV To",
        "אב גד",
        "你好世界",
        "👨‍👩‍👧‍👦👍🏽1\u{fe0f}\u{20e3}🇬🇪",
        "Forma — векторы",
    ] {
        let shaped_sum: f32 = shape_text(sample, 18.).iter().map(|g| g.x_advance).sum();
        let fast_measured = measure_shaped_text(sample, 18.);
        assert!(
            (shaped_sum - fast_measured).abs() < 1e-3,
            "{sample}: {shaped_sum} vs {fast_measured}"
        );
    }
}
