import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join, resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {buildNativeRustExport} from '../src/native-export.js';
import {sampleProject} from '../src/sample-project.js';

const typedProject = {
  'Cargo.toml': `[package] # application metadata
name = "counter-source"
version = "0.2.0"
edition = "2021"

[features] # optional application features
extra = []

[dependencies] # user dependencies must survive
serde = "1"
`,
  'components/Button.ui': sampleProject['components/Button.ui'],
  'ui/Counter.ui': `component Counter {
    contextType: 'crate::CounterVm';
    event increment();
    Frame {
        width: 320;
        height: 200;
        padding: 16;
        gap: 12;
        Button { key: 'increment'; text: '+1'; clicked -> events.increment(); }
        Text { text: state.count; color: #ffffff; }
    }
  }`,
  'src/app.rs': `use crate::forma_generated::Counter;
use forma::binding::Property;
use std::rc::Rc;

pub struct CounterVm {
    pub count: Property<String>,
}

fn with_view_model(model: Rc<CounterVm>) -> Result<Counter, String> {
    let form = Counter::new(model)?;
    form.on_increment(|event| {
        let next = event.context.count.get().parse::<i32>().unwrap_or(0) + 1;
        event.context.count.set(next.to_string());
    });
    Ok(form)
}

pub fn create_form() -> Result<Counter, String> {
    with_view_model(Rc::new(CounterVm { count: Property::new("0".into()) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_form_dispatches_the_registered_event_handler() {
        let model = Rc::new(CounterVm { count: Property::new("0".into()) });
        let mut form = with_view_model(model.clone()).unwrap();
        let before = form.runtime().unwrap().pixels(320, 200, 1.0);
        form.update(|runtime| runtime.activate_control(0)).unwrap();
        assert_eq!(model.count.get(), "1");
        let after = form.runtime().unwrap().pixels(320, 200, 1.0);
        assert_ne!(before, after, "handler must update the rendered ViewModel text");
    }
}
`,
};

const runtimeProject = {
  'ui/SearchWindow.ui': `component SearchWindow {
    Frame {
        padding: 32;
        gap: 16;
        Text { text: 'Runtime export'; color: #e8edf7; }
        Button { text: 'Найти документы'; clicked -> actions.search(); }
    }
  }`,
  'components/Button.ui': sampleProject['components/Button.ui'],
};

test('buildNativeRustExport generates a real window and a headless test for a Runtime project', () => {
  const bundle = buildNativeRustExport(runtimeProject, 'ui/SearchWindow.ui', {
    name: 'Knowledge Workspace',
  });
  assert.equal(bundle.name, 'knowledge-workspace');
  assert.equal(bundle.entry, 'ui/SearchWindow.ui');
  assert.ok(bundle.files['Cargo.toml'].includes('name = "knowledge-workspace"'));
  assert.ok(bundle.files['Cargo.toml'].includes('forma = { path = "../vector-ui" }'));
  assert.ok(bundle.files['Cargo.toml'].includes('native = ["forma/native"]'));
  assert.ok(bundle.files['src/ui_bundle.rs'].includes('pub const ENTRY_FILE: &str = "ui/SearchWindow.ui";'));
  assert.ok(bundle.files['src/ui_bundle.rs'].includes('include_str!("../ui/SearchWindow.ui")'));
  assert.ok(bundle.files['src/main.rs'].includes('mod ui_bundle;'));
  assert.ok(bundle.files['src/main.rs'].includes('#[cfg(feature = "native")]'));
  assert.ok(bundle.files['src/main.rs'].includes('forma::native_window::{run, NativeWindowConfig}'));
  assert.ok(bundle.files['src/main.rs'].includes('runtime.sync_ui_bindings_native()'));
  assert.ok(bundle.files['src/main.rs'].includes('exported_ui_project_compiles_and_renders_natively_in_rust'));
  assert.ok(bundle.files['README.md'].includes('cargo run --features native'));
  assert.ok(bundle.files['ios/Info.plist'].includes('dev.forma.knowledgeworkspace'));
  assert.ok(bundle.files['ios/run-ios.sh'].includes('--platform ios'));
  assert.ok(bundle.files['android/app/src/main/AndroidManifest.xml'].includes('android.app.NativeActivity'));
  assert.ok(bundle.files['android/build.gradle.kts'].includes('arm64-v8a'));
  assert.ok(bundle.files['android/run-android.sh'].includes('--platform android'));
  assert.ok(!bundle.files['src/main.rs'].includes('mod actions;'));
});

test('buildNativeRustExport generates typed bindings and preserves the application handlers', () => {
  const before = structuredClone(typedProject);
  const bundle = buildNativeRustExport(typedProject, 'ui/Counter.ui', {
    crateName: 'Counter App',
    formaDependencyPath: '/tmp/forma/vector-ui',
  });
  assert.deepEqual(typedProject, before, 'export must not mutate source project files');
  assert.equal(bundle.name, 'counter-app');
  assert.equal(bundle.files['src/app.rs'], typedProject['src/app.rs']);
  assert.ok(bundle.files['Cargo.toml'].includes('forma = { path = "/tmp/forma/vector-ui" }'));
  assert.ok(bundle.files['Cargo.toml'].includes('serde = "1"'), 'user dependencies must survive export');
  assert.ok(bundle.files['Cargo.toml'].includes('version = "0.2.0"'));
  assert.ok(bundle.files['Cargo.toml'].includes('extra = []'));
  assert.ok(bundle.files['Cargo.toml'].includes('native = ["forma/native"]'));
  for (const section of ['package', 'features', 'dependencies']) {
    assert.equal(bundle.files['Cargo.toml'].match(new RegExp(`^\\[${section}\\]`, 'gm'))?.length, 1);
  }
  assert.ok(!bundle.files['src/ui_bundle.rs']);
  assert.ok(bundle.files['src/forma_generated.rs'].includes('pub struct Counter'));
  assert.ok(bundle.files['src/forma_generated.rs'].includes('impl ::forma::native_window::NativeApp for Counter'));
  assert.ok(bundle.files['src/forma_generated.rs'].includes('Counter::on_dirty(self, callback)'));
  assert.ok(bundle.files['src/main.rs'].includes('pub use app::*;'));
  assert.ok(bundle.files['src/main.rs'].includes('let mut form = app::create_form()?;'));
  assert.ok(bundle.files['src/main.rs'].includes('run('));
  assert.ok(bundle.files['README.md'].includes('create_form()'));
});

test('buildNativeRustExport requires an application factory for typed forms', () => {
  const files = {...typedProject};
  delete files['src/app.rs'];
  assert.throws(() => buildNativeRustExport(files, 'ui/Counter.ui'), /src\/app\.rs.*create_form/);
});

test('buildNativeRustExport rejects projects without .ui files', () => {
  assert.throws(() => buildNativeRustExport({'README.md': 'hello'}), /нет \.ui файлов/);
});

const cargo = spawnSync('cargo', ['--version'], {encoding: 'utf8'});
const rustOptions = cargo.status === 0 ? {} : {skip: 'Cargo is not installed'};

function compileBundle(bundle, targetDir) {
  const directory = mkdtempSync(join(tmpdir(), `${bundle.name}-`));
  for (const [path, content] of Object.entries(bundle.files)) {
    const output = join(directory, path);
    mkdirSync(dirname(output), {recursive: true});
    writeFileSync(output, content);
  }
  const manifest = join(directory, 'Cargo.toml');
  const env = {...process.env, CARGO_TARGET_DIR: targetDir};
  const test = spawnSync('cargo', ['test', '--offline', '--manifest-path', manifest], {
    encoding: 'utf8', timeout: 240_000, maxBuffer: 8 * 1024 * 1024, env,
  });
  assert.equal(test.status, 0, test.stderr + test.stdout);
  const native = spawnSync('cargo', ['check', '--offline', '--features', 'native', '--manifest-path', manifest], {
    encoding: 'utf8', timeout: 240_000, maxBuffer: 8 * 1024 * 1024, env,
  });
  assert.equal(native.status, 0, native.stderr + native.stdout);
}

test('exported Runtime, starter, and typed crates compile, test handlers, and check the native window', rustOptions, () => {
  const targetDir = join(mkdtempSync(join(tmpdir(), 'forma-export-target-')), 'target');
  const formaPath = resolve('vector-ui');
  compileBundle(buildNativeRustExport(runtimeProject, 'ui/SearchWindow.ui', {formaPath}), targetDir);
  compileBundle(buildNativeRustExport(sampleProject, 'ui/SearchWindow.ui', {formaPath}), targetDir);
  compileBundle(buildNativeRustExport(typedProject, 'ui/Counter.ui', {formaPath}), targetDir);
});
