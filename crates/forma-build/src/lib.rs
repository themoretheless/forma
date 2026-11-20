//! Autonomous `.ui` component linker, SVG parser and typed Rust form code generator.

mod codegen;
mod expr;
mod linker;
mod svg;

pub use codegen::{GeneratedForm, SourceLocation};

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

#[derive(Debug, Clone, Default)]
pub struct GenerateOptions {
    pub state: Option<Value>,
    pub native_app: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOutput {
    pub rust: String,
    pub source_map: BTreeMap<usize, SourceLocation>,
    pub form_name: String,
}

#[derive(Debug, Clone)]
pub struct ProjectBuildSpec<'a> {
    pub project: &'a Path,
    pub output: &'a Path,
    pub entry: Option<&'a str>,
    pub state_file: Option<&'a Path>,
    pub native_app: bool,
}

/// Generates typed Rust bindings and embedded `Document` for a single `.ui` form entry.
pub fn generate_form(
    files: &BTreeMap<String, String>,
    entry: &str,
    options: &GenerateOptions,
) -> Result<GeneratedForm, String> {
    let empty = Value::Object(Default::default());
    let state = options.state.as_ref().unwrap_or(&empty);
    codegen::generate_form_impl(files, entry, state, options.native_app)
}

fn collect_tree(
    root: &Path,
    dir: &str,
    files: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let full = root.join(dir);
    if !full.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(&full).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return Err(format!("Не поддерживается ссылка {dir}"));
    }
    let mut entries: Vec<_> = fs::read_dir(&full)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        let rel = format!("{dir}/{name_str}");
        let file_type = entry.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            return Err(format!("Не поддерживается ссылка {rel}"));
        }
        if file_type.is_dir() {
            collect_tree(root, &rel, files)?;
        } else if name_str.ends_with(".ui") || name_str.ends_with(".svg") {
            let text = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
            files.insert(rel, text);
        }
    }
    Ok(())
}

fn normalize_relative_entry(entry: &str) -> Result<String, String> {
    let normalized = entry.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("entry должен быть относительным путём внутри проекта".into());
    }
    Ok(normalized)
}

fn write_if_changed(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    fs::write(path, content).map_err(|e| e.to_string())
}

/// Compiles selected project `.ui` forms into `output` and `output.map.json`.
pub fn generate_project(spec: &ProjectBuildSpec<'_>) -> Result<ProjectOutput, String> {
    let mut files = BTreeMap::new();
    collect_tree(spec.project, "components", &mut files)?;
    collect_tree(spec.project, "assets", &mut files)?;

    let entries = if let Some(entry_raw) = spec.entry {
        let entry = normalize_relative_entry(entry_raw)?;
        let entry_path = spec.project.join(&entry);
        let text = fs::read_to_string(&entry_path)
            .map_err(|e| format!("Форма не найдена: {entry}: {e}"))?;
        files.insert(entry.clone(), text);
        vec![entry]
    } else {
        collect_tree(spec.project, "ui", &mut files)?;
        let mut selected = Vec::new();
        for (name, src) in &files {
            if name.starts_with("ui/") && name.ends_with(".ui") && !name.ends_with(".design.ui") {
                let doc =
                    linker::parse_ui_document(src, false).map_err(|e| format!("{name}: {e}"))?;
                if doc
                    .get("defaults")
                    .and_then(|d| d.get("contextType"))
                    .is_some()
                {
                    selected.push(name.clone());
                }
            }
        }
        selected
    };

    if entries.is_empty() {
        return Err("Нет форм с contextType".into());
    }

    let state = match spec.state_file {
        Some(path) => {
            let full = if path.is_absolute() {
                path.to_path_buf()
            } else {
                spec.project.join(path)
            };
            let raw = fs::read_to_string(&full).map_err(|e| e.to_string())?;
            Some(serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string())?)
        }
        None => None,
    };

    let options = GenerateOptions {
        state,
        native_app: spec.native_app,
    };

    let mut rust = String::new();
    let mut source_map = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut ordered_names = Vec::new();
    for entry in &entries {
        let result = generate_form(&files, entry, &options)?;
        if !names.insert(result.form_name.clone()) {
            return Err(format!("Повторное имя формы {}", result.form_name));
        }
        ordered_names.push(result.form_name);
        let offset = rust.split('\n').count().saturating_sub(1);
        for (line, loc) in result.source_map {
            source_map.insert(line + offset, loc);
        }
        rust.push_str(&result.rust);
        rust.push('\n');
    }

    write_if_changed(spec.output, &rust)?;
    let map_path = Path::new(&format!("{}.map.json", spec.output.display())).to_path_buf();
    let mut map_json = serde_json::to_string_pretty(&source_map).map_err(|e| e.to_string())?;
    map_json.push('\n');
    write_if_changed(&map_path, &map_json)?;

    Ok(ProjectOutput {
        rust,
        source_map,
        form_name: ordered_names.join(", "),
    })
}

/// Runs `generate_project` from a Cargo `build.rs`, emitting `cargo:rerun-if-changed` directives.
pub fn build_script_generate(spec: &ProjectBuildSpec<'_>) -> Result<ProjectOutput, String> {
    for dir in ["ui", "components", "assets"] {
        let full = spec.project.join(dir);
        if full.exists() {
            println!("cargo:rerun-if-changed={}", full.display());
        }
    }
    if let Some(entry) = spec.entry {
        let full = spec.project.join(entry);
        if full.exists() {
            println!("cargo:rerun-if-changed={}", full.display());
        }
    }
    if let Some(state) = spec.state_file {
        let full = if state.is_absolute() {
            state.to_path_buf()
        } else {
            spec.project.join(state)
        };
        println!("cargo:rerun-if-changed={}", full.display());
    }
    generate_project(spec)
}
