use forma::{
    DEFAULT_RESOURCE_BUDGET_BYTES, DirtyFlags, ResourceKind, Runtime, clear_resource_caches,
    display_list::RasterizeOpts,
    form_document::{Document, Properties, Value},
    resource_cache_stats, set_resource_budget_bytes,
};
use std::{collections::BTreeMap, sync::Arc};

struct BudgetResetGuard;

impl Drop for BudgetResetGuard {
    fn drop(&mut self) {
        clear_resource_caches();
        set_resource_budget_bytes(DEFAULT_RESOURCE_BUDGET_BYTES);
    }
}

fn multi_control_ui_files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "components/Button.ui".to_owned(),
            forma::BUTTON_COMPONENT.to_owned(),
        ),
        (
            "App.ui".to_owned(),
            "component App { \
                state tint: String = '#224466'; \
                state cardWidth: Number = 100; \
                Column { \
                    width: 260; height: 140; padding: 10; gap: 8; \
                    Button { key: 'card'; width: state.cardWidth; height: 36; background: state.tint; text: 'Save'; } \
                    TextInput { key: 'input'; width: 180; height: 36; value: ''; } \
                } \
            }"
            .to_owned(),
        ),
    ])
}

#[test]
fn stage1_dirty_flags_and_in_place_reconciliation() {
    let files = multi_control_ui_files();
    let mut runtime = Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    runtime.clear_dirty_flags();
    assert_eq!(runtime.dirty_flags(), DirtyFlags::NONE);

    // Focus the TextInput and edit its text; verify in-place reconciliation keeps focus & text.
    assert!(runtime.focus_control(1));
    assert!(runtime.text_insert("abc"));
    assert_eq!(runtime.text_value(1), "abc");
    runtime.clear_dirty_flags();

    let mut doc = Document::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
    let mut paint_state = Properties::new();
    paint_state.insert("tint".into(), Value::Text("#883344".into()));
    doc.states[0].extend(paint_state);
    let paint_ir = doc.render_scene_ir().unwrap();

    let layout_before = runtime.layout_revision();
    let comp_before = runtime.composition_revision();
    let paint_flags = runtime.reconcile_scene_ir(&paint_ir).unwrap();
    assert_eq!(paint_flags, DirtyFlags::PAINT);
    assert_eq!(runtime.dirty_flags(), DirtyFlags::PAINT);
    assert_eq!(runtime.layout_revision(), layout_before);
    assert_eq!(runtime.composition_revision(), comp_before);
    assert!(runtime.control_focused(1));
    assert_eq!(runtime.text_value(1), "abc");

    // Reconciling an identical SceneIr produces zero dirty flags.
    runtime.clear_dirty_flags();
    let noop_flags = runtime.reconcile_scene_ir(&paint_ir).unwrap();
    assert_eq!(noop_flags, DirtyFlags::NONE);
    assert_eq!(runtime.dirty_flags(), DirtyFlags::NONE);

    // Layout change flips DirtyFlags::LAYOUT | DirtyFlags::PAINT and bumps layout_revision.
    doc.states[0].insert("cardWidth".into(), Value::Number(140.0));
    let layout_ir = doc.render_scene_ir().unwrap();
    let layout_flags = runtime.reconcile_scene_ir(&layout_ir).unwrap();
    assert!(layout_flags.contains(DirtyFlags::LAYOUT | DirtyFlags::PAINT));
    assert_ne!(runtime.layout_revision(), layout_before);
    assert!(runtime.control_focused(1));
    assert_eq!(runtime.text_value(1), "abc");
}

#[test]
fn stage2_unified_display_list_ir_and_diagnostics() {
    let source = "component Demo { Frame { width: 160; height: 100; clip: true; radius: 8; padding: 8; \
        Button { key: 'a'; width: 120; height: 40; text: 'IR Test'; } } }";
    let template = "component Button { \
        Rectangle { radius: 6; background: #224466; \
        Border { width: 1; background: #88aaff; } \
        Text { text: props.text; fontSize: 13; color: #ffffff; } \
        ContentShape { points: '4 4 12 4 8 12'; color: #ffcc44; } \
        PointerArea { clicked -> events.clicked(); } } }";
    let runtime = Runtime::from_sources(source, template).unwrap();
    let list = runtime.vector_snapshot(1., true);
    assert!(!list.items.is_empty());
    assert!(!list.clips.is_empty());
    assert!(
        list.items
            .iter()
            .any(|item| item.resource_id.map(|id| id.kind()) == Some(ResourceKind::GlyphAtlas))
    );
    assert!(
        list.items
            .iter()
            .any(|item| item.resource_id.map(|id| id.kind()) == Some(ResourceKind::SvgShape))
    );

    let diag_json = runtime.display_list_diagnostics_json(1., true);
    let parsed: serde_json::Value = serde_json::from_str(&diag_json).unwrap();
    assert_eq!(parsed["itemCount"].as_u64(), Some(list.items.len() as u64));
    assert_eq!(parsed["clipCount"].as_u64(), Some(list.clips.len() as u64));

    let mut ir_rgba = vec![0u8; 160 * 100 * 4];
    let paints = runtime.gpu_paints();
    let mut opts = RasterizeOpts::new(160, 100, 1.);
    opts.paints = &paints;
    list.rasterize_rgba(&opts, &mut ir_rgba).unwrap();

    let mut cpu_rgba = vec![0u8; 160 * 100 * 4];
    runtime
        .paint_rgba(&mut cpu_rgba, 160, 100, 1., true)
        .unwrap();
    let sum_diff: u64 = ir_rgba
        .iter()
        .zip(&cpu_rgba)
        .map(|(&a, &b)| a.abs_diff(b) as u64)
        .sum();
    let mean_diff = sum_diff as f64 / ir_rgba.len() as f64;
    assert!(
        mean_diff < 2.0,
        "unified DisplayList IR raster diverged from Runtime::paint_rgba (mean {mean_diff:.4})"
    );
}

#[test]
fn stage3_f15_clip_bounds_dirty_rects_and_f16_nested_scroll() {
    // F-15: Deeply nested clip inside tall scrollable content (4000px) renders cleanly in small viewport.
    let f15_source = "component Demo { Frame { width: 160; height: 120; clip: true; Scroll { Button { width: 160; height: 4000; } } } }";
    let f15_template = "component Button { \
        Rectangle { background: #142238; \
        ContentClip { x: 10; y: 10; width: 80; height: 60; radius: 6; } \
        ContentClip { x: 15; y: 15; width: 60; height: 45; radius: 4; } \
        ContentShape { points: '0 0 140 0 140 3900 0 3900'; color: #44aaee; } \
        ContentClipEnd {} ContentClipEnd {} } }";
    let f15_rt = Runtime::from_sources(f15_source, f15_template).unwrap();
    let mut small_buf = vec![0u8; 160 * 120 * 4];
    f15_rt
        .paint_rgba(&mut small_buf, 160, 120, 1., true)
        .unwrap();
    assert!(small_buf.chunks_exact(4).any(|p| p[2] > 150));

    // Dirty-rect incremental CPU compositing: 32-control form produces bit-identical output on partial damage.
    let mut controls = String::new();
    for i in 0..32 {
        controls.push_str(&format!(
            "Button {{ key: 'c{i}'; x: {}; y: {}; width: 106; height: 26; }}",
            12 + (i % 4) * 124,
            10 + (i / 4) * 34
        ));
    }
    let form_source = format!(
        "component Demo {{ Frame {{ width: 512; height: 320; padding: 0; radius: 8; background: #102030; {controls} }} }}"
    );
    let form_template = "component Button { \
        Rectangle { radius: 6; background: Brush { color: #324674; hover: #99bbff; transition: 160ms; }; \
        Border { width: 1; background: #d4e0ff; } \
        Text { text: 'Btn'; fontSize: 12; color: #ffffff; } \
        PointerArea { clicked -> events.clicked(); } } }";
    let (w, h, scale) = (640u32, 400u32, 1.25f32);
    let mut rt = Runtime::from_sources(&form_source, form_template).unwrap();
    let mut inc_out = vec![0u8; (w * h * 4) as usize];
    rt.paint_rgba(&mut inc_out, w, h, scale, true).unwrap();

    rt.pointer(30., 20., 0);
    rt.tick(32.);
    rt.paint_rgba(&mut inc_out, w, h, scale, true).unwrap();
    let rects = rt.last_dirty_rects();
    assert!(!rects.is_empty());
    let dirty_area: u64 = rects
        .iter()
        .map(|&[x0, y0, x1, y1]| (x1 - x0) as u64 * (y1 - y0) as u64)
        .sum();
    let total_area = w as u64 * h as u64;
    assert!(
        dirty_area * 4 < total_area,
        "partial single-control hover damaged too much area: {dirty_area} / {total_area}"
    );

    rt.release_cpu_cache();
    let mut cold_out = vec![0u8; (w * h * 4) as usize];
    rt.paint_rgba(&mut cold_out, w, h, scale, true).unwrap();
    assert_eq!(
        inc_out, cold_out,
        "incremental dirty-rect CPU composite must be bit-identical to cold full composite"
    );

    // F-16: Nested Scroll area scroll keeps DisplayList identity intact.
    let mut inner_rows = String::new();
    for i in 0..8 {
        inner_rows.push_str(&format!(
            "Button {{ key: 'r{i}'; width: 90; height: 40; text: 'Row {i}'; }}"
        ));
    }
    let nested_ui = BTreeMap::from([
        (
            "components/Button.ui".to_owned(),
            forma::BUTTON_COMPONENT.to_owned(),
        ),
        (
            "App.ui".to_owned(),
            format!(
                "component App {{ Column {{ width: 220; height: 220; Scroll {{ width: 120; height: 100; Column {{ width: 100; height: 400; {inner_rows} }} }} }} }}"
            ),
        ),
    ]);
    let mut scroll_rt = Runtime::from_ui_files(&nested_ui, "App.ui", &Default::default()).unwrap();
    let before_list = scroll_rt.vector_snapshot(1., false);
    scroll_rt.pointer(20., 20., 0);
    scroll_rt.scroll(0., 30.);
    let after_list = scroll_rt.vector_snapshot(1., false);
    assert!(
        Arc::ptr_eq(&before_list, &after_list),
        "F-16 regression: scrolling a nested ScrollArea rebuilt DisplayList"
    );
}

#[test]
fn stage4_shared_glyph_and_svg_lru_budget() {
    let _guard = BudgetResetGuard;
    clear_resource_caches();
    set_resource_budget_bytes(3072);

    // Populate SVGs and glyph outlines past the 3 KiB cap to exercise global LRU eviction.
    for i in 0..8 {
        let svg = format!(
            "<svg viewBox='0 0 24 24'><rect x='{}' y='1' width='12' height='10' fill='#ff00aa'/><circle cx='12' cy='12' r='{}' stroke='#112233'/></svg>",
            i + 1,
            3 + i
        );
        let parsed = Document::parse_svg_image(&svg).unwrap();
        assert!(!parsed.shapes.is_empty());
    }
    for (idx, word) in ["Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta"]
        .into_iter()
        .enumerate()
    {
        let src = format!(
            "component Demo {{ Frame {{ width: 120; height: 40; Button {{ width: 100; height: 30; text: '{word}'; }} }} }}"
        );
        let size = 12 + idx;
        let tmpl = format!(
            "component Button {{ Rectangle {{ background: #223344; Text {{ text: props.text; fontSize: {size}; color: #ffffff; }} }} }}"
        );
        let rt = Runtime::from_sources(&src, &tmpl).unwrap();
        drop(rt.vector_snapshot(1., true));
    }

    let pressure = resource_cache_stats();
    assert!(
        pressure.budget_in_use_bytes <= pressure.budget_limit_bytes,
        "cache exceeded budget limit: {} > {}",
        pressure.budget_in_use_bytes,
        pressure.budget_limit_bytes
    );
    assert!(
        pressure.glyph_evictions + pressure.svg_evictions > 0,
        "expected LRU eviction under 3 KiB cap, got stats={pressure:?}"
    );

    // At default budget, repeated SVGs and glyph runs produce hits in both caches.
    set_resource_budget_bytes(DEFAULT_RESOURCE_BUDGET_BYTES);
    let repeated_svg =
        "<svg viewBox='0 0 24 24'><rect x='2' y='2' width='16' height='16' fill='#33ccff'/></svg>";
    let _ = Document::parse_svg_image(repeated_svg).unwrap();
    let _ = Document::parse_svg_image(repeated_svg).unwrap();

    let hit_src = "component Demo { Frame { width: 160; height: 50; Button { width: 140; height: 40; text: 'AAAAAA'; } } }";
    let hit_rt = Runtime::from_sources(hit_src, forma::BUTTON_COMPONENT).unwrap();
    drop(hit_rt.vector_snapshot(1., true));

    let steady = resource_cache_stats();
    assert!(steady.svg_hits > 0, "expected SVG cache hit: {steady:?}");
    assert!(
        steady.glyph_hits > 0,
        "expected glyph atlas hit: {steady:?}"
    );
}

#[cfg(feature = "gpu")]
#[test]
fn stage5_cpu_fallback_renders_when_gpu_unavailable() {
    let source = "component Demo { Frame { width: 128; height: 96; Button { width: 100; height: 40; text: 'Fallback'; } } }";
    let rt = Runtime::from_sources(source, forma::BUTTON_COMPONENT).unwrap();
    let mut out = vec![0u8; 128 * 96 * 4];
    let used_gpu = forma::gpu::render_rgba_with_cpu_fallback(
        None,
        &mut None,
        &rt,
        &mut out,
        forma::gpu::Viewport::new(128, 96, 1.),
        true,
        true,
    )
    .unwrap();
    assert!(!used_gpu, "None renderer must fall back cleanly to CPU");
    assert!(out.chunks_exact(4).all(|px| px[3] == 255));
    assert!(out.chunks_exact(4).any(|px| px != out[0..4].as_ref()));
}

#[cfg(feature = "gpu")]
#[test]
#[ignore = "requires a real GPU"]
fn stage5_gpu_render_rgba_damaged_scissor_f16_zero_uploads_and_device_loss_fallback() {
    use forma::gpu::{
        GpuOffscreenTarget, OffscreenRender, Renderer, Viewport, render_rgba_with_cpu_fallback,
    };
    use forma::gpu_runtime as wgpu;

    pollster::block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
            .await
            .unwrap();

        // F-16 on real GPU: scrolling a nested Scroll container performs zero geometry uploads.
        let mut rows = String::new();
        for i in 0..8 {
            rows.push_str(&format!(
                "Button {{ key: 'r{i}'; width: 90; height: 40; text: 'R{i}'; }}"
            ));
        }
        let nested_ui = BTreeMap::from([
            (
                "components/Button.ui".to_owned(),
                forma::BUTTON_COMPONENT.to_owned(),
            ),
            (
                "App.ui".to_owned(),
                format!(
                    "component App {{ Column {{ width: 200; height: 200; Scroll {{ width: 110; height: 90; Column {{ width: 100; height: 320; {rows} }} }} }} }}"
                ),
            ),
        ]);
        let mut nested_rt =
            Runtime::from_ui_files(&nested_ui, "App.ui", &Default::default()).unwrap();
        let mut nested_target: Option<GpuOffscreenTarget> = None;
        let mut nested_rgba_0 = vec![0u8; 200 * 200 * 4];
        let mut nested_rgba_1 = vec![0u8; 200 * 200 * 4];
        let nested_vp = Viewport::new(200, 200, 1.);
        let nested_opts = OffscreenRender::new(nested_vp, true, true);
        renderer
            .render_rgba(
                &nested_rt,
                &mut nested_target,
                &mut nested_rgba_0,
                nested_opts,
            )
            .unwrap();
        let uploads_before_scroll = renderer.resource_stats().geometry_uploads_total;
        nested_rt.pointer(20., 20., 0);
        nested_rt.scroll(0., 40.);
        renderer
            .render_rgba(
                &nested_rt,
                &mut nested_target,
                &mut nested_rgba_1,
                nested_opts,
            )
            .unwrap();
        assert_eq!(
            renderer.resource_stats().geometry_uploads_total,
            uploads_before_scroll,
            "nested ScrollArea scroll must reuse GPU geometry without re-upload"
        );
        assert_ne!(
            nested_rgba_0, nested_rgba_1,
            "nested ScrollArea GPU output must visually reflect the scroll offset"
        );

        // render_rgba + draw_damaged scissor pass matches full GPU render byte-for-byte.
        let mut controls = String::new();
        for i in 0..16 {
            controls.push_str(&format!(
                "Button {{ key: 'g{i}'; x: {}; y: {}; width: 80; height: 28; }}",
                8 + (i % 4) * 90,
                8 + (i / 4) * 36
            ));
        }
        let form_source = format!(
            "component Demo {{ Frame {{ width: 384; height: 192; padding: 0; radius: 8; background: #141c2b; {controls} }} }}"
        );
        let form_template = "component Button { \
            Rectangle { radius: 6; background: Brush { color: #314873; hover: #99bbff; transition: 0ms; }; \
            Text { text: 'GPU'; fontSize: 12; color: #ffffff; } \
            PointerArea { clicked -> events.clicked(); } } }";
        let mut rt = Runtime::from_sources(&form_source, form_template).unwrap();
        let vp = Viewport::new(384, 192, 1.);
        let full_opts = OffscreenRender::new(vp, true, true);
        let mut scratch_cpu = vec![0u8; 384 * 192 * 4];
        rt.paint_rgba(&mut scratch_cpu, 384, 192, 1., true).unwrap();

        let mut target_inc: Option<GpuOffscreenTarget> = None;
        let mut dummy = vec![0u8; 384 * 192 * 4];
        renderer
            .render_rgba(&rt, &mut target_inc, &mut dummy, full_opts)
            .unwrap();

        rt.pointer(30., 20., 0);
        rt.paint_rgba(&mut scratch_cpu, 384, 192, 1., true).unwrap();
        let rects = rt.last_dirty_rects();
        assert!(!rects.is_empty());

        let mut damaged_out = vec![0u8; 384 * 192 * 4];
        renderer
            .render_rgba(
                &rt,
                &mut target_inc,
                &mut damaged_out,
                full_opts.with_dirty_rects(&rects),
            )
            .unwrap();

        let mut target_full: Option<GpuOffscreenTarget> = None;
        let mut full_out = vec![0u8; 384 * 192 * 4];
        renderer
            .render_rgba(&rt, &mut target_full, &mut full_out, full_opts)
            .unwrap();
        assert_eq!(
            damaged_out, full_out,
            "draw_damaged scissor pass must match full GPU render byte-for-byte"
        );

        // Latched GPU validation fault triggers automatic fallback to CPU DisplayList reference.
        let mut invalid_encoder = renderer.device.create_command_encoder(&Default::default());
        {
            let mut pass = invalid_encoder.begin_compute_pass(&Default::default());
            pass.dispatch_workgroups(1, 1, 1);
        }
        drop(invalid_encoder.finish());
        assert!(renderer.check_health().is_err());

        let mut fallback_out = vec![0u8; 384 * 192 * 4];
        let used_gpu = render_rgba_with_cpu_fallback(
            Some(&mut renderer),
            &mut target_full,
            &rt,
            &mut fallback_out,
            vp,
            true,
            true,
        )
        .unwrap();
        assert!(
            !used_gpu,
            "faulted GPU renderer must fail over to CPU reference"
        );
        let max_diff = fallback_out
            .iter()
            .zip(&full_out)
            .map(|(&a, &b)| a.abs_diff(b))
            .max()
            .unwrap_or(0);
        assert!(
            max_diff <= 4,
            "CPU fallback differed from GPU by {max_diff}/255"
        );
    });
}
