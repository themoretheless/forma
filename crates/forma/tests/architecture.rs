//! Keeps module dependencies pointing downward.
//!
//! Every source module has a layer; production code may use modules of its own layer or below.
//! Edges that already break the rule are listed in `KNOWN_VIOLATIONS`. The list is a ratchet: a new
//! upward edge fails the test, and so does a listed edge that no longer exists, so the list shrinks
//! as modules are untangled and can never silently grow.

use std::collections::BTreeSet;
use std::path::Path;

/// Module name and layer, lowest first. `lib` stands for items defined in the crate root.
const LAYERS: &[(&str, u8)] = &[
    ("blend", 0),
    ("frame_stats", 0),
    ("geometry", 0),
    ("markup", 0),
    ("control_state", 1),
    ("reveal", 1),
    ("shape", 1),
    ("template", 1),
    ("text", 1),
    ("text_input", 1),
    ("canvas", 2),
    ("display_list", 2),
    ("form_document", 2),
    ("raster_cache", 2),
    ("lib", 3),
    ("binding", 4),
    ("native_document_graph", 4),
    ("runtime", 4),
    ("control_models_wasm", 5),
    ("gpu", 5),
    ("mcp", 5),
    ("metal_resize", 5),
    ("native_accessibility", 5),
    ("native_clipboard", 5),
    ("native_gpu", 5),
    ("native_window", 5),
];

/// Upward edges that exist today, as `from -> to`. Remove an entry when its edge is gone.
const KNOWN_VIOLATIONS: &[&str] = &["display_list -> lib", "raster_cache -> lib"];

/// Names in the crate root that are re-exports of a module rather than root items.
const ROOT_REEXPORTS: &[(&str, &str)] = &[("Runtime", "runtime")];

fn layer(module: &str) -> Option<u8> {
    LAYERS
        .iter()
        .find(|(name, _)| *name == module)
        .map(|(_, layer)| *layer)
}

/// The module a source file belongs to: `form_document/eval.rs` is part of `form_document`.
fn module_of(relative: &Path) -> String {
    let first = relative
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    first.strip_suffix(".rs").unwrap_or(&first).to_owned()
}

/// Production text only: unit tests may reach across layers to set up fixtures.
fn production(source: &str) -> &str {
    source
        .find("#[cfg(test)]")
        .map_or(source, |end| &source[..end])
}

fn resolve(name: &str) -> String {
    if layer(name).is_some() {
        name.to_owned()
    } else if let Some((_, module)) = ROOT_REEXPORTS.iter().find(|(item, _)| *item == name) {
        (*module).to_owned()
    } else {
        "lib".to_owned()
    }
}

fn identifier(text: &str) -> String {
    text.trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Every module named after `crate::`, including each entry of a `crate::{a, b::c}` group.
fn targets(source: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = source;
    while let Some(at) = rest.find("crate::") {
        rest = &rest[at + "crate::".len()..];
        if let Some(group) = rest.strip_prefix('{') {
            let mut depth = 1usize;
            let mut entry = String::new();
            for c in group.chars() {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                if depth == 0 || (depth == 1 && c == ',') {
                    let name = identifier(&entry);
                    if !name.is_empty() {
                        found.insert(resolve(&name));
                    }
                    entry.clear();
                    if depth == 0 {
                        break;
                    }
                } else if depth == 1 || !entry.is_empty() {
                    entry.push(c);
                }
            }
        } else {
            let name = identifier(rest);
            if !name.is_empty() {
                found.insert(resolve(&name));
            }
        }
    }
    found
}

fn collect(dir: &Path, root: &Path, edges: &mut BTreeSet<String>, unknown: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, root, edges, unknown);
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        // Unit-test files may reach across layers to set up fixtures, like inline test modules.
        let file = path.file_stem().unwrap().to_string_lossy();
        if file == "tests" || file.ends_with("_tests") {
            continue;
        }
        let from = module_of(path.strip_prefix(root).unwrap());
        // The binary is a client of the library, not a layer of it.
        if from == "main" {
            continue;
        }
        let Some(from_layer) = layer(&from) else {
            unknown.insert(from);
            continue;
        };
        let source = std::fs::read_to_string(&path).unwrap();
        for to in targets(production(&source)) {
            if to != from && layer(&to).unwrap() > from_layer {
                edges.insert(format!("{from} -> {to}"));
            }
        }
    }
}

#[test]
fn modules_depend_only_on_their_own_layer_or_below() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let (mut edges, mut unknown) = (BTreeSet::new(), BTreeSet::new());
    collect(&root, &root, &mut edges, &mut unknown);
    assert!(
        unknown.is_empty(),
        "modules without a layer in tests/architecture.rs: {unknown:?}"
    );

    let known: BTreeSet<String> = KNOWN_VIOLATIONS
        .iter()
        .map(|edge| (*edge).to_owned())
        .collect();
    let new: Vec<_> = edges.difference(&known).collect();
    assert!(new.is_empty(), "new upward dependencies: {new:?}");
    let fixed: Vec<_> = known.difference(&edges).collect();
    assert!(
        fixed.is_empty(),
        "these edges are gone, remove them from KNOWN_VIOLATIONS: {fixed:?}"
    );
}
