// Builds a standalone native Rust application bundle (map of relative paths -> file contents)
// from a Forma Studio project so users can compile and run it directly with `cargo run --features native`
// or `cargo test` using the Rust `.ui` compiler (`forma::Runtime::from_ui_files`) with zero JS/WebView dependencies.

export function buildNativeRustExport(files, entryFile, options = {}) {
  if (!files || typeof files !== 'object') throw new Error('Передайте файлы проекта');
  const uiEntries = Object.entries(files).filter(
    ([path, content]) => typeof content === 'string' && (path.endsWith('.ui') || path.endsWith('.svg'))
  );
  if (!uiEntries.length) throw new Error('В проекте нет .ui файлов для экспорта');

  const resolvedEntry =
    entryFile && files[entryFile]
      ? entryFile
      : uiEntries.find(([p]) => p.endsWith('.ui') && !p.endsWith('.design.ui') && !p.startsWith('components/'))?.[0] ??
        uiEntries.find(([p]) => p.endsWith('.ui') && !p.endsWith('.design.ui'))?.[0] ??
        uiEntries[0][0];

  const appName = String(options.name || 'forma-native-export')
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/^-+|-+$/g, '') || 'forma-native-export';
  const formaPath = options.formaPath || '../vector-ui';

  const out = {};
  for (const [path, content] of Object.entries(files)) {
    if (typeof content === 'string') out[path] = content;
  }

  out['Cargo.toml'] = `[package]
name = "${appName}"
version = "0.1.0"
edition = "2021"
publish = false

[features]
default = []
native = ["dep:winit", "dep:softbuffer"]

[dependencies]
forma = { path = "${formaPath}" }
winit = { version = "0.30", optional = true }
softbuffer = { version = "0.4", optional = true }
`;

  const embeddedFilesCode = uiEntries
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([path]) => `    files.insert(${JSON.stringify(path)}.to_string(), include_str!(${JSON.stringify('../' + path)}).to_string());`)
    .join('\n');

  out['src/ui_bundle.rs'] = `use std::collections::BTreeMap;

pub const ENTRY_FILE: &str = ${JSON.stringify(resolvedEntry)};

pub fn project_files() -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
${embeddedFilesCode}
    files
}

pub fn create_runtime() -> Result<forma::Runtime, String> {
    let files = project_files();
    forma::Runtime::from_ui_files(&files, ENTRY_FILE, &forma::form_document::Properties::new())
}
`;

  out['src/main.rs'] = `mod ui_bundle;
${files['src/actions.rs'] ? 'mod actions;\n' : ''}
fn main() -> Result<(), String> {
    let mut runtime = ui_bundle::create_runtime()?;
    let _ = runtime.sync_ui_bindings_native()?;
    let w = runtime.render_width().max(1.0) as u32;
    let h = runtime.render_height().max(1.0) as u32;
    let pixels = runtime.pixels(w.min(1024), h.min(1024), 1.0);
    println!(
        "Forma Native Export ({}) · {} controls · {}x{} ({} RGBA bytes)",
        ui_bundle::ENTRY_FILE,
        runtime.control_count(),
        w,
        h,
        pixels.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_ui_project_compiles_and_renders_natively_in_rust() {
        let mut runtime = ui_bundle::create_runtime().expect("compile .ui project in Rust");
        assert!(runtime.control_count() > 0);
        let _ = runtime.sync_ui_bindings_native().expect("sync two-way bindings");
        let pixels = runtime.pixels(320, 200, 1.0);
        assert_eq!(pixels.len(), 320 * 200 * 4);
    }
}
`;

  out['README.md'] = `# ${appName} — Нативный Rust экспорт Forma

Сгенерировано из проекта Forma Studio (входной компонент: \`${resolvedEntry}\`).

## Быстрый запуск в нативном окне (GPU / Metal / Vulkan / DX12)
\`\`\`sh
cargo run --manifest-path ../vector-ui/Cargo.toml --features native -- ${resolvedEntry}
\`\`\`

## Сборка и проверка автономного Rust-крейта
\`\`\`sh
cargo test
cargo run
\`\`\`
`;

  return {
    name: appName,
    entry: resolvedEntry,
    files: out,
  };
}
