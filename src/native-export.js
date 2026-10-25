// Builds a standalone native Rust application bundle (map of relative paths -> file contents).
// `cargo run --features native` opens a real window; `cargo test` stays headless.

import {parse} from './language.js';
import {generateForm} from './form-codegen.js';

function updateTomlSection(manifest, name, update) {
  const header = new RegExp(`^\\[${name}\\][ \\t]*(?:#.*)?$`, 'm').exec(manifest);
  if (!header) {
    const prefix = manifest && !manifest.endsWith('\n') ? '\n' : '';
    return update(`${manifest}${prefix}\n[${name}]\n`);
  }
  const start = header.index + header[0].length;
  const rest = manifest.slice(start);
  const next = rest.search(/^\[/m);
  const end = next < 0 ? manifest.length : start + next;
  return manifest.slice(0, start) + update(manifest.slice(start, end)) + manifest.slice(end);
}

function setTomlValue(body, key, value) {
  const pattern = new RegExp(`^[ \\t]*${key}[ \\t]*=.*$`, 'm');
  if (pattern.test(body)) {
    const line = body.match(pattern)[0];
    if (key === 'forma' && line.includes('{') && !line.includes('}')) {
      throw new Error('Cargo.toml: многострочная зависимость forma не поддерживается; используйте одну строку');
    }
    return body.replace(pattern, `${key} = ${value}`);
  }
  return `${body}${body && !body.endsWith('\n') ? '\n' : ''}${key} = ${value}\n`;
}

function nativeCargoManifest(source, appName, formaPath) {
  let manifest = source || `[package]\nversion = "0.1.0"\nedition = "2021"\npublish = false\n`;
  manifest = updateTomlSection(manifest, 'package', body => {
    let next = setTomlValue(body, 'name', JSON.stringify(appName));
    if (!/^[ \t]*version[ \t]*=/m.test(next)) next = setTomlValue(next, 'version', '"0.1.0"');
    if (!/^[ \t]*edition[ \t]*=/m.test(next)) next = setTomlValue(next, 'edition', '"2021"');
    return next;
  });
  manifest = updateTomlSection(manifest, 'features', body => {
    let next = body;
    const current = next.match(/^[ \t]*native[ \t]*=[ \t]*\[([^\]\r\n]*)\]/m);
    if (/^[ \t]*native[ \t]*=/m.test(next) && !current) {
      throw new Error('Cargo.toml: feature native должна быть однострочным массивом');
    }
    const features = current?.[1].split(',').map(value => value.trim()).filter(Boolean) ?? [];
    if (!features.includes('"forma/native"')) features.push('"forma/native"');
    next = setTomlValue(next, 'native', `[${features.join(', ')}]`);
    if (!/^[ \t]*default[ \t]*=/m.test(next)) next = setTomlValue(next, 'default', '[]');
    return next;
  });
  return updateTomlSection(manifest, 'dependencies', body =>
    setTomlValue(body, 'forma', `{ path = ${JSON.stringify(formaPath)} }`)
  );
}

export function buildNativeRustExport(files, entryFile, options = {}) {
  if (!files || typeof files !== 'object') throw new Error('Передайте файлы проекта');
  const uiEntries = Object.entries(files).filter(
    ([path, content]) => typeof content === 'string' && (path.endsWith('.ui') || path.endsWith('.svg'))
  );
  const forms = uiEntries.filter(([path]) => path.endsWith('.ui'));
  if (!forms.length) throw new Error('В проекте нет .ui файлов для экспорта');

  const resolvedEntry =
    entryFile && files[entryFile]
      ? entryFile
      : forms.find(([p]) => !p.endsWith('.design.ui') && !p.startsWith('components/'))?.[0] ??
        forms.find(([p]) => !p.endsWith('.design.ui'))?.[0] ??
        forms[0][0];
  if (!resolvedEntry.endsWith('.ui')) throw new Error('Входной файл native-экспорта должен быть .ui');

  const appName = String(options.name || options.crateName || 'forma-native-export')
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/^-+|-+$/g, '') || 'forma-native-export';
  const formaPath = options.formaPath || options.formaDependencyPath || '../vector-ui';
  const document = parse(files[resolvedEntry]);
  const typed = Boolean(document.defaults.contextType);
  if (typed && typeof files['src/app.rs'] !== 'string') {
    throw new Error(
      'Для формы с contextType нужен src/app.rs с функцией create_form(), которая создаёт ViewModel, форму и регистрирует обработчики'
    );
  }
  if (typed && Object.hasOwn(files, 'src/forma_generated.rs')) {
    throw new Error('src/forma_generated.rs зарезервирован для результата генерации');
  }
  if (!typed && Object.hasOwn(files, 'src/ui_bundle.rs')) {
    throw new Error('src/ui_bundle.rs зарезервирован для результата генерации');
  }

  const generated = typed ? generateForm(files, resolvedEntry, {state: options.state ?? {}, nativeApp: true}) : null;
  const out = {};
  for (const [path, content] of Object.entries(files)) {
    if (typeof content === 'string') out[path] = content;
  }

  out['Cargo.toml'] = nativeCargoManifest(files['Cargo.toml'], appName, formaPath);

  if (typed) {
    out['src/forma_generated.rs'] = generated.rust;
    out['src/main.rs'] = `mod app;
mod forma_generated;

// Типы contextType: 'crate::…' экспортируются из пользовательского app.rs.
pub use app::*;

#[cfg(feature = "native")]
fn main() -> Result<(), String> {
    use forma::native_window::{run, NativeWindowConfig};

    let mut form = app::create_form()?;
    let (width, height) = {
        let runtime = form.runtime()?;
        (runtime.render_width().max(1.0), runtime.render_height().max(1.0))
    };
    run(
        form,
        NativeWindowConfig {
            title: ${JSON.stringify(appName)}.into(),
            width,
            height,
            ..Default::default()
        },
    )
}

#[cfg(not(feature = "native"))]
fn main() {
    eprintln!("Для запуска нативного окна: cargo run --features native");
}

#[cfg(test)]
mod tests {
    #[test]
    fn exported_typed_form_and_handlers_compile_and_render() {
        let mut form = crate::app::create_form().expect("create typed form and register handlers");
        assert!(form.runtime().expect("sync form").control_count() > 0);
        let pixels = form.runtime().expect("render form").pixels(320, 200, 1.0);
        assert_eq!(pixels.len(), 320 * 200 * 4);
    }
}
`;
  } else {
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

#[cfg(feature = "native")]
fn main() -> Result<(), String> {
    use forma::native_window::{run, NativeWindowConfig};

    let runtime = ui_bundle::create_runtime()?;
    let width = runtime.render_width().max(1.0);
    let height = runtime.render_height().max(1.0);
    run(
        runtime,
        NativeWindowConfig {
            title: ${JSON.stringify(appName)}.into(),
            width,
            height,
            ..Default::default()
        },
    )
}

#[cfg(not(feature = "native"))]
fn main() -> Result<(), String> {
    let mut runtime = ui_bundle::create_runtime()?;
    runtime.sync_ui_bindings_native()?;
    let pixels = runtime.pixels(320, 200, 1.0);
    println!(
        "Forma Native Export ({}) · {} controls · {} RGBA bytes",
        ui_bundle::ENTRY_FILE,
        runtime.control_count(),
        pixels.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn exported_ui_project_compiles_and_renders_natively_in_rust() {
        let mut runtime = crate::ui_bundle::create_runtime().expect("compile .ui project in Rust");
        assert!(runtime.control_count() > 0);
        runtime.sync_ui_bindings_native().expect("sync two-way bindings");
        let pixels = runtime.pixels(320, 200, 1.0);
        assert_eq!(pixels.len(), 320 * 200 * 4);
    }
}
`;
  }

  const typedReadme = typed
    ? `\n## ViewModel и обработчики\n\nФорма генерируется в \`src/forma_generated.rs\`. Пользовательский \`src/app.rs\` сохраняется и должен экспортировать \`create_form()\`: она создаёт ViewModel и форму, регистрирует обработчики \`on_…\` и возвращает форму. Типизированные \`state.method()\` и \`events.name()\` выполняются через эти bindings во время обработки native-ввода.\n`
    : `\n## Состояние и события\n\nОкно обрабатывает ввод и синхронизирует встроенные двусторонние привязки retained \`.ui\`-документа. Для пользовательской Rust ViewModel и обработчиков \`state.method()\` / \`events.name()\` используйте форму с \`contextType\` и \`src/app.rs\`; произвольный \`actions.*\` автоматически не вызывается.\n`;

  out['README.md'] = `# ${appName} — нативный Rust экспорт Forma

Сгенерировано из проекта Forma Studio (входной компонент: \`${resolvedEntry}\`).

## Запуск настоящего нативного окна

\`\`\`sh
# Интерактивный выбор бэкенда (egui / svet) при старте окна:
cargo run --features native

# Явный выбор бэкенда egui или svet:
cargo run --features native -- --backend egui
cargo run --features native -- --backend svet
\`\`\`

Окно использует общий Forma Runtime, winit event loop и поддерживает выбор бэкенда \`egui\` или \`svet\` при старте (а также горячее переключение по клавише \`F7\`). Ввод, фокус, прокрутка, IME, анимации и изменения ViewModel запускают перерисовку.
${typedReadme}
## Headless-проверка

\`\`\`sh
cargo test
\`\`\`

Тест не открывает окно и проверяет компиляцию \`.ui\`, bindings и рендеринг.
`;

  return {
    name: appName,
    entry: resolvedEntry,
    files: out,
  };
}
