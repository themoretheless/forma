use super::*;
fn editor() -> Runtime {
    Runtime::from_ui_files(&std::collections::BTreeMap::from([("App.ui".into(),
        "component App { state query: String = 'Привет'; state other: String = 'untouched'; Column { width:260; height:120; TextInput { key:'editor'; width:200; height:32; value <-> state.query; } TextInput { key:'other'; width:200; height:32; value <-> state.other; } } }".into())]), "App.ui", &Default::default()).unwrap()
}
#[test]
fn non_latin_commands_use_physical_fallback_without_remapping_latin_layouts() {
    use winit::keyboard::{KeyCode, PhysicalKey};
    for (code, logical, expected) in [
        (KeyCode::KeyA, "ф", "a"),
        (KeyCode::KeyC, "с", "c"),
        (KeyCode::KeyX, "ч", "x"),
        (KeyCode::KeyV, "м", "v"),
        (KeyCode::KeyZ, "я", "z"),
        (KeyCode::KeyY, "н", "y"),
    ] {
        assert_eq!(command_key(&PhysicalKey::Code(code), logical), expected);
    }
    assert_eq!(
        command_key(&PhysicalKey::Code(KeyCode::KeyA), "q"),
        "q",
        "Latin AZERTY-like logical key wins"
    );
    assert_eq!(command_key(&PhysicalKey::Code(KeyCode::KeyQ), "a"), "a");
    assert_eq!(
        command_key(&PhysicalKey::Code(KeyCode::KeyQ), "й"),
        "й",
        "unknown shortcuts stay unknown"
    );
}

#[test]
fn copy_and_failed_cut_preserve_selection_and_model() {
    let mut runtime = editor();
    runtime.focus_control(0);
    runtime.text_key("a", false, true);
    let mut writes = Vec::new();
    assert!(handle_with(
        &mut runtime,
        "C",
        || panic!("copy must not read"),
        |text| {
            writes.push(text.to_owned());
            Ok(())
        }
    ));
    assert_eq!(writes, vec!["Привет"]);
    assert_eq!(runtime.text_selected(), "Привет");
    assert!(handle_with(
        &mut runtime,
        "x",
        || panic!("cut must not read"),
        |_| Err(std::io::Error::other("refused"))
    ));
    assert_eq!(runtime.text_value(0), "Привет");
    assert_eq!(runtime.text_selected(), "Привет");
    assert!(handle_with(
        &mut runtime,
        "v",
        || Err(std::io::Error::other("refused")),
        |_| panic!("paste must not write")
    ));
    assert_eq!(runtime.text_value(0), "Привет");
    assert_eq!(runtime.text_selected(), "Привет");
    runtime.sync_ui_bindings_native().unwrap();
    assert_eq!(runtime.text_value(1), "untouched");
}
#[cfg(unix)]
#[test]
fn clipboard_writer_reaps_children_on_success_failure_and_missing_pipe() {
    for (script, piped, large, success) in [
        ("cat >/dev/null", true, false, true),
        ("exit 7", true, true, false),
        ("exit 0", false, false, false),
    ] {
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", script]);
        if piped {
            command.stdin(std::process::Stdio::piped());
        }
        let mut child = command.spawn().unwrap();
        let pid = child.id() as libc::pid_t;
        let text = if large {
            "x".repeat(1_000_000)
        } else {
            "你好🙂".into()
        };
        assert_eq!(write_clipboard_child(&mut child, &text).is_ok(), success);
        assert!(child.stdin.is_none());
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
            -1,
            "child must already be reaped"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}
#[test]
fn malformed_clipboard_is_rejected_without_replacing_selection() {
    assert_eq!(
        decode_clipboard("你好🙂".as_bytes().to_vec()).unwrap(),
        "你好🙂"
    );
    for bytes in [vec![0xff], vec![0xe2, 0x82], vec![b'a', 0xc0, 0xaf]] {
        let mut runtime = editor();
        runtime.focus_control(0);
        runtime.text_key("a", false, true);
        let selection = runtime.text_selection_utf16();
        assert!(handle_with(
            &mut runtime,
            "v",
            || decode_clipboard(bytes.clone()),
            |_| panic!()
        ));
        assert_eq!(runtime.text_value(0), "Привет");
        assert_eq!(runtime.text_selection_utf16(), selection);
        assert_eq!(
            decode_clipboard(bytes).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
#[test]
fn empty_paste_preserves_selection_bindings_and_undo_history() {
    let mut runtime = editor();
    runtime.focus_control(0);
    runtime.text_insert("!");
    runtime.text_key("a", false, true);
    let original = runtime.text_value(0);
    let selection = runtime.text_selection_utf16();
    assert!(handle_with(
        &mut runtime,
        "v",
        || Ok(String::new()),
        |_| panic!("paste must not write")
    ));
    assert_eq!(runtime.text_value(0), original);
    assert_eq!(runtime.text_selection_utf16(), selection);
    runtime.sync_ui_bindings_native().unwrap();
    let state: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert_eq!(state["query"], original);
    assert_eq!(state["other"], "untouched");
    runtime.text_key("z", false, true);
    assert_eq!(runtime.text_value(0), "Привет");
    runtime.text_key("z", true, true);
    assert_eq!(runtime.text_value(0), original);
}
#[test]
fn cut_is_undoable_and_unicode_paste_commits_only_selected_editor() {
    let mut runtime = editor();
    runtime.focus_control(0);
    runtime.text_key("a", false, true);
    let mut copied = String::new();
    assert!(handle_with(
        &mut runtime,
        "x",
        || panic!(),
        |text| {
            copied = text.to_owned();
            Ok(())
        }
    ));
    assert_eq!(copied, "Привет");
    assert_eq!(runtime.text_value(0), "");
    runtime.text_key("z", false, true);
    assert_eq!(runtime.text_value(0), "Привет");
    runtime.text_key("z", true, true);
    assert_eq!(runtime.text_value(0), "");
    assert!(handle_with(
        &mut runtime,
        "V",
        || Ok("你好\r\nмир🙂".into()),
        |_| panic!()
    ));
    assert_eq!(runtime.text_value(0), "你好 мир🙂");
    runtime.sync_ui_bindings_native().unwrap();
    let model: serde_json::Value = serde_json::from_str(&runtime.ui_state_json()).unwrap();
    assert_eq!(model["query"], "你好 мир🙂");
    assert_eq!(model["other"], "untouched");
}
#[test]
fn unavailable_editor_and_empty_copy_do_not_access_clipboard() {
    let mut runtime = editor();
    assert!(!handle_with(&mut runtime, "v", || panic!(), |_| panic!()));
    runtime.focus_control(0);
    assert!(handle_with(
        &mut runtime,
        "c",
        || panic!(),
        |_| panic!("empty selection must not clear clipboard")
    ));
    assert!(!handle_with(&mut runtime, "a", || panic!(), |_| panic!()));
    runtime.set_binding_disabled(0, true).unwrap();
    assert!(!handle_with(&mut runtime, "v", || panic!(), |_| panic!()));
}

#[cfg(unix)]
#[test]
fn clipboard_reader_handles_utf8_failures_and_timeout_without_touching_os_clipboard() {
    let ok_child = std::process::Command::new("/bin/sh")
        .args(["-c", "printf 'Привет🙂\\nمرحبا'"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let bytes = read_clipboard_child(ok_child).unwrap();
    assert_eq!(decode_clipboard(bytes).unwrap(), "Привет🙂\nمرحبا");

    let fail_child = std::process::Command::new("/bin/sh")
        .args(["-c", "printf 'partial'; exit 3"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    assert!(read_clipboard_child(fail_child).is_err());

    let no_pipe = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    assert!(read_clipboard_child(no_pipe).is_err());

    let slow = std::process::Command::new("/bin/sleep")
        .arg("2")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = slow.id() as libc::pid_t;
    let err = read_clipboard_child(slow).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
}
