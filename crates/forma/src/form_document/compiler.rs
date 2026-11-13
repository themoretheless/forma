use super::{Definition, Document, Expression, Node, Properties, Value, number, parse_svg_shapes};
use std::collections::BTreeMap;

mod components;
mod json;

use components::*;
pub(super) use json::*;

pub(in crate::form_document) fn compile_ui_files_to_document(
    files: &BTreeMap<String, String>,
    entry_file: &str,
    initial_state: &Properties,
) -> Result<Document, String> {
    let mut stack = Vec::new();
    let scene_ast = compile_single_component_ast(files, entry_file, &mut stack)?;
    let name = scene_ast
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("Scene")
        .to_string();
    let mut scene_nodes = scene_ast
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .map(json_to_doc_node)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    if scene_nodes.len() != 1 {
        return Err("Scene component must have a single layout root node".into());
    }
    let mut root = scene_nodes.remove(0);

    // Attach scene-level scope (`defaults`, `tokens`, `i18n`, `styles`, `enums`, `structs`, `lets`, `matches`)
    let scene_def = ast_to_definition(&scene_ast)?;
    if !scene_def.defaults.is_empty() {
        root.props
            .insert("__scope".into(), Value::Object(scene_def.defaults));
    }

    // Parse all SVG assets in `files`
    let mut images = BTreeMap::new();
    for (path, content) in files {
        if path.ends_with(".svg") {
            images.insert(path.clone(), parse_svg_shapes(content)?);
        }
    }

    // Discover and compile all referenced component definitions transitively
    let mut definitions = BTreeMap::new();
    let mut pending = std::collections::BTreeSet::new();
    collect_component_names(&root, &mut pending);
    for path in files.keys() {
        if let Some(stem) = path.strip_suffix(".component.ui") {
            pending.insert(stem.to_string());
        }
    }
    let mut visited = std::collections::BTreeSet::new();
    while let Some(comp_name) = pending.pop_first() {
        if !visited.insert(comp_name.clone()) {
            continue;
        }
        if resolve_ui_file(files, &comp_name).is_some() {
            let comp_ast = compile_single_component_ast(files, &comp_name, &mut Vec::new())?;
            let def = ast_to_definition(&comp_ast)?;
            for n in &def.nodes {
                collect_component_names(n, &mut pending);
            }
            definitions.insert(comp_name, def);
        }
    }
    for (builtin_name, builtin_def) in builtin_definitions() {
        if !definitions.contains_key(*builtin_name) {
            definitions.insert((*builtin_name).into(), builtin_def.clone());
        }
    }

    // Merge initial state from `stateDefinitions` defaults + `designs` + `initial_state`
    let mut root_state = Properties::new();
    if let Some(state_defs) = scene_ast
        .get("stateDefinitions")
        .and_then(|s| s.as_object())
    {
        for (k, spec) in state_defs {
            if let Some(def_val) = spec.get("default").or_else(|| spec.get("defaultValue")) {
                if !def_val.is_null() {
                    root_state.insert(k.clone(), json_to_doc_value(def_val)?);
                }
            }
        }
    }
    if let Some(designs) = scene_ast.get("designs").and_then(|d| d.as_array()) {
        for design_ref in designs {
            if let Some(design_file) = design_ref.as_str() {
                if let Some((_, design_src)) = resolve_ui_file(files, design_file) {
                    let design_ast = themoretheless_tokenizer_forma::parse(design_src)
                        .map_err(|e| e.to_string())?;
                    let patch = themoretheless_tokenizer_forma::semantics::design_state_patch(
                        &design_ast,
                        None,
                    )
                    .map_err(|e| e.to_string())?;
                    if let Some(obj) = patch.as_object() {
                        for (k, v) in obj {
                            root_state.insert(k.clone(), json_to_doc_value(v)?);
                        }
                    }
                }
            }
        }
    }
    for (k, v) in initial_state {
        root_state.insert(k.clone(), v.clone());
    }

    Ok(Document {
        name,
        root,
        definitions,
        fallback: Properties::new(),
        images,
        states: vec![root_state],
    })
}
