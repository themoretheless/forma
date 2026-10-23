import {test} from 'node:test';
import assert from 'node:assert/strict';
import {buildNativeRustExport} from '../src/native-export.js';
import {sampleProject} from '../src/sample-project.js';

test('buildNativeRustExport generates a standalone Rust crate from a .ui project', () => {
  const bundle = buildNativeRustExport(sampleProject, 'ui/SearchWindow.ui', {
    name: 'Knowledge Workspace',
  });
  assert.equal(bundle.name, 'knowledge-workspace');
  assert.equal(bundle.entry, 'ui/SearchWindow.ui');
  assert.ok(bundle.files['Cargo.toml'].includes('name = "knowledge-workspace"'));
  assert.ok(bundle.files['Cargo.toml'].includes('forma = { path = "../vector-ui" }'));
  assert.ok(bundle.files['src/ui_bundle.rs'].includes('pub const ENTRY_FILE: &str = "ui/SearchWindow.ui";'));
  assert.ok(bundle.files['src/ui_bundle.rs'].includes('include_str!("../ui/SearchWindow.ui")'));
  assert.ok(bundle.files['src/ui_bundle.rs'].includes('forma::Runtime::from_ui_files'));
  assert.ok(bundle.files['src/main.rs'].includes('mod ui_bundle;'));
  assert.ok(bundle.files['src/main.rs'].includes('mod actions;'));
  assert.ok(bundle.files['src/main.rs'].includes('runtime.sync_ui_bindings_native()'));
  assert.ok(bundle.files['README.md'].includes('cargo run --manifest-path ../vector-ui/Cargo.toml --features native -- ui/SearchWindow.ui'));
});

test('buildNativeRustExport rejects projects without .ui files', () => {
  assert.throws(() => buildNativeRustExport({'README.md': 'hello'}), /нет \.ui файлов/);
});
