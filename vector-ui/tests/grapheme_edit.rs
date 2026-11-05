use forma::control_state::TextEdit;

#[test]
fn grapheme_navigation_deletion_selection_and_history_are_atomic() {
    for cluster in ["e\u{301}","👍🏽","👨‍👩‍👧‍👦","🇬🇪","1\u{fe0f}\u{20e3}","각","क्‍ष"] {
        let value=format!("L{cluster}R");let start=1;let end=1+cluster.len();
        let mut edit=TextEdit::new(value.clone()).unwrap();
        edit.set_selection(start,start).unwrap();
        edit.move_graphemes(1,true);assert_eq!(edit.selection(),start..end,"{cluster}");
        assert_eq!(edit.selected_text(),cluster);
        edit.backspace_grapheme();assert_eq!(edit.value(),"LR");
        edit.undo();assert_eq!(edit.value(),value);assert_eq!(edit.selected_text(),cluster);
        edit.redo();assert_eq!(edit.value(),"LR");
        edit.undo();edit.set_selection(end,end).unwrap();
        edit.backspace_grapheme();assert_eq!(edit.value(),"LR");
        edit.undo();edit.set_selection(start,start).unwrap();
        edit.delete_grapheme();assert_eq!(edit.value(),"LR");
        edit.undo();edit.end(false);
        edit.move_graphemes(-2,false);assert_eq!(edit.caret(),start);
        edit.move_graphemes(1,false);assert_eq!(edit.caret(),end);
    }
}

#[test]
fn partial_scalar_selection_expands_but_scalar_api_remains_available() {
    let mut scalar=TextEdit::new("e\u{301}".into()).unwrap();
    scalar.backspace();assert_eq!(scalar.value(),"e");
    for forward in [false,true] {
        let mut edit=TextEdit::new("e\u{301}R".into()).unwrap();
        edit.set_selection(1,1).unwrap();
        if forward {edit.delete_grapheme();} else {edit.backspace_grapheme();}
        assert_eq!(edit.value(),"R");
    }
    let mut edit=TextEdit::new("e\u{301}R".into()).unwrap();
    edit.set_selection(1,3).unwrap();
    assert_eq!(edit.grapheme_selection(),0..3);
    edit.insert_graphemes("X").unwrap();assert_eq!(edit.value(),"XR");
}

#[test]
fn insertion_that_joins_suffix_and_crlf_keep_valid_caret_positions() {
    let mut edit=TextEdit::new("\u{301}R".into()).unwrap();
    edit.home(false);edit.insert_graphemes("e").unwrap();
    assert_eq!(edit.value(),"e\u{301}R");assert_eq!(edit.caret(),3);
    edit.undo();assert_eq!(edit.value(),"\u{301}R");assert_eq!(edit.caret(),0);
    edit.redo();assert_eq!(edit.caret(),3);
    let mut multiline=TextEdit::with_multiline("L\r\nR".into(),true).unwrap();
    multiline.set_selection(1,1).unwrap();multiline.delete_grapheme();assert_eq!(multiline.value(),"LR");
    let mut empty=TextEdit::new(String::new()).unwrap();
    assert!(!empty.move_graphemes(isize::MIN,false));
    assert!(!empty.move_graphemes(isize::MAX,true));
    assert!(!empty.backspace_grapheme());assert!(!empty.delete_grapheme());
}

#[test]
fn live_binding_replacement_does_not_leave_caret_inside_a_cluster() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state query: String = 'a'; Column { width:240; height:100; TextInput { width:200; height:32; value <-> state.query; } } }".into())]);
    let mut runtime=forma::Runtime::from_ui_files(&files,"App.ui",&Default::default()).unwrap();
    assert!(runtime.focus_control(0));
    runtime.set_binding_value(0,"e\u{301}R",false).unwrap();
    assert!(runtime.text_key("ArrowLeft",true,false));
    assert_eq!(runtime.text_selected(),"e\u{301}");
    runtime.text_key("Backspace",false,false);
    assert_eq!(runtime.text_value(0),"R");
}
