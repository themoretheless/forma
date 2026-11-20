use forma_build::{ProjectBuildSpec, generate_project};
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> &'static str {
    "Usage: forma-build --project DIR --output FILE [--entry ui/Form.ui] [--state state.json] [--native-app]"
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut project: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut entry: Option<String> = None;
    let mut state_file: Option<PathBuf> = None;
    let mut native_app = false;

    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--native-app" => native_app = true,
            "--project" | "--output" | "--entry" | "--state" => {
                let value = args.next().ok_or_else(|| usage().to_string())?;
                match flag.as_str() {
                    "--project" => project = Some(PathBuf::from(value)),
                    "--output" => output = Some(PathBuf::from(value)),
                    "--entry" => entry = Some(value),
                    "--state" => state_file = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            _ => return Err(usage().to_string()),
        }
    }

    let project = project.ok_or("Нужны --project и --output")?;
    let output = output.ok_or("Нужны --project и --output")?;
    let generated = generate_project(&ProjectBuildSpec {
        project: &project,
        output: &output,
        entry: entry.as_deref(),
        state_file: state_file.as_deref(),
        native_app,
    })?;
    println!("Generated {}: {}", generated.form_name, output.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}
