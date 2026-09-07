use forma::Runtime;
#[test]
fn empty_frame_renders_and_has_no_phantom_control() {
    let mut runtime = Runtime::from_sources("component Empty { Frame { width: 100; height: 80; background: #123456; } }", "FORMA-TEMPLATES-1\n").unwrap();
    assert_eq!(runtime.control_count(), 0);
    assert_eq!(runtime.background_color(), "#123456ff");
    assert_eq!(runtime.key(), "");
    assert_eq!(runtime.label(), "");
    assert_eq!(runtime.action(), "");
    assert_eq!(runtime.bounds(), vec![0., 0., 100., 80.]);
    assert_eq!(runtime.hit_index(20.,20.), -1);
    assert_eq!(runtime.pixels(100,80,1.).len(), 100*80*4);
    runtime.focus(false);
}
