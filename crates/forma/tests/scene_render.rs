//! Pins everything `Document::render_scene_artifact` produces: scene source, templates, control
//! records and the logical content extent, for each shape of scene the renderer distinguishes.
//!
//! The renderer is one long function with a positioned and an unpositioned path, a root `Scroll`
//! special case and per-control template emission. It is covered here as a whole before being
//! taken apart, so a refactoring that changes one byte of output is caught.

#[path = "support/golden.rs"]
mod golden;

use forma::form_document::{Document, Properties, Value};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

const TOGGLE: &str = "component Toggle { prop checked: Bool = false; prop label: String = ''; \
    width: 140; height: 32; Rectangle { Row { padding: 4; Text { text: props.checked ? \
    '[x] ${props.label}' : '[ ] ${props.label}'; } } PointerArea { clicked -> events.clicked(); } } }";

const BUTTON: &str = "component Button { width: 120; height: 32; text: ''; disabled: false; \
    Rectangle { radius: 6; background: Brush { color: #425b8a; hover: #536fa3; pressed: #34486f; \
    disabled: #303846; }; ContentPresenter { key: 'content'; Text { text: props.text; color: \
    #ffffff; fontSize: 14; } } PointerArea { clicked -> events.clicked(); } } }";

/// Scenes by the branch of the renderer they reach.
const SCENES: &[(&str, &str)] = &[
    (
        "plain frame, one control",
        "component App { Frame { width: 200; height: 100; padding: 10; Button { key: 'ok'; \
         width: 80; height: 30; text: 'OK'; clicked -> events.ok(); } } }",
    ),
    (
        "plain frame, absolute controls",
        "component App { Frame { width: 300; height: 200; Button { key: 'a'; x: 10; y: 20; \
         width: 80; height: 30; text: 'A'; } Text { key: 'b'; x: 120; y: 60; text: 'label'; } } }",
    ),
    (
        "column with gap and padding",
        "component App { state query: String = 'forma'; state agreed: Bool = false; Column { \
         width: 260; height: 140; padding: 10; gap: 8; TextInput { key: 'search'; width: 200; \
         height: 32; value <-> state.query; } Toggle { key: 'agree'; label: 'Terms'; checked <-> \
         state.agreed; } Text { key: 'summary'; text: '${state.query}:${state.agreed}'; } } }",
    ),
    (
        "nested rows and columns",
        "component App { Column { width: 320; height: 200; padding: [8, 12, 8, 12]; gap: 6; Row { \
         gap: 4; Button { key: 'a'; width: 60; height: 24; text: 'A'; } Button { key: 'b'; width: \
         60; height: 24; text: 'B'; } } Row { gap: 4; Text { key: 't'; text: 'tail'; } Toggle { \
         key: 'x'; label: 'X'; } } } }",
    ),
    (
        "grid with tracks and cells",
        "component App { Frame { width: 300; height: 120; columns: [80, *, -]; rows: [30, *]; gap: \
         4; Button { key: 'a'; cell: 1 1; text: 'A'; } Button { key: 'b'; cell: 1 2; text: 'B'; } \
         Text { key: 'c'; cell: 2 3; text: 'C'; } } }",
    ),
    (
        "stack",
        "component App { Stack { width: 120; height: 80; Button { key: 'under'; width: 120; \
         height: 80; text: 'under'; } Text { key: 'over'; text: 'over'; } } }",
    ),
    (
        "root scroll",
        "component App { Column { width: 360; height: 400; padding: 20; Scroll { width: 320; \
         height: 300; Column { width: 100; gap: 4; Button { key: 'a'; width: 100; height: 200; \
         text: 'A'; } Button { key: 'b'; width: 100; height: 200; text: 'B'; } } } } }",
    ),
    (
        "root scroll without content",
        "component App { Column { width: 360; height: 400; Scroll { width: 320; height: 300; \
         Column { width: 100; height: 512; } } } }",
    ),
    (
        "slider",
        "component App { state level: Number = 40; Column { width: 240; height: 60; Slider { key: \
         'level'; width: 200; height: 24; value <-> state.level; } } }",
    ),
    (
        "conditional and repeated controls",
        "component App { state show: Bool = true; state items: List = [{ id: 'a', title: 'First' \
         }, { id: 'b', title: 'Second' }]; Column { width: 200; height: 200; gap: 2; if state.show \
         { Text { key: 'head'; text: 'shown'; } } else { Text { key: 'tail'; text: 'hidden'; } } \
         for item in state.items key item.id { Button { width: 120; height: 24; text: item.title; \
         clicked -> events.pick(item.id); } } } }",
    ),
    (
        "empty layout",
        "component App { Column { width: 100; height: 80; } }",
    ),
    (
        "two roots",
        "component App { Column { width: 10; height: 10; } Column { width: 10; height: 10; } }",
    ),
    (
        "control as root",
        "component App { Button { width: 80; height: 30; text: 'OK'; } }",
    ),
    (
        "root scroll with an unsupported property",
        "component App { Column { width: 360; height: 400; Scroll { width: 320; height: 300; \
         background: #ff0000; Column { width: 100; height: 512; } } } }",
    ),
    (
        "root scroll larger than the scene",
        "component App { Column { width: 100; height: 100; Scroll { width: 320; height: 300; \
         Column { width: 100; height: 512; } } } }",
    ),
];

fn record(out: &mut String, title: &str, document: Result<Document, String>) {
    writeln!(out, "=== {title}").unwrap();
    match document.and_then(|document| document.render_scene_artifact()) {
        Ok(artifact) => {
            writeln!(out, "source: {}", artifact.source).unwrap();
            writeln!(out, "templates: {}", artifact.templates).unwrap();
            for control in &artifact.controls {
                writeln!(
                    out,
                    "control: node {} key {:?} state {:?} events {:?}",
                    control.node, control.key, control.state, control.event_args
                )
                .unwrap();
            }
            writeln!(
                out,
                "logical content end: {:?}",
                artifact.logical_content_end
            )
            .unwrap();
        }
        Err(error) => writeln!(out, "ERROR {error}").unwrap(),
    }
    out.push('\n');
}

/// Every `.ui` file under `directory`, keyed by its path relative to it.
fn project(directory: &Path) -> BTreeMap<String, String> {
    fn collect(root: &Path, directory: &Path, files: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(root, &path, files);
            } else if path.extension().is_some_and(|extension| extension == "ui") {
                let key = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(key, std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    collect(directory, directory, &mut files);
    files
}

#[test]
fn rendered_scenes_match_the_record() {
    let mut out = String::new();
    for (title, source) in SCENES {
        let files = BTreeMap::from([
            ("App.ui".to_string(), source.to_string()),
            ("Toggle.component.ui".to_string(), TOGGLE.to_string()),
            ("Button.component.ui".to_string(), BUTTON.to_string()),
        ]);
        record(
            &mut out,
            title,
            Document::from_ui_files(&files, "App.ui", &Properties::new()),
        );
    }

    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let task = |id: &str, title: &str| {
        Value::Object(Properties::from([
            ("id".into(), Value::Text(id.into())),
            ("title".into(), Value::Text(title.into())),
        ]))
    };
    for (title, tasks, show_info) in [
        (
            "example language-app, two tasks",
            vec![task("a", "Write"), task("b", "Review")],
            true,
        ),
        ("example language-app, no tasks", Vec::new(), false),
    ] {
        let state = Properties::from([
            ("tasks".into(), Value::List(tasks)),
            ("show_info".into(), Value::Bool(show_info)),
            ("status".into(), Value::Text("2 open".into())),
        ]);
        let files = project(&examples.join("language-app"));
        record(
            &mut out,
            title,
            Document::from_ui_files(&files, "ui/TaskList.ui", &state),
        );
    }
    golden::assert_golden("scene_render.txt", &out);
}
