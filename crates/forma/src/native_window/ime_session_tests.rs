//! Composition ownership across focus changes, scene rebuilds and cancellation.

use super::ImeSession;
use winit::event::Ime;

// The cases below predate `ImeSession` and keep passing its three fields separately.
fn with_session(
    composing: &mut bool,
    owner: &mut Option<(u64, i32)>,
    cancelled: &mut bool,
    apply: impl FnOnce(&mut ImeSession),
) {
    let mut session = ImeSession {
        composing: *composing,
        cancelled: *cancelled,
        owner: *owner,
    };
    apply(&mut session);
    (*composing, *owner, *cancelled) = (session.composing, session.owner, session.cancelled);
}

fn standalone_ime_event(
    runtime: &mut crate::Runtime,
    composing: &mut bool,
    owner: &mut Option<(u64, i32)>,
    cancelled: &mut bool,
    event: Ime,
) {
    with_session(composing, owner, cancelled, |session| {
        session.event(runtime, event)
    });
}

fn standalone_observe_ime(
    runtime: &crate::Runtime,
    composing: &mut bool,
    mut owner: Option<(u64, i32)>,
    cancelled: &mut bool,
) {
    with_session(composing, &mut owner, cancelled, |session| {
        session.observe(runtime)
    });
}

fn standalone_cancel_ime(
    runtime: &mut crate::Runtime,
    composing: &mut bool,
    owner: &mut Option<(u64, i32)>,
    cancelled: &mut bool,
) {
    with_session(composing, owner, cancelled, |session| {
        session.cancel(runtime)
    });
}
fn runtime() -> crate::Runtime {
    crate::Runtime::from_sources("component App { Frame { width:240; height:100; Button { key:'first'; } Button { key:'second'; } } }","component Button { Rectangle { ContentInput { value:'seed'; width:200; height:32; } PointerArea { clicked -> events.clicked(); } } }").unwrap()
}
#[test]
fn standalone_ime_rejects_retired_owner_and_allows_fresh_preedit() {
    let mut rt = runtime();
    rt.focus_control(0);
    let (mut composing, mut owner, mut cancelled) = (false, None, false);
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Preedit("old".into(), None),
    );
    rt.focus_control(1);
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Commit("wrong".into()),
    );
    assert_eq!(rt.text_value(0), "seed");
    assert_eq!(rt.text_value(1), "seed");
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Preedit("fresh".into(), None),
    );
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Preedit("".into(), None),
    );
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Commit("fresh".into()),
    );
    assert!(rt.text_value(1).contains("fresh"));
    assert_eq!(rt.text_value(0), "seed");
}
#[test]
fn standalone_ime_binding_sync_retires_rebuilt_session_but_not_idle_session() {
    let files=std::collections::BTreeMap::from([("App.ui".into(),"component App { state a: String = 'first'; state b: String = 'second'; Column { width:240; height:140; TextInput { key:'a'; width:200; height:32; value <-> state.a; } TextInput { key:'b'; width:200; height:32; value <-> state.b; } } }".into())]);
    for svet in [false, true] {
        let mut rt = crate::Runtime::from_ui_files(&files, "App.ui", &Default::default()).unwrap();
        rt.set_svet_document_scheduling(svet);
        rt.sync_ui_bindings_native().unwrap();
        rt.focus_control(0);
        rt.text_key("End", false, false);
        let (mut composing, mut owner, mut cancelled) = (false, None, false);
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("pending".into(), None),
        );
        let original = owner;
        assert!(!rt.sync_ui_bindings_native().unwrap());
        assert_eq!(rt.native_editor_session(), original);
        standalone_observe_ime(&rt, &mut composing, owner, &mut cancelled);
        assert!(composing);
        assert!(!cancelled);
        // A sibling binding causes scene reconstruction while preserving focus/key.
        rt.set_binding_value(1, "updated", false).unwrap();
        assert!(rt.sync_ui_bindings_native().unwrap());
        assert_ne!(rt.native_editor_session(), original);
        assert_eq!(rt.focused_index(), 0);
        standalone_observe_ime(&rt, &mut composing, owner, &mut cancelled);
        assert!(!composing);
        assert!(cancelled);
        for text in ["late", "again"] {
            standalone_ime_event(
                &mut rt,
                &mut composing,
                &mut owner,
                &mut cancelled,
                Ime::Commit(text.into()),
            );
        }
        assert_eq!(rt.text_value(0), "first");
        assert_eq!(rt.text_value(1), "updated");
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Preedit("new".into(), None),
        );
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit("新".into()),
        );
        rt.sync_ui_bindings_native().unwrap();
        assert_eq!(rt.text_value(0), "first新");
        assert_eq!(rt.text_value(1), "updated");
        assert!(rt.ui_state_json().contains("first新"));
        assert!(!rt.ui_state_json().contains("late"));
    }
}
#[test]
fn standalone_navigation_cancellation_survives_empty_preedit_and_late_commits() {
    let mut rt = runtime();
    rt.focus_control(0);
    let (mut composing, mut owner, mut cancelled) = (false, None, false);
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Preedit("pending".into(), None),
    );
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Preedit("".into(), None),
    );
    assert!(!composing);
    assert!(owner.is_some());
    standalone_cancel_ime(&mut rt, &mut composing, &mut owner, &mut cancelled);
    for text in ["late", "again"] {
        standalone_ime_event(
            &mut rt,
            &mut composing,
            &mut owner,
            &mut cancelled,
            Ime::Commit(text.into()),
        );
    }
    assert_eq!(rt.text_value(0), "seed");
    assert_eq!(rt.text_value(1), "seed");
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Preedit("new".into(), None),
    );
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Commit("new".into()),
    );
    assert!(rt.text_value(0).contains("new"));
}
#[test]
fn standalone_ime_rejects_replacement_and_disabled_commit() {
    let mut rt = runtime();
    rt.focus_control(0);
    let (mut composing, mut owner, mut cancelled) = (false, None, false);
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Preedit("old".into(), None),
    );
    rt = runtime();
    rt.focus_control(0);
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Commit("wrong".into()),
    );
    assert_eq!(rt.text_value(0), "seed");
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Disabled,
    );
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Commit("late".into()),
    );
    assert_eq!(rt.text_value(0), "seed");
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Enabled,
    );
    rt.text_key("a", false, true);
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Commit("".into()),
    );
    assert_eq!(rt.text_value(0), "seed");
    assert_eq!(rt.text_selected(), "seed");
    standalone_ime_event(
        &mut rt,
        &mut composing,
        &mut owner,
        &mut cancelled,
        Ime::Commit("direct".into()),
    );
    assert_eq!(rt.text_value(0), "direct");
}
