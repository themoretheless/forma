use std::{env, path::PathBuf};

fn main() {
    let project = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("task_list.rs");
    forma_build::build_script_generate(&forma_build::ProjectBuildSpec {
        project: &project,
        output: &output,
        entry: Some("ui/TaskList.ui"),
        state_file: None,
        native_app: false,
    })
    .expect("Forma form generation failed");
}
