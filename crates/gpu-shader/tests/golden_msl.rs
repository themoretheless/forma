//! Pins the WGSL to MSL translation of every shader in the repository.
//!
//! The crate had no tests: its only check was whatever the GPU crates happened to exercise on a
//! Mac. Each tracked `.wgsl` file outside recorded evidence is parsed, validated and translated,
//! and the MSL text, binding map or error is compared with a recorded file. Run with
//! `FORMA_BLESS=1` to record an intended change.

use gpu_shader::back::msl;
use gpu_shader::front::wgsl;
use gpu_shader::valid::{Capabilities, ValidationFlags, Validator};
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Directories holding live shaders, relative to the repository root.
const ROOTS: &[&str] = &[
    "crates/forma/src",
    "crates/compute-core/shaders",
    "crates/compute-core/src",
    "crates/compute-core/examples",
    "crates/math-compute/src",
    "crates/raster-core/shaders",
];

fn shaders(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            shaders(&path, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "wgsl")
        {
            out.push(path);
        }
    }
}

fn translate(source: &str) -> String {
    let module = match wgsl::parse_str(source) {
        Ok(module) => module,
        Err(error) => return format!("PARSE ERROR {error}\n"),
    };
    let info = match Validator::new(ValidationFlags::all(), Capabilities::all()).validate(&module) {
        Ok(info) => info,
        Err(error) => return format!("VALIDATION ERROR {error:?}\n"),
    };
    match msl::write_string(
        &module,
        &info,
        &msl::Options::default(),
        &Default::default(),
    ) {
        Ok((source, bindings)) => format!("{source}\n--- bindings ---\n{bindings:#?}\n"),
        Err(error) => format!("MSL ERROR {error}\n"),
    }
}

#[test]
fn every_repository_shader_translates_as_recorded() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ROOTS {
        shaders(&root.join(dir), &mut files);
    }
    files.sort();
    assert!(
        files.len() > 100,
        "found only {} shaders; did the layout change?",
        files.len()
    );

    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let bless = std::env::var_os("FORMA_BLESS").is_some();
    let mut index = String::new();
    let mut differing = Vec::new();
    for file in &files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let name = relative.replace('/', "__").replace(".wgsl", ".msl.txt");
        let actual = translate(&std::fs::read_to_string(file).unwrap());
        writeln!(index, "{relative} -> {name}").unwrap();
        let path = golden.join(&name);
        if bless {
            std::fs::write(&path, &actual).unwrap();
        } else if std::fs::read_to_string(&path)
            .ok()
            .map(|text| text.replace("\r\n", "\n"))
            != Some(actual.clone())
        {
            differing.push(relative);
        }
    }
    let index_path = golden.join("INDEX.txt");
    if bless {
        std::fs::write(&index_path, &index).unwrap();
    } else {
        assert_eq!(
            std::fs::read_to_string(&index_path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            index,
            "the set of shaders changed; record it with FORMA_BLESS=1"
        );
    }
    assert!(
        differing.is_empty(),
        "translation changed for: {differing:#?}"
    );
}
