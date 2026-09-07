use forma::Button;

#[test]
fn disabled_border_uses_its_state_before_the_first_event() {
    let mut b=Button::from_sources(
        "component Demo { Frame { width:100; height:60; Button { width:80; height:40; disabled:true; } } }",
        "component Button { Rectangle { Border { width:1; background:Brush { color:#ff0000; disabled:#00ff00; transition:0ms; }; } } }",
    ).unwrap();
    let before=b.gpu_params(100,60,1.,true);
    assert_eq!(&before[8..12],&[0.,1.,0.,1.]);
    b.focus(false);
    assert_eq!(before,b.gpu_params(100,60,1.,true));
}

#[test]
fn keyboard_activation_is_a_focused_matching_release_without_repeat() {
    let mut b=Button::new();
    b.key_event(1,true,false);b.key_event(1,false,false);assert_eq!(b.clicks(),0);
    b.focus(true);
    b.key_event(1,true,false);b.key_event(1,true,true);assert_eq!(b.clicks(),0);
    b.key_event(2,false,false);assert_eq!(b.clicks(),0);
    b.key_event(1,false,false);assert_eq!(b.clicks(),1);
    b.key_event(1,false,false);assert_eq!(b.clicks(),1);
    b.key_event(2,true,false);b.focus(false);b.focus(true);b.key_event(2,false,false);assert_eq!(b.clicks(),1);
    b.key_event(2,true,false);b.tick(1000.);assert!(b.visual_revision()>0);
    b.key_event(2,false,false);assert_eq!(b.clicks(),2);
}
