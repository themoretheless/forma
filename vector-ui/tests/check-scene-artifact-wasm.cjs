const assert = require('node:assert/strict');
const { resolve } = require('node:path');
const { readFileSync, writeFileSync } = require('node:fs');
const { createHash } = require('node:crypto');
assert(process.argv[2] && process.argv[3], 'Expected generated Node WASM directory and report path');
const directory = resolve(process.argv[2]);
const { Runtime } = require(resolve(directory, 'forma.js'));
const source = 'component App { Column { width:360; height:400; Scroll { width:320; height:300; Column { width:100; height:512; } } } }';
const json = Runtime.compile_ui_scene_artifact(JSON.stringify({ 'App.ui': source }), 'App.ui', '{}');
const artifact = JSON.parse(json);
assert.equal(artifact.protocol, 'forma-scene-artifact/1');
assert.equal(artifact.controls.length, 0);
const runtime = new Runtime();
try {
  runtime.load_scene_artifact(json);
  runtime.scroll(0, 10000);
  assert.deepEqual(Array.from(runtime.scroll_offset()), [0, 212]);
  assert.deepEqual(Array.from(runtime.viewport()), [20, 50, 320, 300]);
  const revision = runtime.layout_revision();
  for (const invalid of ['{', json.replace('forma-scene-artifact/1', 'forma-scene-artifact/99'),
    JSON.stringify({ ...artifact, logical_content_end: [-1, 500] }),
    JSON.stringify({ ...artifact, controls: [{ node: 0, key: 'ghost', state: {}, event_args: {} }] })]) {
    assert.throws(() => runtime.load_scene_artifact(invalid));
    assert.equal(runtime.layout_revision(), revision);
    assert.deepEqual(Array.from(runtime.scroll_offset()), [0, 212]);
  }
  runtime.load_scene_artifact(json);
  assert.notEqual(runtime.layout_revision(), revision);
  const oversizedSource = source.replace('width:100; height:512;', 'width:512; height:600;');
  const oversizedJson = Runtime.compile_ui_scene_artifact(JSON.stringify({ 'App.ui': oversizedSource }), 'App.ui', '{}');
  runtime.load_scene_artifact(oversizedJson);runtime.scroll(10000, 10000);
  assert.deepEqual(Array.from(runtime.scroll_offset()), [192, 300]);
  runtime.scroll(-10000, -10000);assert.deepEqual(Array.from(runtime.scroll_offset()), [0, 0]);
  const report = { status: 'passed', node: process.version, protocol: artifact.protocol,
    checks: ['Rust compile via JS static API', 'versioned JSON load', 'empty layout scroll extent',
      'viewport parity', 'malformed/version/extent/mapping refusals preserve runtime', 'reload changes layout revision', 'oversized layout reachable along both scroll axes'],
    scope: 'Generated wasm-bindgen NodeJS ABI, actual WASM execution; not browser UI/window qualification',
    wasmSha256: createHash('sha256').update(readFileSync(resolve(directory, 'forma_bg.wasm'))).digest('hex') };
  writeFileSync(resolve(process.argv[3]), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} finally { runtime.free(); }
