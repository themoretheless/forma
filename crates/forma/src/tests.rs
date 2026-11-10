use super::*;
fn compare_reference(b: &Button, w: u32, h: u32, s: f32) {
    for background in [false, true] {
        let reference = b.raster_reference(w, h, s, background);
        let cached = b.raster_cached(w, h, s, background);
        for (index, (a, b)) in reference
            .chunks_exact(4)
            .zip(cached.chunks_exact(4))
            .enumerate()
        {
            // Compare premultiplied channels: an RGB of alpha=0 is undefined.
            for k in 0..4 {
                let aa = if k == 3 {
                    a[3] as f32
                } else {
                    a[k] as f32 * a[3] as f32 / 255.
                };
                let bb = if k == 3 {
                    b[3] as f32
                } else {
                    b[k] as f32 * b[3] as f32 / 255.
                };
                assert!(
                    (aa - bb).abs() <= 2.,
                    "pixel {index} channel {k}: {a:?} vs {b:?}"
                );
            }
        }
    }
}
#[test]
fn cached_raster_preserves_coverage_text_and_clips() {
    let mut b = Button::new();
    for s in [1., 1.25, 2.] {
        compare_reference(&b, (400. * s) as u32, (200. * s) as u32, s);
    }
    b.pointer(100., 90., 0);
    b.tick(55.);
    compare_reference(&b, 400, 200, 1.);
    let source = "component Demo { Frame { width:64; height:48; radius:12; clip:true; padding:2; background:#12345680; Scroll { Button { width:100; height:60; } } } }";
    let component = "component Button { Rectangle { radius:12; background:#ff000080; Border { width:2; background:#aabbcc80; } ContentClip { x:5; y:2; width:45; height:40; radius:5; } ContentShape { points:'0 0 70 0 70 50 0 50'; color:#abcdef80; } ContentText { x:1; y:2; width:55; height:30; text:'Тест'; color:#ffffffa0; fontSize:16; } ContentClipEnd {} } }";
    let mut b = Button::from_sources(source, component).unwrap();
    compare_reference(&b, 80, 60, 1.25);
    b.scroll(10., 4.);
    compare_reference(&b, 80, 60, 1.25);
}
#[test]
fn unchanged_hover_does_not_dirty_or_rebuild_static_layers() {
    let mut b = Button::new();
    let first = b.pixels(400, 200, 1.);
    assert_eq!(b.raster_stats(), vec![1, 1]);
    b.pointer(100., 90., 0);
    assert!(b.is_animating());
    b.tick(1000.);
    let hovered = b.pixels(400, 200, 1.);
    assert_ne!(first, hovered);
    assert_eq!(b.raster_stats(), vec![1, 2]);
    let revision = b.visual_revision();
    for i in 0..100 {
        b.pointer(101. + i as f32, 95., 0);
        assert!(!b.is_animating());
        assert_eq!(b.visual_revision(), revision);
    }
    assert_eq!(hovered, b.pixels(400, 200, 1.));
    assert_eq!(b.raster_stats(), vec![1, 2]);
    b.pixels(800, 400, 2.);
    assert_eq!(b.raster_stats(), vec![2, 3]);
    b.load_source(&EXAMPLE.replace("Найти документы", "Новый текст"))
        .unwrap();
    assert_eq!(b.raster_stats(), vec![0, 0]);
    assert_ne!(hovered, b.pixels(400, 200, 1.));
}
const OVERFLOW: &str = "component Demo { Frame { width:100; height:80; padding:10; Button { width:150; height:120; background:#ff0000; text:''; } } }";
#[test]
fn overflow_is_valid_layout() {
    let b = Button::from_source(OVERFLOW).unwrap();
    assert_eq!(b.render_width(), 170.);
    assert_eq!(b.render_height(), 140.);
    assert!(b.hit(130., 60.));
}
#[test]
fn hidden_overflow_clips_pixels_and_hit_testing() {
    let b = Button::from_source(&OVERFLOW.replace("padding:10;", "padding:10; overflow:hidden;"))
        .unwrap();
    assert_eq!(b.render_width(), 100.);
    assert!(!b.hit(130., 60.));
    let p = b.pixels(170, 140, 1.);
    assert_eq!(
        &p[(60 * 170 + 130) * 4..(60 * 170 + 130) * 4 + 4],
        &[0, 0, 0, 0],
        "Outside Frame is transparent, not an extended square background"
    );
}
#[test]
fn scroll_is_an_element_and_clamps_offsets() {
    let source = OVERFLOW
        .replace("Button {", "Scroll { Button {")
        .replace("} } }", "} } } }");
    let mut b = Button::from_source(&source).unwrap();
    assert!(b.scrollable());
    assert_eq!(b.viewport(), vec![10., 10., 80., 60.]);
    assert_eq!(b.render_width(), 100.);
    let before = b.pixels(100, 80, 1.);
    b.scroll(500., 500.);
    assert_eq!(b.scroll_offset(), vec![70., 60.]);
    assert_ne!(before, b.pixels(100, 80, 1.));
    assert!(b.hit(40., 40.));
    assert!(!b.hit(95., 40.));
    b.scroll(-500., -500.);
    assert_eq!(b.scroll_offset(), vec![0., 0.]);
}
#[test]
fn scroll_is_not_an_overflow_value() {
    assert!(markup::parse(&OVERFLOW.replace("padding:10;", "overflow:scroll;")).is_err());
}
#[test]
fn clicks_require_press_and_release_inside() {
    let mut b = Button::new();
    b.pointer(100., 90., 2);
    assert_eq!(b.clicks(), 0);
    b.pointer(100., 90., 1);
    b.pointer(0., 0., 2);
    assert_eq!(b.clicks(), 0);
    b.pointer(100., 90., 1);
    b.pointer(100., 90., 2);
    assert_eq!(b.clicks(), 1);
}
#[test]
fn markup_controls_pixels_and_geometry() {
    let mut b = Button::new();
    let old = b.pixels(400, 200, 1.);
    b.load_source(
        &EXAMPLE
            .replace("#8ca5ff", "#ff0000")
            .replace("padding: 65 70", "padding: 20 70"),
    )
    .unwrap();
    assert_ne!(old, b.pixels(400, 200, 1.));
    assert!(b.hit(100., 30.));
    assert_eq!(b.action(), "actions.search");
}
#[test]
fn disabled_markup_blocks_events() {
    let mut b = Button::from_source(&EXAMPLE.replace("disabled: false", "disabled: true")).unwrap();
    b.activate();
    b.pointer(100., 90., 1);
    b.pointer(100., 90., 2);
    assert_eq!(b.clicks(), 0);
}
#[test]
fn malformed_markup_keeps_last_scene() {
    let mut b = Button::new();
    assert!(b.load_source("component Bad {}").is_err());
    assert_eq!(b.label(), "Найти документы");
}
#[test]
fn guard_raster_size() {
    assert!(Button::new().pixels(u32::MAX, 100, 1.).is_empty());
}
#[test]
fn frame_clip_rounds_content_and_hit_area() {
    let source = "component Demo { Frame { width:40; height:40; radius:12; clip:true; background:#000; Button { width:60; height:60; } } }";
    let template = "component Button { Rectangle { background:#ffffff; PointerArea { clicked -> events.clicked(); } } }";
    let b = Button::from_sources(source, template).unwrap();
    assert!(b.clipped());
    assert_eq!(b.render_width(), 40.);
    assert!(!b.hit(1., 1.));
    assert!(b.hit(20., 1.));
    assert!(!b.hit(45., 20.));
    let pixels = b.content_pixels(60, 60, 1.);
    assert_eq!(pixels[3], 0);
    assert_eq!(pixels[(20 * 60 + 45) * 4 + 3], 0);
    assert_eq!(pixels[(20 * 60 + 20) * 4 + 3], 255);
    assert!(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
    let open = Button::from_sources(&source.replace("clip:true", "clip:false"), template).unwrap();
    assert_eq!(open.render_width(), 60.);
    assert!(open.hit(1., 1.));
    assert!(open.hit(45., 20.));
    assert_eq!(open.content_pixels(60, 60, 1.)[3], 255);
    assert!(Button::from_sources(&source.replace("clip:true", "clip:1"), template).is_err());
    assert!(
        Button::from_sources(
            &source.replace("clip:true", "clip:true; overflow:visible"),
            template
        )
        .is_err()
    );
}
#[test]
fn nested_frame_clip_does_not_clip_siblings() {
    let source =
        "component Demo { Frame { width:40; height:40; Button { width:40; height:40; } } }";
    let template = "component Button { Rectangle { ContentClip { x:0; y:0; width:20; height:20; radius:8; } ContentShape { points:'0 0 40 0 40 40 0 40'; color:#ff0000; } ContentClipEnd {} ContentShape { points:'30 30 40 30 40 40 30 40'; color:#00ff00; } } }";
    let b = Button::from_sources(source, template).unwrap();
    let p = b.content_pixels(40, 40, 1.);
    assert_eq!(p[3], 0);
    assert_eq!(
        &p[(10 * 40 + 10) * 4..(10 * 40 + 10) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(p[(25 * 40 + 25) * 4 + 3], 0);
    assert_eq!(
        &p[(35 * 40 + 35) * 4..(35 * 40 + 35) * 4 + 4],
        &[0, 255, 0, 255]
    );
    assert!(Button::from_sources(source, &template.replace("ContentClipEnd {}", "")).is_err());
}
#[test]
fn transparent_artboard_has_no_square_background_and_keeps_overflow() {
    let source = "component Demo { Frame { width:40; height:40; padding:0; background:#123456; Button { width:60; height:60; radius:16; } } }";
    let component = "component Button { Rectangle { radius:props.radius; background:#ffffff; } }";
    let button = Button::from_sources(source, component).unwrap();
    let transparent = button.content_pixels(60, 60, 1.);
    assert_eq!(&transparent[0..4], &[0, 0, 0, 0]);
    assert_eq!(
        transparent[(30 * 60 + 50) * 4 + 3],
        255,
        "visible overflow must survive"
    );
    assert!(
        transparent.chunks_exact(4).any(|p| p[3] > 0
            && p[3] < 255
            && p[0] == 255
            && p[1] == 255
            && p[2] == 255),
        "edge coverage must not introduce a dark halo"
    );
    let opaque = button.pixels(60, 60, 1.);
    assert_eq!(&opaque[0..4], &[18, 52, 86, 255]);
    let hidden = Button::from_sources(
        &source.replace("padding:0;", "padding:0; overflow:hidden;"),
        component,
    )
    .unwrap()
    .content_pixels(60, 60, 1.);
    assert_eq!(
        &hidden[(30 * 60 + 50) * 4..(30 * 60 + 50) * 4 + 4],
        &[0, 0, 0, 0]
    );
}
#[test]
fn composed_content_is_rasterized_and_keeps_button_events() {
    let source = "component Demo { Frame { width:100; height:60; padding:0; background:#000; Button { width:100; height:60; clicked -> actions.search(); } } }";
    let component = "component Button { Rectangle { ContentShape { points:'5 5 20 5 20 20 5 20'; color:#ff0000; } ContentText { x:25; y:0; width:70; height:30; text:'A'; color:#ffffff; fontSize:20; } ContentText { x:25; y:30; width:70; height:30; text:'Б'; color:#ffffff; fontSize:20; } PointerArea { clicked -> events.clicked(); } } }";
    let mut button = Button::from_sources(source, component).unwrap();
    let pixels = button.pixels(100, 60, 1.);
    assert_eq!(
        &pixels[(10 * 100 + 10) * 4..(10 * 100 + 10) * 4 + 3],
        &[255, 0, 0]
    );
    assert!(
        pixels
            .chunks_exact(4)
            .any(|p| p[0] > 100 && p[1] > 100 && p[2] > 100)
    );
    assert_eq!(button.label(), "A Б");
    button.activate();
    assert_eq!(button.clicks(), 1);
    assert_eq!(button.action(), "actions.search");
    assert!(
        Button::from_sources(
            source,
            &component.replace("5 5 20 5 20 20 5 20", "NaN 1 2 3 4 5")
        )
        .is_err()
    );
}
#[test]
fn brush_transition_is_time_driven() {
    let mut b = Button::new();
    let initial = b.fill.color();
    b.pointer(100., 90., 0);
    assert_eq!(b.fill.color(), initial);
    assert!(b.tick(70.));
    assert_ne!(b.fill.color(), initial);
    assert!(b.fill.color() != [168, 186, 255, 255]);
    b.tick(1000.);
    assert_eq!(b.fill.color(), [168, 186, 255, 255]);
}
#[test]
fn component_controls_primitives_and_event_forwarding() {
    let template = BUTTON_COMPONENT
        .replace("clicked -> events.clicked();", "")
        .replace("text: props.text;", "text: 'Из компонента';");
    let mut b = Button::from_sources(EXAMPLE, &template).unwrap();
    assert_eq!(b.label(), "Из компонента");
    b.activate();
    assert_eq!(b.clicks(), 0);
    assert!(b.action().is_empty());
}
