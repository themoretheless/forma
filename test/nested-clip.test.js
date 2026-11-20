import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { compileComponents } from '../src/compiler/components.js';

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

// A 100 x 60 scroll area below a title holds a 200-high column of five 40-high rows.
const scrollArea = {
  'ui/App.ui': `component App { Column { width: 200; height: 200; Text { key: 'title'; text: 't'; width: 80; height: 30; } Scroll { width: 100; height: 60; Column { width: 100; height: 200; ${[0, 1, 2, 3, 4].map(i => `Text { key: 'r${i}'; text: 'r${i}'; width: 80; height: 40; background: #ff0000; }`).join(' ')} } } } }`,
};

test('a Scroll below the root becomes a scroll area around its controls', () => {
  const out = compileComponents(scrollArea, 'ui/App.ui');
  assert.deepEqual(out.controlScrolls[0], [], 'the title is outside it');
  const [area] = out.controlScrolls[1];
  assert.deepEqual(area, { area: 0, viewport: [50, 30, 100, 60], content: [100, 200] });
  for (const chain of out.controlScrolls.slice(1)) assert.deepEqual(chain, [area]);
  assert.deepEqual(out.controlClips[1], area.viewport, 'and clips them to its viewport');
});

test(
  'the runtime scrolls a Studio scroll area under the pointer',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const out = compileComponents(scrollArea, 'ui/App.ui');
    const model = new runtime.Runtime();
    try {
      model.load_component(out.source, out.template);
      out.controlClips.forEach((clip, i) => clip && model.set_control_clip(i, ...clip));
      model.set_control_scroll_areas(JSON.stringify(out.controlScrolls));
      const [x, y] = [60, 40];
      assert.equal(model.hit_index(x, y), 1, 'first row');
      model.pointer(x, y, 0);
      model.scroll(0, 50);
      assert.deepEqual([...model.scroll_offset()], [0, 0], 'the root stays');
      assert.equal(model.hit_index(x, y), 2, 'second row under the pointer');
      assert.throws(() => model.set_control_scroll_areas('[]'));
    } finally {
      model.free();
    }
  },
);

const panel = {
  'ui/App.ui': `component App { Column { width: 200; height: 200; Column { key: 'panel'; width: 120; height: 100; clicked -> actions.panel(); Text { key: 'label'; text: 'a'; width: 60; height: 20; } } } }`,
};

test('a container clicked handler becomes a pressable area under its children', () => {
  const out = compileComponents(panel, 'ui/App.ui');
  assert.deepEqual(
    out.previewControls.map(c => [c.type, c.props.key]),
    [
      ['ContainerArea', 'panel'],
      ['Text', 'label'],
    ],
  );
  assert.deepEqual(out.previewControls[0].props.width, 120);
  assert.equal(out.previewControls[1].events.clicked, out.previewControls[0].events.clicked);
});

test(
  'the runtime answers clicks on the container and through its Text',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const out = compileComponents(panel, 'ui/App.ui');
    const model = new runtime.Runtime();
    try {
      model.load_component(out.source, out.template);
      assert.equal(model.control_action(0), 'actions.panel');
      const click = (x, y) => {
        const before = model.control_clicks(0);
        model.pointer(x, y, 0);
        model.pointer(x, y, 1);
        model.pointer(x, y, 2);
        return model.control_clicks(0) - before;
      };
      assert.equal(click(45, 90), 1, 'empty space of the panel');
      assert.equal(click(75, 5), 1, 'through the Text');
      assert.equal(click(190, 5), 0, 'outside the panel');
    } finally {
      model.free();
    }
  },
);

test('a capturing container lays its area over its children', () => {
  const scene = capture => ({
    'ui/App.ui': `component App { Column { width: 200; height: 200; Column { key: 'panel'; width: 120; height: 100; ${capture} clicked -> actions.panel(); TextInput { key: 'own'; width: 100; height: 30; clicked -> actions.own(); } } } }`,
  });
  const keys = out => out.previewControls.map(c => c.props.key);
  assert.deepEqual(keys(compileComponents(scene(''), 'ui/App.ui')), ['panel', 'own']);
  assert.deepEqual(keys(compileComponents(scene('capture: true;'), 'ui/App.ui')), ['own', 'panel']);
  assert.throws(
    () =>
      compileComponents(
        {
          'ui/App.ui': `component App { Column { width: 200; height: 200; Column { capture: true; Text { text: 'a'; width: 50; height: 20; } } } }`,
        },
        'ui/App.ui',
      ),
    /capture требует clicked/,
  );
});

test(
  'arrow keys move through a Studio radio group in the runtime',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const model = new runtime.Runtime();
    try {
      // Two pressable controls stand in for radio buttons; the group comes from outside.
      const out = compileComponents(
        {
          'ui/App.ui': `component App { Column { width: 200; height: 200; TextInput { key: 'a'; width: 100; height: 30; } TextInput { key: 'b'; width: 100; height: 30; } } }`,
        },
        'ui/App.ui',
      );
      model.load_component(out.source, out.template);
      assert.ok(model.set_control_group(0, 'state.choice'));
      assert.ok(model.set_control_group(1, 'state.choice'));
      assert.ok(!model.set_control_group(2, 'state.choice'));
      assert.ok(model.focus_control(0));
      assert.ok(model.range_key('ArrowDown'));
      assert.equal(model.focused_index(), 1);
      assert.equal(model.control_clicks(1), 1, 'and chooses it');
    } finally {
      model.free();
    }
  },
);
