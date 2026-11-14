//! What the flat control list of the runtime accepts today: how many controls, which nesting of
//! containers, scrolling and clipping. Recorded before the hierarchical control tree of the
//! roadmap replaces it, so a change in any of these is visible.

#[path = "support/golden.rs"]
mod golden;

use forma::Runtime;
use std::collections::BTreeMap;
use std::fmt::Write;

fn load(body: &str) -> String {
    let files = BTreeMap::from([("App.ui".to_owned(), format!("component App {{ {body} }}"))]);
    match Runtime::from_ui_files(&files, "App.ui", &Default::default()) {
        Ok(runtime) => format!(
            "loads, {} control(s), {} node(s)",
            runtime.control_count(),
            runtime.nodes().len()
        ),
        Err(error) => format!("ERROR {error}"),
    }
}

fn texts(count: usize) -> String {
    (0..count)
        .map(|i| format!("Text {{ key: 't{i}'; text: 'x'; width: 20; height: 10; }}"))
        .collect()
}

const CASES: &[(&str, &str)] = &[
    (
        "nested column in column",
        "Column { width: 200; height: 200; Column { Text { text: 'a'; width: 50; height: 20; } } }",
    ),
    (
        "row in column",
        "Column { width: 200; height: 200; Row { Text { text: 'a'; width: 50; height: 20; } Text { text: 'b'; width: 50; height: 20; } } }",
    ),
    (
        "scroll at the root",
        "Column { width: 200; height: 200; Scroll { width: 200; height: 100; Column { width: 100; height: 400; Text { text: 'a'; width: 50; height: 20; } } } }",
    ),
    (
        "scroll beside a control",
        "Column { width: 200; height: 200; Text { text: 'a'; width: 50; height: 20; } Scroll { width: 200; height: 100; Column { width: 100; height: 400; Text { text: 'b'; width: 50; height: 20; } } } }",
    ),
    (
        "scroll inside a nested column",
        "Column { width: 200; height: 200; Column { Scroll { width: 200; height: 100; Column { width: 100; height: 400; Text { text: 'a'; width: 50; height: 20; } } } } }",
    ),
    (
        "scroll inside a scroll",
        "Column { width: 200; height: 200; Scroll { width: 200; height: 100; Column { width: 100; height: 400; Scroll { width: 100; height: 50; Column { width: 50; height: 200; Text { text: 'a'; width: 50; height: 20; } } } } } }",
    ),
    (
        "clip on the root",
        "Column { width: 200; height: 200; clip: true; Text { text: 'a'; width: 50; height: 20; } }",
    ),
    (
        "clip on a nested column",
        "Column { width: 200; height: 200; Column { clip: true; Text { text: 'a'; width: 50; height: 20; } } }",
    ),
    (
        "stack overlap",
        "Stack { width: 200; height: 200; Text { text: 'a'; width: 50; height: 20; } Text { text: 'b'; width: 50; height: 20; } }",
    ),
    (
        "grid with cells",
        "Frame { width: 200; height: 200; columns: [*, *]; rows: [*, *]; Text { cell: 1 1; text: 'a'; width: 50; height: 20; } Text { cell: 2 2; text: 'b'; width: 50; height: 20; } }",
    ),
    (
        "child wider than its column",
        "Column { width: 200; height: 200; Text { text: 'a'; width: 260; height: 20; } }",
    ),
];

#[test]
fn the_flat_scene_accepts_what_is_recorded() {
    let mut out = String::new();
    for count in [255, 256, 257, 1024, 1025] {
        let body = format!("Column {{ width: 400; height: 400; {} }}", texts(count));
        writeln!(out, "{count} texts: {}", load(&body)).unwrap();
    }
    for (title, body) in CASES {
        writeln!(out, "{title}: {}", load(body)).unwrap();
    }
    golden::assert_golden("flat_scene_limits.txt", &out);
}
