use forma_build::{GenerateOptions, ProjectBuildSpec, generate_form, generate_project};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn binding_app_files() -> BTreeMap<String, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = manifest_dir.join("../forma/examples/binding-app");
    let mut files = BTreeMap::new();
    files.insert(
        "ui/CustomerForm.ui".into(),
        fs::read_to_string(project.join("ui/CustomerForm.ui")).unwrap(),
    );
    for entry in fs::read_dir(project.join("components")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        files.insert(
            format!("components/{name}"),
            fs::read_to_string(entry.path()).unwrap(),
        );
    }
    files
}

#[test]
fn rejects_misspelled_or_invalid_declarations_with_utf16_line_numbers() {
    let files = binding_app_files();
    let entry = "ui/CustomerForm.ui";
    let base = &files[entry];
    let cases = [
        (
            "contextType: 'crate::CustomerVm'",
            "contextType: 'crate::CustomerVm; bad'",
            "ui/CustomerForm.ui:2:",
            "contextType",
        ),
        (
            "events.saveRequested()",
            "events.saveRequsted()",
            "ui/CustomerForm.ui:26:",
            "не объявлено",
        ),
        (
            "value <-> state.name",
            "value <-> state.na-me",
            "ui/CustomerForm.ui:",
            "Строка 14:",
        ),
        (
            "value <-> state.name",
            "widht: state.name",
            "ui/CustomerForm.ui:14:",
            "Неизвестное свойство",
        ),
        (
            "key: 'nameInput'",
            "key: 'runtime'",
            "Конфликт Rust-метода runtime",
            "runtime",
        ),
        (
            "disabled: state.busy",
            "disabled <-> state.busy",
            "двусторонняя привязка disabled",
            "disabled",
        ),
    ];
    for (from, to, expected_prefix, expected_detail) in cases {
        let mut next = files.clone();
        next.insert(entry.into(), base.replacen(from, to, 1));
        let err = generate_form(&next, entry, &GenerateOptions::default()).unwrap_err();
        assert!(
            err.contains(expected_prefix) && err.contains(expected_detail),
            "unexpected error for ({from} -> {to}): {err}"
        );
    }
}

#[test]
fn generates_row_grid_submitted_native_app_and_override_forms() {
    let mut files = binding_app_files();
    files.insert(
        "ui/Horizontal.ui".into(),
        "component Horizontal {contextType:'crate::CustomerVm';Row {width:500;height:100;gap:8;Button {text:'First';} Button {text:'Second';}}}".into(),
    );
    let plain = generate_form(&files, "ui/Horizontal.ui", &GenerateOptions::default()).unwrap();
    assert!(plain.rust.contains("root:N {kind:r#\"Row\"#"));
    assert!(!plain.rust.contains("native_window::NativeApp"));

    let native = generate_form(
        &files,
        "ui/Horizontal.ui",
        &GenerateOptions {
            native_app: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        native
            .rust
            .contains("native_window::NativeApp for Horizontal")
    );
    assert!(!plain.source_map.is_empty() || plain.form_name == "Horizontal");

    files.insert(
        "ui/Submit.ui".into(),
        "component Submit {contextType:'crate::Vm';event sent(query:String);Frame {width:300;height:100;TextInput {key:'query';value <-> state.query;submitted -> events.sent(state.query);} TextInput {key:'direct';submitted -> state.send();}}}".into(),
    );
    let submit = generate_form(&files, "ui/Submit.ui", &GenerateOptions::default()).unwrap();
    assert!(submit.rust.contains("declare_event(1, r#\"submitted\"#"));
    assert!(
        submit
            .rust
            .contains(".on_submitted(move |event| handler(SubmitSentEvent")
    );
    assert!(submit.rust.contains("vm.send();"));
}

#[test]
fn preserves_current_color_and_literal_hex_svg_colors_in_single_pass() {
    let mut files = binding_app_files();
    files.insert(
        "assets/probe.svg".into(),
        r##"<svg viewBox="0 0 24 24"><rect x="1" y="1" width="10" height="10" fill="currentColor"/><rect x="12" y="1" width="10" height="10" fill="#010203"/><line x1="2" y1="20" x2="20" y2="20" stroke="currentColor" stroke-width="2"/></svg>"##.into(),
    );
    files.insert(
        "components/IconBox.ui".into(),
        "component IconBox { width: 40; height: 40; Image { source: 'assets/probe.svg'; } }".into(),
    );
    files.insert(
        "ui/IconForm.ui".into(),
        "component IconForm { contextType: 'crate::Vm'; Frame { width: 100; height: 100; IconBox {} } }".into(),
    );
    let generated = generate_form(&files, "ui/IconForm.ui", &GenerateOptions::default()).unwrap();
    assert!(generated.rust.contains("r#\"currentColor\"#.into()"));
    assert!(generated.rust.contains("r#\"#010203\"#.into()"));
    assert!(generated.rust.contains("r#\"IconBox\"#.into(),D {"));
    assert!(
        generated
            .rust
            .contains("r#\"assets/probe.svg\"#.into(), ::forma::form_document::Image {")
    );
}

#[test]
fn generate_project_writes_source_and_source_map_for_language_example() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = manifest_dir.join("../forma/examples/language-app");
    let out_dir = std::env::temp_dir().join(format!("forma-build-test-{}", std::process::id()));
    let out_file = out_dir.join("task_list.rs");
    let res = generate_project(&ProjectBuildSpec {
        project: &project,
        output: &out_file,
        entry: Some("ui/TaskList.ui"),
        state_file: None,
        native_app: false,
    })
    .unwrap();
    assert_eq!(res.form_name, "TaskList");
    assert!(res.rust.contains("pub struct TaskList"));
    assert!(out_file.exists());
    let map_file = out_dir.join("task_list.rs.map.json");
    assert!(map_file.exists());
    let map_raw = fs::read_to_string(&map_file).unwrap();
    assert!(map_raw.contains("ui/TaskList.ui"));
    fs::remove_dir_all(out_dir).ok();
}
