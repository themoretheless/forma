import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { compileComponents } from '../src/compiler/components.js';
import { sampleProject } from '../src/workspace/sample-project.js';

test('a state method handler reaches the runtime scene as an action name', () => {
  const out = compileComponents(sampleProject, 'ui/SearchWindow.ui');
  assert.doesNotMatch(out.source, /clicked -> state\./);
  assert.match(out.source, /clicked -> actions\.search\(\);/);
  assert.match(out.source, /clicked -> actions\.reset\(\);/);
  const handlers = out.previewControls.map(node => node.events?.clicked).filter(Boolean);
  assert.deepEqual(handlers.sort(), ['state.reset', 'state.search'], 'the source keeps its own');
});

test(
  'the starter project loads into the runtime and reports its handlers',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const out = compileComponents(sampleProject, 'ui/SearchWindow.ui');
    const model = new runtime.Runtime();
    try {
      model.load_component(out.source, out.template);
      const actions = out.previewControls.map((_, index) => model.control_action(index));
      assert.ok(actions.includes('actions.search'));
      assert.ok(actions.includes('actions.reset'));
    } finally {
      model.free();
    }
  },
);
