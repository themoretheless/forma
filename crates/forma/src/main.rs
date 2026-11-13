//! The `forma` binary: reads the command line, loads the scene and hands it to the dev window
//! or to the MCP server. The window itself is `forma::native_window`.

use forma::native_window::{DevProject, TargetPlatform, UiBackend, run_dev};

/// What the command line asks for.
struct CliArgs {
    source_path: Option<String>,
    template_path: Option<String>,
    backend: Option<UiBackend>,
    platform: TargetPlatform,
    mcp: bool,
}

/// Reads the arguments after the program name. `backend`, `platform` and `mcp` are the values
/// already resolved from the environment; a flag replaces them.
fn parse_cli_args(
    args: impl IntoIterator<Item = String>,
    mut backend: Option<UiBackend>,
    mut platform: TargetPlatform,
    mut mcp: bool,
) -> CliArgs {
    let mut positional = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--mcp" | "mcp" => mcp = true,
            "--egui" | "--backend=egui" => backend = Some(UiBackend::Egui),
            "--svet" | "--backend=svet" => backend = Some(UiBackend::Svet),
            "--ask" | "--choose-backend" | "--backend=ask" => backend = None,
            "--backend" | "-b" => {
                if let Some(next) = iter.next() {
                    if matches!(next.trim().to_ascii_lowercase().as_str(), "ask" | "choose") {
                        backend = None;
                    } else if let Some(b) = UiBackend::from_arg(&next) {
                        backend = Some(b);
                    }
                }
            }
            "--ios" | "--platform=ios" => platform = TargetPlatform::Ios,
            "--android" | "--platform=android" => platform = TargetPlatform::Android,
            "--desktop" | "--platform=desktop" => platform = TargetPlatform::Desktop,
            "--platform" | "-p" => {
                if let Some(next) = iter.next() {
                    if let Some(p) = TargetPlatform::from_arg(&next) {
                        platform = p;
                    }
                }
            }
            _ => positional.push(arg),
        }
    }
    let mut positional = positional.into_iter();
    CliArgs {
        source_path: positional.next(),
        template_path: positional.next(),
        backend,
        platform,
        mcp,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_cli_args(
        std::env::args().skip(1),
        UiBackend::resolve_from_env_and_args(),
        TargetPlatform::resolve_from_env_and_args(),
        std::env::var("FORMA_MCP").as_deref() == Ok("1"),
    );
    let mut project = DevProject::load(args.source_path, args.template_path)?;
    if args.mcp {
        project
            .runtime
            .set_svet_document_scheduling(args.backend == Some(UiBackend::Svet));
        let mut mcp_app =
            forma::mcp::McpApp::from_runtime(project.runtime, project.source, project.template);
        mcp_app.backend = args
            .backend
            .map_or_else(|| "egui".into(), |b| b.as_str().into());
        mcp_app.platform = args.platform.as_str().into();
        return forma::mcp::serve_stdio(mcp_app).map_err(|e| std::io::Error::other(e).into());
    }
    run_dev(project, args.backend, args.platform)?;
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn cli(args: &[&str]) -> CliArgs {
        parse_cli_args(
            args.iter().map(|arg| (*arg).to_owned()),
            Some(UiBackend::Svet),
            TargetPlatform::Desktop,
            false,
        )
    }

    #[test]
    fn command_line_separates_flags_from_the_scene_and_template_paths() {
        let args = cli(&[]);
        assert!(args.source_path.is_none() && args.template_path.is_none());
        assert_eq!(args.backend, Some(UiBackend::Svet));
        assert_eq!(args.platform, TargetPlatform::Desktop);
        assert!(!args.mcp);

        let args = cli(&["--egui", "a.ui", "--ios", "b.ui", "ignored.ui", "--mcp"]);
        assert_eq!(args.source_path.as_deref(), Some("a.ui"));
        assert_eq!(args.template_path.as_deref(), Some("b.ui"));
        assert_eq!(args.backend, Some(UiBackend::Egui));
        assert_eq!(args.platform, TargetPlatform::Ios);
        assert!(args.mcp);

        assert_eq!(cli(&["--ask"]).backend, None);
        assert_eq!(cli(&["-b", "choose"]).backend, None);
        assert_eq!(cli(&["--backend", "egui"]).backend, Some(UiBackend::Egui));
        // An unknown value is consumed and changes nothing.
        let args = cli(&["--backend", "nope", "-p", "nope", "scene.ui"]);
        assert_eq!(args.backend, Some(UiBackend::Svet));
        assert_eq!(args.platform, TargetPlatform::Desktop);
        assert_eq!(args.source_path.as_deref(), Some("scene.ui"));
        assert_eq!(cli(&["-p", "pixel"]).platform, TargetPlatform::Android);
        assert_eq!(
            cli(&["--platform=android", "--desktop"]).platform,
            TargetPlatform::Desktop
        );
        assert!(cli(&["mcp"]).mcp);
    }
}
