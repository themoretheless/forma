use forma_vector::{
    display_list::{DisplayList, TileScratch, STRIDE, TILE},
    Button,
};
use std::sync::Arc;

#[test]
fn shared_snapshot_keeps_identity_during_interaction_and_viewport_changes() {
    let mut button = Button::new();
    let first = button.vector_snapshot(1., true);
    let commands = first.commands.as_ptr();
    let edges = first.edges.as_ptr();
    for i in 0..8 {
        button.pointer(if i % 2 == 0 { 100. } else { -1. }, 90., 0);
        button.focus(i % 2 == 0);
        button.tick(16.);
        let current = button.vector_snapshot(1., true);
        assert!(Arc::ptr_eq(&first, &current));
        assert_eq!(current.commands.as_ptr(), commands);
        assert_eq!(current.edges.as_ptr(), edges);
    }
    for (width, height) in [(64, 32), (400, 200), (1024, 768)] {
        button.gpu_tiles(width, height, 1., true);
        assert!(Arc::ptr_eq(&first, &button.vector_snapshot(1., true)));
    }
    // Existing callers still receive the same borrowed DisplayList API.
    let borrowed: std::cell::Ref<'_, DisplayList> = button.vector_list(1., true);
    assert!(std::ptr::eq(&*borrowed, first.as_ref()));
}

#[test]
fn shared_snapshot_invalidates_for_geometry_inputs_and_source_replacement() {
    let mut button = Button::from_source(
        "component Demo { Frame { width:40; height:30; Scroll { Button { width:80; height:60; text:'О'; } } } }",
    ).unwrap();
    let original = button.vector_snapshot(1., true);
    let dpi = button.vector_snapshot(2., true);
    assert!(!Arc::ptr_eq(&original, &dpi));
    let foreground = button.vector_snapshot(2., false);
    assert!(!Arc::ptr_eq(&dpi, &foreground));
    button.scroll(0., 0.);
    assert!(Arc::ptr_eq(&foreground, &button.vector_snapshot(2., false)));
    button.scroll(5., 5.);
    let scrolled = button.vector_snapshot(2., false);
    assert!(!Arc::ptr_eq(&foreground, &scrolled));
    assert_ne!(foreground.commands, scrolled.commands);

    button.load_source(forma_vector::EXAMPLE).unwrap();
    let source = button.vector_snapshot(2., false);
    assert!(!Arc::ptr_eq(&scrolled, &source));
    // Successful loading is a replacement even if the text is byte-identical.
    button.load_source(forma_vector::EXAMPLE).unwrap();
    let reloaded = button.vector_snapshot(2., false);
    assert!(!Arc::ptr_eq(&source, &reloaded));
    assert!(button.load_source("not valid markup").is_err());
    assert!(Arc::ptr_eq(&reloaded, &button.vector_snapshot(2., false)));
    button
        .load_component(forma_vector::EXAMPLE, forma_vector::BUTTON_COMPONENT)
        .unwrap();
    let component = button.vector_snapshot(2., false);
    assert!(!Arc::ptr_eq(&reloaded, &component));
}

#[test]
fn held_snapshot_prevents_aba_after_owner_drop_or_reuse() {
    let mut button = Button::new();
    let held = button.vector_snapshot(1., true);
    button = Button::new();
    assert!(!Arc::ptr_eq(&held, &button.vector_snapshot(1., true)));
    drop(button);
    assert_eq!(Arc::strong_count(&held), 1);
    for _ in 0..64 {
        let replacement = Button::new();
        let fresh = replacement.vector_snapshot(1., true);
        assert!(!Arc::ptr_eq(&held, &fresh));
        assert_eq!(held.commands, fresh.commands);
    }
    // Arc preserves the ability to move a model/renderer to another thread.
    fn assert_send<T: Send>() {}
    assert_send::<Button>();
    assert_send::<Arc<DisplayList>>();
    #[cfg(feature = "gpu")]
    assert_send::<forma_vector::gpu::Renderer>();
}

#[test]
fn list_is_vectors_not_pixels_and_animation_keeps_geometry() {
    let mut b = Button::new();
    let commands = b.gpu_commands(2., true);
    let edges = b.gpu_edges(2., true);
    assert_eq!(commands.len() % STRIDE, 0);
    assert_eq!(edges.len() % 4, 0);
    assert!(
        commands
            .chunks_exact(STRIDE)
            .any(|c| c[0] == 1. && c[1] == 1.),
        "glyph winding paths"
    );
    assert!(
        commands.chunks_exact(STRIDE).any(|c| c[0] == 4.),
        "analytic button primitive"
    );
    assert!(
        commands.len() + edges.len() < 400 * 200,
        "geometry, not a full image"
    );
    let before = b.gpu_params(800, 400, 2., true);
    b.pointer(100., 90., 0);
    b.tick(80.);
    assert_eq!(commands, b.gpu_commands(2., true));
    assert_eq!(edges, b.gpu_edges(2., true));
    assert_ne!(before, b.gpu_params(800, 400, 2., true));
    assert_eq!(b.raster_stats(), vec![0, 0]);
}
#[test]
fn every_tile_keeps_balanced_clips_and_painter_order() {
    let b = Button::new();
    let commands = b.gpu_commands(1., true);
    let tiles = b.gpu_tiles(800, 400, 1., true);
    for pair in tiles[..25 * 13 * 2].chunks_exact(2) {
        let indices = &tiles[pair[0] as usize..(pair[0] + pair[1]) as usize];
        assert!(indices.windows(2).all(|w| w[0] < w[1]));
        let mut depth = 0;
        for &i in indices {
            let kind = commands[i as usize * STRIDE];
            if kind == 2. {
                depth += 1;
            }
            if kind == 3. {
                depth -= 1;
            }
            assert!(depth >= 0 && depth < 32);
        }
        assert_eq!(depth, 0);
    }
}
#[test]
fn scroll_and_reload_rebuild_vectors() {
    let mut b=Button::from_source("component Demo { Frame { width:40; height:30; Scroll { Button { width:80; height:60; text:'О'; } } } }").unwrap();
    let before = b.gpu_commands(1., true);
    b.scroll(10., 10.);
    assert_ne!(before, b.gpu_commands(1., true));
    b.load_source(forma_vector::EXAMPLE).unwrap();
    assert_ne!(before, b.gpu_commands(1., true));
    assert!(b.gpu_tiles(0, 1, 1., true).is_empty());
    assert!(b.gpu_tiles(u32::MAX, 1, 1., true).is_empty());
}
#[test]
fn empty_tiles_do_not_scan_all_glyphs() {
    let b = Button::new();
    let tiles = b.gpu_tiles(3840, 2160, 2., true);
    let commands = b.gpu_commands(2., true);
    let i = (120 * 68 - 1) * 2;
    let last = &tiles[tiles[i] as usize..(tiles[i] + tiles[i + 1]) as usize];
    assert!(
        last.is_empty(),
        "no draw or clip commands in an empty distant tile"
    );
    assert!(!commands.is_empty());
}

// Independent copy of the pre-optimization binner. Exact list equality proves
// unchanged un-clipped bins; pixel tests below validate clipped compositions.
fn legacy_tiles(list: &DisplayList, width: u32, height: u32, scale: f32) -> Vec<u32> {
    let cols = width.div_ceil(TILE);
    let rows = height.div_ceil(TILE);
    let mut bins = vec![Vec::new(); (cols * rows) as usize];
    for (id, c) in list.commands.chunks_exact(STRIDE).enumerate() {
        let (x0, y0, x1, y1) = if c[0] == 2. || c[0] == 3. {
            (0, 0, cols, rows)
        } else {
            (
                ((c[4] * scale - 1.).max(0.) / TILE as f32)
                    .floor()
                    .min(cols as f32) as u32,
                ((c[5] * scale - 1.).max(0.) / TILE as f32)
                    .floor()
                    .min(rows as f32) as u32,
                (((c[4] + c[6]) * scale + 1.).max(0.) / TILE as f32)
                    .ceil()
                    .min(cols as f32) as u32,
                (((c[5] + c[7]) * scale + 1.).max(0.) / TILE as f32)
                    .ceil()
                    .min(rows as f32) as u32,
            )
        };
        for y in y0..y1 {
            for x in x0..x1 {
                bins[(y * cols + x) as usize].push(id as u32);
            }
        }
    }
    let mut result = vec![0; bins.len() * 2];
    for (i, bin) in bins.into_iter().enumerate() {
        result[i * 2] = result.len() as u32;
        result[i * 2 + 1] = bin.len() as u32;
        result.extend(bin);
    }
    result
}

fn boundary_scene() -> DisplayList {
    let mut list = DisplayList {
        commands: Vec::new(),
        edges: Vec::new(),
    };
    let mut add = |kind: f32, bounds: [f32; 4]| {
        let mut command = [0.; STRIDE];
        command[0] = kind;
        command[4..8].copy_from_slice(&bounds);
        list.commands.extend(command);
    };
    add(2., [0.25, 0.75, 64., 64.]);
    add(0., [-0.75, 31.75, 33., 0.5]);
    add(2., [50.25, -2., 10., 10.]);
    add(1., [63.8, 31.9, 31.5, 32.5]);
    add(3., [0.; 4]);
    add(3., [0.; 4]);
    // Separate groups must still balance, including an empty group.
    add(2., [1000., 1000., 20., 20.]);
    add(3., [0.; 4]);
    for n in 0..80 {
        add(
            (n % 2) as f32,
            [n as f32 * 3.25 - 12., n as f32 * 1.75 - 15., 7.5, 10.25],
        );
    }
    list
}

fn without_clips(list: DisplayList) -> DisplayList {
    DisplayList {
        commands: list
            .commands
            .chunks_exact(STRIDE)
            .filter(|c| c[0] != 2. && c[0] != 3.)
            .flatten()
            .copied()
            .collect(),
        edges: list.edges,
    }
}

#[test]
fn unclipped_flat_binner_is_byte_identical_to_legacy_at_tile_and_dpi_boundaries() {
    let b = Button::new();
    let generated = b.vector_list(2., true);
    let cases = [
        without_clips(boundary_scene()),
        without_clips(DisplayList {
            commands: generated.commands.clone(),
            edges: generated.edges.clone(),
        }),
    ];
    let mut scratch = TileScratch::default();
    for list in &cases {
        for (width, height) in [
            (0, 0),
            (1, 1),
            (31, 33),
            (32, 32),
            (33, 65),
            (800, 400),
            (3840, 2160),
        ] {
            for scale in [1., 1.25, 2., 3.] {
                let expected = legacy_tiles(list, width, height, scale);
                assert_eq!(
                    list.tiles_into(width, height, scale, &mut scratch),
                    expected,
                    "{width}x{height} scale={scale}"
                );
                assert_eq!(list.tiles(width, height, scale), expected);
            }
        }
    }
}

#[test]
fn tile_scratch_retains_capacity_and_output_pointer_across_repeated_resize() {
    let list = boundary_scene();
    let mut scratch = TileScratch::default();
    let pointer = list.tiles_into(3840, 2160, 3., &mut scratch).as_ptr();
    let capacity = scratch.capacity_bytes();
    assert!(capacity > 0);
    for _ in 0..3 {
        for (width, height) in [(800, 400), (33, 33), (0, 0), (3840, 2160)] {
            let actual = list.tiles_into(width, height, 3., &mut scratch);
            assert_eq!(actual, list.tiles(width, height, 3.));
            assert_eq!(actual.as_ptr(), pointer, "output allocation changed");
            assert_eq!(
                scratch.capacity_bytes(),
                capacity,
                "scratch grew after largest viewport warmup"
            );
        }
    }
}

#[test]
fn tile_index_depends_on_grid_dimensions_not_exact_framebuffer_size() {
    let list = boundary_scene();
    for scale in [1., 1.25, 2.] {
        assert_eq!(list.tiles(33, 65, scale), list.tiles(64, 96, scale));
        assert_eq!(list.tiles(3810, 2145, scale), list.tiles(3840, 2176, scale));
    }
}

fn add_command(list: &mut DisplayList, kind: f32, bounds: [f32; 4], color: [f32; 4], radius: f32) {
    let mut c = [0.; STRIDE];
    c[0] = kind;
    c[4..8].copy_from_slice(&bounds);
    c[8..12].copy_from_slice(&color);
    c[12] = radius;
    list.commands.extend(c);
}

fn empty_list() -> DisplayList {
    DisplayList {
        commands: Vec::new(),
        edges: Vec::new(),
    }
}

fn tile_commands(tiles: &[u32], width: u32, x: u32, y: u32) -> &[u32] {
    let tile = ((y / TILE) * width.div_ceil(TILE) + x / TILE) as usize * 2;
    &tiles[tiles[tile] as usize..(tiles[tile] + tiles[tile + 1]) as usize]
}

#[test]
fn empty_disjoint_and_zero_area_clip_groups_have_no_tile_entries() {
    for inner in [[10., 0., 5., 5.], [1., 1., 0., 5.], [1., 1., 5., 0.]] {
        let mut list = empty_list();
        add_command(&mut list, 2., [0., 0., 5., 5.], [0.; 4], 0.);
        add_command(&mut list, 2., inner, [0.; 4], 0.);
        add_command(&mut list, 0., [0., 0., 20., 20.], [1., 0., 0., 0.5], 0.);
        add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
        add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
        // Explicit empty group outside any ancestor.
        add_command(&mut list, 2., [0., 0., 30., 30.], [0.; 4], 0.);
        add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
        for scale in [1., 1.25, 2.] {
            let tiles = list.tiles(128, 128, scale);
            assert_eq!(
                tiles.len(),
                4 * 4 * 2,
                "invisible commands leaked into tiles"
            );
            assert!(tiles.chunks_exact(2).all(|pair| pair[1] == 0));
        }
    }
}

// Small independent CPU interpreter of the rounded-rectangle/group part of the
// WGSL contract. It compares old/new bin membership at every pixel, including
// nested translucent groups, without relying on the culling implementation.
fn sample_rectangles(list: &DisplayList, ids: &[u32], x: u32, y: u32, scale: f32) -> [f32; 4] {
    fn over(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
        std::array::from_fn(|i| a[i] + b[i] * (1. - a[3]))
    }
    let p = [(x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale];
    let mut parents = [[0.; 4]; 32];
    let mut masks = [0.; 32];
    let mut depth = 0usize;
    let mut result = [0.; 4];
    for &id in ids {
        let c = &list.commands[id as usize * STRIDE..(id as usize + 1) * STRIDE];
        if c[0] == 3. {
            assert!(depth > 0, "unbalanced pop in culled tile");
            depth -= 1;
            result = over(result.map(|v| v * masks[depth]), parents[depth]);
            continue;
        }
        let coverage = if c[6] <= 0. || c[7] <= 0. {
            0.
        } else {
            let r = c[12].min(c[6].min(c[7]) * 0.5);
            let dx = (p[0] - c[4] - c[6] * 0.5).abs() - (c[6] * 0.5 - r);
            let dy = (p[1] - c[5] - c[7] * 0.5).abs() - (c[7] * 0.5 - r);
            let distance = dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - r;
            (0.5 - distance * scale).clamp(0., 1.)
        };
        if c[0] == 2. {
            parents[depth] = result;
            masks[depth] = coverage;
            depth += 1;
            result = [0.; 4];
        } else {
            assert_eq!(c[0], 0., "fixture must contain only rectangles and clips");
            let alpha = c[11] * coverage;
            result = over([c[8] * alpha, c[9] * alpha, c[10] * alpha, alpha], result);
        }
    }
    assert_eq!(depth, 0, "unclosed group in culled tile");
    result
}

#[test]
fn culled_translucent_nested_groups_match_legacy_pixels_at_fractional_dpi() {
    let mut list = empty_list();
    add_command(&mut list, 0., [0., 0., 90., 75.], [0.1, 0.2, 0.3, 0.7], 7.);
    add_command(&mut list, 2., [9.25, 8.75, 58.5, 48.5], [0.; 4], 9.);
    add_command(&mut list, 0., [-4., 2., 80., 60.], [0.8, 0.2, 0.1, 0.5], 5.);
    add_command(&mut list, 2., [25.8, 21.1, 50.3, 38.7], [0.; 4], 8.);
    add_command(
        &mut list,
        0.,
        [20.25, 20.25, 60., 60.],
        [0.2, 0.7, 0.5, 0.35],
        15.,
    );
    add_command(
        &mut list,
        0.,
        [31.75, 30.9, 18.5, 8.25],
        [0.9, 0.8, 0.1, 0.8],
        2.5,
    );
    add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
    add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
    add_command(&mut list, 2., [71.5, 43.25, 14.5, 20.75], [0.; 4], 6.);
    add_command(
        &mut list,
        0.,
        [63.75, 32.1, 40., 50.],
        [0.3, 0.8, 0.9, 0.65],
        7.,
    );
    add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
    let (width, height) = (192, 160);
    for scale in [1., 1.25, 2.] {
        let before = legacy_tiles(&list, width, height, scale);
        let after = list.tiles(width, height, scale);
        assert!(after.len() < before.len());
        for y in 0..height {
            for x in 0..width {
                let expected =
                    sample_rectangles(&list, tile_commands(&before, width, x, y), x, y, scale);
                let actual =
                    sample_rectangles(&list, tile_commands(&after, width, x, y), x, y, scale);
                for k in 0..4 {
                    assert!(
                        (actual[k] - expected[k]).abs() <= 1e-6,
                        "x={x} y={y} scale={scale} channel={k}: {actual:?} != {expected:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn logically_disjoint_clips_keep_overlapping_antialias_fringe() {
    let mut list = empty_list();
    add_command(&mut list, 2., [5.2, 4.3, 30.6, 30.7], [0.; 4], 0.);
    add_command(&mut list, 2., [35.95, 5., 1., 20.], [0.; 4], 0.);
    add_command(&mut list, 0., [0., 0., 60., 40.], [1., 0.5, 0.25, 0.75], 0.);
    add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
    add_command(&mut list, 3., [0.; 4], [0.; 4], 0.);
    let before = legacy_tiles(&list, 96, 64, 1.);
    let after = list.tiles(96, 64, 1.);
    let expected = sample_rectangles(&list, tile_commands(&before, 96, 35, 10), 35, 10, 1.);
    assert!(
        expected[3] > 0.,
        "fixture must exercise nonzero overlapping fringe"
    );
    assert_eq!(
        sample_rectangles(&list, tile_commands(&after, 96, 35, 10), 35, 10, 1.),
        expected
    );
}
