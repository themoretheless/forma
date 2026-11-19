use forma::mcp::McpApp;
use forma_mcp::McpServer;
use forma_mcp::bridge::Bridge;
use std::error::Error;
use std::io;

#[derive(Debug, Default, PartialEq, Eq)]
struct CliArgs {
    source_path: Option<String>,
    template_path: Option<String>,
    backend: Option<&'static str>,
    platform: Option<&'static str>,
}

fn parse_backend(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "egui" | "1" => Some("egui"),
        "svet" | "2" => Some("svet"),
        "ask" | "choose" => Some("ask"),
        _ => None,
    }
}

fn parse_platform(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "desktop" | "macos" | "windows" | "linux" => Some("desktop"),
        "ios" | "iphone" | "ipad" => Some("ios"),
        "android" | "pixel" => Some("android"),
        _ => None,
    }
}

fn parse_cli_args(args: impl IntoIterator<Item = String>) -> CliArgs {
    let mut positional = Vec::new();
    let mut backend = None;
    let mut platform = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--mcp" | "mcp" => {}
            "--egui" => backend = Some("egui"),
            "--svet" => backend = Some("svet"),
            "--ask" | "--choose-backend" => backend = Some("ask"),
            "--desktop" => platform = Some("desktop"),
            "--ios" => platform = Some("ios"),
            "--android" => platform = Some("android"),
            "--backend" | "-b" => {
                if let Some(next) = iter.next()
                    && let Some(parsed) = parse_backend(&next)
                {
                    backend = Some(parsed);
                }
            }
            "--platform" | "-p" => {
                if let Some(next) = iter.next()
                    && let Some(parsed) = parse_platform(&next)
                {
                    platform = Some(parsed);
                }
            }
            _ if arg.starts_with("--backend=") => {
                if let Some(parsed) = parse_backend(&arg["--backend=".len()..]) {
                    backend = Some(parsed);
                }
            }
            _ if arg.starts_with("--platform=") => {
                if let Some(parsed) = parse_platform(&arg["--platform=".len()..]) {
                    platform = Some(parsed);
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
    }
}

fn build_app(args: CliArgs) -> Result<McpApp, Box<dyn Error>> {
    let mut app = if let Some(source_path) = args.source_path {
        let source = std::fs::read_to_string(source_path)?;
        let template = match args.template_path {
            Some(path) => std::fs::read_to_string(path)?,
            None => forma::BUTTON_COMPONENT.to_owned(),
        };
        let runtime = forma::Runtime::from_sources(&source, &template)
            .map_err(|error| io::Error::other(error.to_string()))?;
        McpApp::from_runtime(runtime, source, template)
    } else {
        McpApp::new()
    };

    if let Some(backend) = args.backend {
        app.backend = backend.to_owned();
    }
    if let Some(platform) = args.platform {
        app.platform = platform.to_owned();
    }
    app.runtime
        .set_svet_document_scheduling(app.backend == "svet");
    Ok(app)
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_cli_args(std::env::args().skip(1));
    let app = build_app(args)?;
    let bridge = Bridge::from_environment()?;
    McpServer::new(bridge, app).serve_io(io::stdin().lock(), io::stdout())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> CliArgs {
        parse_cli_args(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn parses_optional_ui_paths_backend_and_platform_flags() {
        assert_eq!(cli(&[]), CliArgs::default());
        assert_eq!(
            cli(&["--mcp", "--svet", "--ios", "a.ui", "b.ui", "ignored.ui"]),
            CliArgs {
                source_path: Some("a.ui".into()),
                template_path: Some("b.ui".into()),
                backend: Some("svet"),
                platform: Some("ios"),
            }
        );
        assert_eq!(
            cli(&[
                "mcp",
                "--backend",
                "egui",
                "-p",
                "pixel",
                "--backend=ask",
                "--platform=desktop",
                "scene.ui",
            ]),
            CliArgs {
                source_path: Some("scene.ui".into()),
                template_path: None,
                backend: Some("ask"),
                platform: Some("desktop"),
            }
        );
        assert_eq!(
            cli(&["--backend", "nope", "-p", "nope", "scene.ui"]),
            CliArgs {
                source_path: Some("scene.ui".into()),
                template_path: None,
                backend: None,
                platform: None,
            }
        );
    }
}
