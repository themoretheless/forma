import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { compileComponents } from '../src/components.js';

// A 40-high clipping column holds two 30-high texts: the second overflows it by 20.
const scene = clip => ({
  'ui/App.ui': `component App { Column { width: 200; height: 200; Column { width: 100; height: 40; ${clip} Text { key: 'a'; text: 'a'; width: 80; height: 30; background: #ff0000; } Text { key: 'b'; text: 'b'; width: 80; height: 30; background: #ff0000; } } } }`,
});

test('a clipping container below the root gives each of its controls the same clip', () => {
  const open = compileComponents(scene(''), 'ui/App.ui');
  assert.deepEqual(open.controlClips, [null, null]);
  const clipped = compileComponents(scene('clip: true;'), 'ui/App.ui');
  const [first, second] = clipped.controlClips;
  assert.deepEqual(first, second);
  assert.deepEqual(first.slice(2), [100, 40]);
  assert.equal(clipped.source, open.source, 'the scene markup itself does not change');
});

test(
  'the runtime hides and ignores what the clip leaves out',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const out = compileComponents(scene('clip: true;'), 'ui/App.ui');
    const model = new runtime.Runtime();
    try {
      model.load_component(out.source, out.template);
      const [x, top, , height] = out.controlClips[1];
      const bottom = top + height;
      const px = x + 10;
      assert.equal(model.hit_index(px, bottom + 10), 1);
      out.controlClips.forEach((clip, i) => assert.ok(model.set_control_clip(i, ...clip)));
      assert.equal(model.hit_index(px, bottom + 10), -1);
      assert.equal(model.hit_index(px, bottom - 2), 1);
    } finally {
      model.free();
    }
  },
);
