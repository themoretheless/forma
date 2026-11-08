use forma::{Button, Runtime};

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

const EDITORS: &str = "component Demo { Frame { width:240; height:120; gap:8; Button { key:'first'; width:200; height:32; } Button { key:'second'; width:200; height:32; } } }";
const EDITOR_TEMPLATE: &str = "component Button { Rectangle { ContentInput { width:180; height:30; value:'seed'; } PointerArea { clicked -> events.clicked(); } } }";

#[test]
fn singleline_enter_submits_the_active_editor_once_without_clicking() {
    let mut runtime=Runtime::from_sources(EDITORS,EDITOR_TEMPLATE).unwrap();
    assert!(runtime.focus_control(0));
    runtime.text_key("a",false,true);
    runtime.text_insert("ready");
    assert!(runtime.text_key("Enter",false,false));
    assert_eq!(runtime.text_value(0),"ready");
    assert_eq!(runtime.control_submissions(0),1);
    assert_eq!(runtime.control_submissions(1),0);
    assert_eq!(runtime.submissions(),1);
    assert_eq!(runtime.submitted_index(),0);
    assert_eq!(runtime.clicks(),0);
}

#[test]
fn textarea_enter_inserts_newline_without_submitted_or_clicked() {
    let files=std::collections::BTreeMap::from([
        ("App.ui".into(),"component App { Frame { width:240; height:100; TextArea { key:'notes'; width:200; height:80; value:'seed'; } } }".into()),
        ("components/TextArea.ui".into(),"component TextArea { value:''; Rectangle { TextInput { width:200; height:80; value:props.value; multiline:true; } PointerArea { clicked -> events.clicked(); } } }".into()),
    ]);
    let mut runtime=Runtime::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
    assert!(runtime.focus_control(0));
    assert!(runtime.text_key("Enter",false,false));
    assert_eq!(runtime.text_value(0),"seed\n");
    assert_eq!(runtime.submissions(),0);
    assert_eq!(runtime.submitted_index(),-1);
    assert_eq!(runtime.clicks(),0);
}

#[test]
fn submissions_follow_unique_keys_without_stale_retrigger_after_reload() {
    let mut previous=Runtime::from_sources(EDITORS,EDITOR_TEMPLATE).unwrap();
    previous.focus_control(0);previous.text_key("Enter",false,false);
    previous.focus_control(1);previous.text_key("Enter",false,false);previous.text_key("Enter",false,false);
    let swapped=EDITORS.replace("key:'first'","key:'temporary'").replace("key:'second'","key:'first'").replace("key:'temporary'","key:'second'");
    let mut next=Runtime::from_sources(&swapped,EDITOR_TEMPLATE).unwrap();
    next.preserve_interaction(&previous);
    assert_eq!(next.control_submissions(0),2);
    assert_eq!(next.control_submissions(1),1);
    assert_eq!(next.submissions(),3);
    assert_eq!(next.submitted_index(),-1);
    next.load_component_native(&swapped,EDITOR_TEMPLATE).unwrap();
    assert_eq!(next.submissions(),0);
    assert_eq!(next.submitted_index(),-1);
    assert_eq!(next.clicks(),0);
}
