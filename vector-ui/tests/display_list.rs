use forma_vector::{display_list::STRIDE, Button};
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
    assert!(last
        .iter()
        .all(|&id| matches!(commands[id as usize * STRIDE] as u32, 2 | 3)));
}
