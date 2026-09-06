use forma_vector::{Button, EXAMPLE};

fn flatten(p: &[u8]) -> u32 {
    let a = p[3] as u32;
    let c = |k: usize, bg: u32| (p[k] as u32 * a + bg * (255 - a) + 127) / 255;
    (c(0, 17) << 16) | (c(1, 19) << 8) | c(2, 25)
}

fn compare(source: &str, template: &str, sizes: &[(u32, u32, f32)]) {
    let b = Button::from_sources(source, template).unwrap();
    let reference = Button::from_sources(source, template).unwrap();
    for &(w, h, s) in sizes {
        let expected: Vec<_> = reference
            .pixels(w, h, s)
            .chunks_exact(4)
            .map(flatten)
            .collect();
        let mut actual = vec![0xDEADBEEF; (w * h) as usize];
        b.paint_native(&mut actual, w, h, s).unwrap();
        assert_eq!(actual, expected, "{w}x{h}, scale={s}");
    }
}

#[test]
fn resize_preserves_every_pixel_and_reuses_scene() {
    compare(
        EXAMPLE,
        forma_vector::BUTTON_COMPONENT,
        &[
            (470, 290, 1.),
            (180, 90, 1.),
            (1000, 650, 2.),
            (110, 70, 1.25),
            (620, 320, 1.25),
        ],
    );
    let b = Button::new();
    for (w, h) in [(800, 500), (600, 450), (1200, 700), (40, 30), (900, 550)] {
        b.paint_native(&mut vec![0; (w * h) as usize], w, h, 2.)
            .unwrap();
    }
    assert_eq!(
        b.raster_stats(),
        vec![1, 1],
        "host resize must not rasterize a fixed scene again"
    );
    b.paint_native(&mut vec![0; 400 * 250], 400, 250, 1.25)
        .unwrap();
    assert_eq!(
        b.raster_stats(),
        vec![2, 2],
        "DPI change must rebuild at full quality"
    );
}

#[test]
fn native_animation_scroll_and_reload_invalidate_correctly() {
    let mut b = Button::new();
    let mut dst = vec![0; 500 * 250];
    b.paint_native(&mut dst, 500, 250, 1.).unwrap();
    let initial = dst.clone();
    b.pointer(100., 90., 0);
    b.tick(1000.);
    b.paint_native(&mut dst, 500, 250, 1.).unwrap();
    assert_ne!(dst, initial);
    assert_eq!(b.raster_stats(), vec![1, 2]);
    assert_eq!(
        dst,
        b.pixels(500, 250, 1.)
            .chunks_exact(4)
            .map(flatten)
            .collect::<Vec<_>>()
    );
    b.load_source(&EXAMPLE.replace("#8ca5ff", "#ff0000"))
        .unwrap();
    b.paint_native(&mut dst, 500, 250, 1.).unwrap();
    assert_ne!(dst, initial);
    let source="component Demo { Frame { width:64; height:48; radius:12; clip:true; padding:2; background:#12345680; Scroll { Button { width:100; height:60; } } } }";
    let mut b = Button::from_source(source).unwrap();
    let mut dst = vec![0; 100 * 80];
    b.paint_native(&mut dst, 100, 80, 1.25).unwrap();
    b.scroll(10., 4.);
    b.paint_native(&mut dst, 100, 80, 1.25).unwrap();
    assert_eq!(b.raster_stats(), vec![2, 2]);
    assert_eq!(
        dst,
        b.pixels(100, 80, 1.25)
            .chunks_exact(4)
            .map(flatten)
            .collect::<Vec<_>>()
    );
}

#[test]
fn overflow_and_nested_clips_survive_retained_bounds() {
    let source="component Demo { Frame { width:40; height:30; radius:8; padding:0; background:#12345680; Button { width:45; height:35; } } }";
    let template="component Button { Rectangle { radius:8; background:#ff000080; ContentShape { points:'50 2 77 2 77 20 50 20'; color:#abcdef; } ContentText { x:3; y:40; width:65; height:30; text:'Тест'; color:#ffffffa0; fontSize:16; } ContentClip { x:5; y:5; width:12; height:12; radius:3; } ContentShape { points:'0 0 100 0 100 100 0 100'; color:#ff00ff80; } ContentClipEnd {} } }";
    for clip in [false, true] {
        let source = source.replace("padding:0;", &format!("padding:0; clip:{clip};"));
        compare(
            &source,
            template,
            &[(180, 140, 1.25), (30, 25, 1.25), (180, 140, 2.)],
        );
    }
}

#[test]
fn large_host_does_not_hit_small_scene_raster_limit() {
    let b = Button::new();
    let mut dst = vec![0; 5000 * 1800];
    b.paint_native(&mut dst, 5000, 1800, 2.).unwrap();
    assert_eq!(dst[dst.len() - 1], 0x111319);
    assert_eq!(dst[1000], 0x111319);
    assert_ne!(dst[200 * 5000 + 400], 0x111319);
    assert_eq!(b.raster_stats(), vec![1, 1]);
}

#[test]
fn native_buffer_validation_does_not_panic_or_partially_write() {
    let b = Button::new();
    let mut dst = vec![0x123456; 10];
    for (w, h, s) in [
        (0, 0, 1.),
        (10, 2, 1.),
        (10, 1, f32::NAN),
        (10, 1, 0.),
        (u32::MAX, u32::MAX, 1.),
    ] {
        assert!(b.paint_native(&mut dst, w, h, s).is_err());
        assert_eq!(dst, vec![0x123456; 10]);
    }
}
