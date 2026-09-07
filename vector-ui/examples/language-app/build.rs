use std::{env, path::PathBuf, process::Command};

fn main() {
    let project = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = project.join("../../..");
    for path in [
        project.join("ui"),
        project.join("components"),
        root.join("src"),
        root.join("scripts/generate-form.mjs"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-env-changed=FORMA_NODE");
    let status = Command::new(env::var_os("FORMA_NODE").unwrap_or_else(|| "node".into()))
        .arg(root.join("scripts/generate-form.mjs"))
        .arg("--project")
        .arg(&project)
        .args(["--entry", "ui/TaskList.ui", "--output"])
        .arg(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("task_list.rs"))
        .status()
        .expect("Node.js is required to generate the form");
    assert!(status.success(), "Forma form generation failed");
}
