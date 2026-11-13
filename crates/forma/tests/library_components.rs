//! Which components of the standard library the Rust runtime loads, alone and in the states
//! their own properties offer.
//!
//! The library in `controls/components` is authored against the Studio compiler; nothing checked
//! it against the runtime, so a component the runtime cannot expand went unnoticed until an
//! application used it. Every component is loaded once with its defaults and once with each of
//! its boolean properties turned on, and the outcome is recorded.

#[path = "support/golden.rs"]
mod golden;

use forma::Runtime;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

/// The light theme of `controls/tokens.js`, substituted the way `materializeTheme` does it there:
/// the library names theme values instead of carrying them. Longer names come first so that a
/// name is not cut short by its prefix.
const THEME: &[(&str, &str)] = &[
    ("theme.surfaceRaised", "#FFFFFF"),
    ("theme.surfaceHover", "#EEE9DF"),
    ("theme.surfacePressed", "#EBE5DC"),
    ("theme.surface", "#FBF9F4"),
    ("theme.canvas", "#F4F1EA"),
    ("theme.backdrop", "#2B2A2666"),
    ("theme.borderStrong", "#C8BFB0"),
    ("theme.border", "#DED7CA"),
    ("theme.textMuted", "#6B665C"),
    ("theme.textDisabled", "#A39C90"),
    ("theme.text", "#2B2A26"),
    ("theme.focus", "#A6470C"),
    ("theme.accentWash", "#F5E5D5"),
    ("theme.accentInk", "#93420E"),
    ("theme.accent", "#DE6A19"),
    ("theme.reveal", "#DE6A19F2"),
    ("theme.primaryHover", "#A6470C"),
    ("theme.primaryPressed", "#913F0B"),
    ("theme.primaryText", "#FFFFFF"),
    ("theme.primary", "#B9510F"),
    ("theme.dangerHover", "#AA2E20"),
    ("theme.dangerPressed", "#92261B"),
    ("theme.dangerText", "#FFFFFF"),
    ("theme.dangerInk", "#A42E22"),
    ("theme.dangerWash", "#F9E4DF"),
    ("theme.danger", "#C23625"),
    ("theme.successWash", "#E7F0E8"),
    ("theme.success", "#306448"),
    ("theme.warningWash", "#F5E8CE"),
    ("theme.warning", "#825219"),
    ("theme.track", "#E3DCCE"),
    ("motion.control", "180ms"),
    ("motion.reveal", "280ms"),
];

fn library() -> BTreeMap<String, String> {
    let controls = Path::new(env!("CARGO_MANIFEST_DIR")).join("controls");
    let mut files = BTreeMap::new();
    for (directory, extension) in [("components", "ui"), ("assets", "svg")] {
        for entry in std::fs::read_dir(controls.join(directory)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|found| found == extension) {
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                let source = THEME.iter().fold(
                    std::fs::read_to_string(&path).unwrap(),
                    |text, (name, value)| text.replace(&format!("state.{name}"), value),
                );
                files.insert(format!("{directory}/{name}"), source);
            }
        }
    }
    files
}

/// Names of the boolean properties a component itself declares with a literal default; the
/// properties of the nodes inside it are not its own.
fn boolean_properties(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let (mut depth, mut statement) = (0usize, String::new());
    for character in source.chars() {
        match character {
            '{' => {
                depth += 1;
                statement.clear();
            }
            '}' => {
                depth = depth.saturating_sub(1);
                statement.clear();
            }
            ';' | '\n' if depth == 1 => {
                if let Some((name, value)) = statement.split_once(':') {
                    let (name, value) = (name.trim(), value.trim());
                    if (value == "false" || value == "true")
                        && !name.is_empty()
                        && name.chars().all(|c| c.is_ascii_alphanumeric())
                        && !names.iter().any(|known| known == name)
                    {
                        names.push(name.to_owned());
                    }
                }
                statement.clear();
            }
            _ if depth == 1 => statement.push(character),
            _ => {}
        }
    }
    names
}

/// The boolean properties of a component and of every component it derives from.
fn inherited_boolean_properties(library: &BTreeMap<String, String>, source: &str) -> Vec<String> {
    let mut names = boolean_properties(source);
    let mut current = source.to_owned();
    for _ in 0..16 {
        let header = current.split('{').next().unwrap_or("");
        let Some(base) = header.split(':').nth(1).map(str::trim) else {
            break;
        };
        let Some(base_source) = library.get(&format!("components/{base}.ui")) else {
            break;
        };
        for name in boolean_properties(base_source) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        current = base_source.clone();
    }
    names
}

fn outcome(library: &BTreeMap<String, String>, instance: &str) -> String {
    let mut files = library.clone();
    files.insert(
        "App.ui".into(),
        format!("component App {{ Column {{ width: 480; height: 480; {instance} }} }}"),
    );
    match Runtime::from_ui_files(&files, "App.ui", &Default::default()) {
        Ok(runtime) => format!("loads, {} control(s)", runtime.control_count()),
        Err(error) => format!("ERROR {error}"),
    }
}

#[test]
fn library_components_load_as_recorded() {
    let library = library();
    let mut out = String::new();
    for (path, source) in &library {
        let Some(name) = path
            .strip_prefix("components/")
            .and_then(|file| file.strip_suffix(".ui"))
        else {
            continue;
        };
        writeln!(
            out,
            "{name}: {}",
            outcome(&library, &format!("{name} {{ key: 'c'; }}"))
        )
        .unwrap();
        for property in inherited_boolean_properties(&library, source) {
            for value in ["true", "false"] {
                let result = outcome(
                    &library,
                    &format!("{name} {{ key: 'c'; {property}: {value}; }}"),
                );
                writeln!(out, "  {property}: {value} -> {result}").unwrap();
            }
        }
    }
    golden::assert_golden("library_components.txt", &out);
}
