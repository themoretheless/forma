use super::*;
#[test]
fn cpu_cache_release_preserves_geometry_and_rebuilds_identical_fallback_pixels() {
    for count in [0, 1, 2] {
        let source = format!(
            "component Demo {{ Frame {{ width:128; height:96; Scroll {{ {} }} }} }}",
            "Button { width:150; height:60; }".repeat(count)
        );
        let runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
        let rgba = runtime.pixels(160, 120, 1.25);
        let mut native = vec![0; 160 * 120];
        runtime.paint_native(&mut native, 160, 120, 1.25).unwrap();
        assert!(
            runtime
                .controls
                .iter()
                .all(|c| c.raster_cache.borrow().is_some())
        );
        let geometry = runtime.vector_snapshot(1.25, true);
        let revision = runtime.visual_revision();
        for _ in 0..2 {
            runtime.release_cpu_cache();
            assert!(runtime.raster_cache.borrow().is_none());
            assert!(
                runtime
                    .controls
                    .iter()
                    .all(|c| c.raster_cache.borrow().is_none())
            );
        }
        assert!(Arc::ptr_eq(&geometry, &runtime.vector_snapshot(1.25, true)));
        assert_eq!(runtime.visual_revision(), revision);
        let mut fallback = vec![0; native.len()];
        runtime.paint_native(&mut fallback, 160, 120, 1.25).unwrap();
        assert_eq!(fallback, native);
        assert_eq!(runtime.pixels(160, 120, 1.25), rgba);
    }
}

#[test]
fn layout_versions_track_scroll_and_same_object_load_but_not_paint() {
    let source = "component Demo { Frame { width:128; height:96; Scroll { Button { width:150; height:160; } } } }";
    let mut runtime = Runtime::from_sources(source, TEMPLATE).unwrap();
    let layout = runtime.layout_revision();
    runtime.pointer(30., 25., 0);
    runtime.tick(16.);
    assert_eq!(runtime.layout_revision(), layout);
    runtime.scroll(0., 0.125);
    assert_ne!(runtime.layout_revision(), layout);
    let layout = runtime.layout_revision();
    runtime.scroll(0., 0.);
    assert_eq!(runtime.layout_revision(), layout);
    let visual = runtime.visual_revision();
    let geometry = runtime.geometry_revision();
    runtime.load_component_native(source, TEMPLATE).unwrap();
    assert_ne!(runtime.layout_revision(), layout);
    assert_ne!(runtime.visual_revision(), visual);
    assert_ne!(runtime.geometry_revision(), geometry);
    assert_eq!(runtime.scroll_offset(), vec![0., 0.]);
}
// Previous full-canvas algorithm is an independent oracle for packed leaf
// compositing, root frame coverage, scrollbars, and background rounding.
impl Runtime {
    fn full_canvas_reference(&self, w: u32, h: u32, s: f32, background: bool) -> Vec<u8> {
        if self.controls.len() == 1 {
            return if background {
                self.controls[0].pixels(w, h, s)
            } else {
                self.controls[0].content_pixels(w, h, s)
            };
        }
        if w == 0
            || h == 0
            || w > 4096
            || h > 4096
            || w as u64 * h as u64 > 8_388_608
            || !s.is_finite()
            || s <= 0.
        {
            return vec![];
        }
        let mut out = vec![0; w as usize * h as usize * 4];
        for (i, c) in self.controls.iter().enumerate() {
            if self.modal_of[i].is_some() {
                continue;
            }
            let pixels = c.content_pixels(w, h, s);
            for (dst, src) in out.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
                crate::raster_cache::over(dst, src);
            }
        }
        let v = self.viewport_rect();
        let [cw, ch] = self.content_size();
        for y in 0..h {
            for x in 0..w {
                let p = &mut out[((y * w + x) * 4) as usize..][..4];
                let px = (x as f32 + 0.5) / s;
                let py = (y as f32 + 0.5) / s;
                if self.scene.scroll {
                    if px < v[0] || py < v[1] || px >= v[0] + v[2] || py >= v[1] + v[3] {
                        p.fill(0);
                    } else {
                        let vertical = ch > v[3]
                            && px >= (v[0] + v[2] - 5.).max(v[0])
                            && py >= v[1] + self.scroll[1] / ch * v[3]
                            && py < v[1] + (self.scroll[1] + v[3]) / ch * v[3];
                        let horizontal = cw > v[2]
                            && py >= (v[1] + v[3] - 5.).max(v[1])
                            && px >= v[0] + self.scroll[0] / cw * v[2]
                            && px < v[0] + (self.scroll[0] + v[2]) / cw * v[2];
                        if vertical || horizontal {
                            p.copy_from_slice(&[110, 130, 170, 255]);
                        }
                    }
                }
            }
        }
        for (m, modal) in self.scene.modals.iter().enumerate() {
            if !self.modal_open[m] {
                continue;
            }
            for p in out.chunks_exact_mut(4) {
                crate::raster_cache::over(p, &modal.backdrop);
            }
            for i in modal.controls.clone() {
                let pixels = self.controls[i].content_pixels(w, h, s);
                for (dst, src) in out.chunks_exact_mut(4).zip(pixels.chunks_exact(4)) {
                    crate::raster_cache::over(dst, src);
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let p = &mut out[((y * w + x) * 4) as usize..][..4];
                let coverage = round_coverage(
                    x,
                    y,
                    s,
                    [0., 0., self.width(), self.height()],
                    self.scene.radius,
                );
                if background {
                    let mut bg = self.scene.background;
                    if !self.scene.clip {
                        bg[3] = (bg[3] as f32 * coverage).round() as u8;
                    }
                    crate::raster_cache::over(&mut bg, p);
                    p.copy_from_slice(&bg);
                }
                if self.scene.clip {
                    p[3] = (p[3] as f32 * coverage).round() as u8;
                    if p[3] == 0 {
                        p.fill(0);
                    }
                }
            }
        }
        out
    }
}

const TEMPLATE: &str = "component Button { Rectangle { radius:6.3; background:Brush { color:#273f6077; hover:#a4e173bb; transition:100ms; }; Border { width:1.4; background:#e7cf8199; } Reveal { width:2.3; color:#dd8822aa; } Text { text:'Я'; fontSize:16; color:#ffffffa0; } ContentText { x:-5.8; y:-10.2; width:52.1; height:13.7; text:'Overflow Я'; fontSize:12.5; color:#f8aaffb0; } ContentShape { points:'-7 -3 64.2 4.1 72.5 21.8 -4.3 29.2'; color:#fc345a61; } ContentClip { x:3.7; y:8.3; width:48.6; height:25.2; radius:4.2; } ContentShape { points:'-20 -30 70 -30 70 50 -20 50'; color:#3467af80; } ContentClip { x:7.2; y:12.1; width:23.4; height:19.7; radius:3.1; } ContentShape { points:'-20 -30 70 -30 70 50 -20 50'; color:#ddeeff90; } ContentClipEnd {} ContentClipEnd {} PointerArea { clicked -> events.clicked(); } } }";

fn assert_reference(runtime: &Runtime, width: u32, height: u32, scale: f32) {
    for background in [false, true] {
        let expected = runtime.full_canvas_reference(width, height, scale, background);
        let actual = runtime.raster(width, height, scale, background);
        if expected != actual {
            let first = expected
                .chunks_exact(4)
                .zip(actual.chunks_exact(4))
                .position(|(a, b)| a != b)
                .unwrap();
            panic!(
                "pixel ({}, {}) differs at DPI {scale}, background {background}: {:?} != {:?}",
                first % width as usize,
                first / width as usize,
                &expected[first * 4..first * 4 + 4],
                &actual[first * 4..first * 4 + 4]
            );
        }
        assert_eq!(runtime.raster(width, height, scale, background), actual);
        let mut reused = vec![0x99; expected.len()];
        runtime
            .paint_rgba(&mut reused, width, height, scale, background)
            .unwrap();
        assert_eq!(
            reused, expected,
            "caller-owned RGBA differs from full-canvas oracle"
        );
        if background {
            let mut native = vec![0; (width * height) as usize];
            runtime
                .paint_native(&mut native, width, height, scale)
                .unwrap();
            for (&actual, expected) in native.iter().zip(expected.chunks_exact(4)) {
                let a = expected[3] as u32;
                let c = |k: usize, bg: u32| (expected[k] as u32 * a + bg * (255 - a) + 127) / 255;
                assert_eq!(actual, c(0, 17) << 16 | c(1, 19) << 8 | c(2, 25));
            }
        }
    }
}

#[test]
fn packed_multicontrol_matches_full_canvas_for_overflow_clips_scroll_and_dpi() {
    for mode in ["overflow:visible;", "clip:true;", "scroll"] {
        let (props, open, close) = if mode == "scroll" {
            ("clip:true;", "Scroll {", "}")
        } else {
            (mode, "", "")
        };
        let source = format!(
            "component Demo {{ Frame {{ width:83.7; height:62.4; padding:5.3; radius:8.1; background:#10203080; {props} {open} Button {{ x:3.7; y:4.2; width:32.2; height:19.7; }} Button {{ x:23.6; y:39.7; width:76.3; height:39.2; }} {close} }} }}"
        );
        let mut runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
        for scale in [1., 1.25, 2.] {
            assert_reference(&runtime, 176, 152, scale);
            runtime.pointer(12.2, 9.4, 0);
            runtime.tick(23.);
            assert_reference(&runtime, 176, 152, scale);
            runtime.pointer(45.7, 43.2, 0);
            runtime.tick(39.);
            runtime.scroll(12.3, 15.7);
            assert_reference(&runtime, 176, 152, scale);
            // Crop/expand the physical host independently of logical layout.
            assert_reference(&runtime, 57, 44, scale);
        }
    }
}

#[test]
fn layers_composite_above_the_scroll_viewport_and_match_the_reference() {
    let source = "component Demo { Frame { width:120; height:90; padding:10; clip:true; radius:6; background:#10203080; Scroll { Button { x:0; y:0; width:80; height:200; } } Modal { key:'m'; backdrop:#ff000080; Button { key:'top'; x:2; y:2; width:40; height:20; } Button { key:'edge'; x:90; y:60; width:60; height:40; } } } }";
    let mut runtime = Runtime::from_sources(source, TEMPLATE).unwrap();
    for scale in [1., 1.25] {
        assert_reference(&runtime, 150, 113, scale);
        runtime.pointer(12., 12., 0);
        runtime.tick(23.);
        runtime.scroll(0., 15.);
        assert_reference(&runtime, 150, 113, scale);
    }
    let open = runtime.content_pixels(120, 90, 1.);
    assert!(
        open[(5 * 120 + 5) * 4 + 3] > 0,
        "layer content paints over the padding outside the viewport"
    );
    runtime.close_modal(0);
    assert_reference(&runtime, 150, 113, 1.);
    assert_eq!(
        runtime.content_pixels(120, 90, 1.)[(5 * 120 + 5) * 4 + 3],
        0
    );
    assert_ne!(runtime.content_pixels(120, 90, 1.), open);
}

#[test]
fn composite_cache_refreshes_after_text_editing_and_validates_native_size() {
    let source = "component Demo { Frame { width:180; height:90; padding:0; Button { width:80; height:30; } Button { width:80; height:30; } } }";
    let template = "component Button { Rectangle { ContentInput { width:80; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }";
    let mut runtime = Runtime::from_sources(source, template).unwrap();
    let before = runtime.pixels(180, 90, 1.);
    runtime.focus_control(0);
    runtime.text_insert("Я");
    let after = runtime.pixels(180, 90, 1.);
    assert_ne!(before, after);
    assert_reference(&runtime, 180, 90, 1.);
    assert!(runtime.paint_native(&mut [0; 3], 180, 90, 1.).is_err());
    assert!(runtime.paint_native(&mut [], 0, 0, 1.).is_err());
    assert!(runtime.pixels(u32::MAX, 4, 1.).is_empty());
}

#[test]
fn empty_frame_raster_and_native_have_no_child_access() {
    for content in ["", "Scroll {}"] {
        let source = format!(
            "component Demo {{ Frame {{ width:24; height:18; padding:2; radius:3.3; background:#27486980; {content} }} }}"
        );
        let runtime = Runtime::from_sources(&source, TEMPLATE).unwrap();
        assert_eq!(runtime.control_count(), 0);
        assert_reference(&runtime, 40, 30, 1.25);
        assert_reference(&runtime, 12, 10, 2.);
    }
}

#[test]
fn warmed_composite_refreshes_when_reconciliation_only_changes_reveal() {
    let source = "component Demo { Frame { width:180; height:100; padding:10; gap:10; Button { key:'a'; width:100; height:30; } Button { key:'b'; width:100; height:30; } } }";
    let template = "component Button { Rectangle { radius:6; background:#112233; Reveal { width:2; color:#ffaabb; } PointerArea { clicked -> events.clicked(); } } }";
    let mut previous = Runtime::from_sources(source, template).unwrap();
    previous.reveal_pointer(9., 40., true);
    let expected = previous.pixels(180, 100, 1.);
    let mut current = Runtime::from_sources(source, template).unwrap();
    let before = current.pixels(180, 100, 1.);
    assert_ne!(before, expected);
    let revision = current.visual_revision();
    current.preserve_interaction(&previous);
    assert!(current.visual_revision() > revision);
    assert_eq!(current.pixels(180, 100, 1.), expected);
    assert_reference(&current, 180, 100, 1.);
}
