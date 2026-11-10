import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { compileComponents } from '../src/components.js';
import init, { Runtime, text_metrics } from '../public/vector-pkg/forma.js';

const project = family => ({
  'ui/App.ui': "component App { Frame { width: 420; height: 80; Code { key: 'code'; } } }",
  'components/Code.ui': `component Code { width: 400; height: 60; Rectangle { radius: 0; background: Brush { color: #101010; }; Frame { columns: [-, *]; Text { cell: 1 1; text: 'iiii\\nWWWW'; color: #ffffff; fontSize: 20; ${family} } } } }`,
});
const widths = template =>
  [...template.matchAll(/ContentText \{[^}]*?width: ([\d.]+);/g)].map(match => Number(match[1]));

test('Text.fontFamily selects the monospace face in metrics, scene and pixels', async () => {
  await init({
    module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
  });
  assert.deepEqual(
    Array.from(text_metrics('Forma', 16)),
    Array.from(text_metrics('Forma', 16, 'sans')),
  );
  assert.equal(text_metrics('iiii', 16, 'mono')[0], text_metrics('WWWW', 16, 'mono')[0]);
  assert.notEqual(text_metrics('iiii', 16)[0], text_metrics('WWWW', 16)[0]);

  const render = family => {
    const compiled = compileComponents(
      project(family),
      'ui/App.ui',
      {},
      { measureText: text_metrics },
    );
    const runtime = new Runtime();
    try {
      runtime.load_component(compiled.source, compiled.template);
      return { template: compiled.template, pixels: runtime.pixels(420, 80, 1) };
    } finally {
      runtime.free();
    }
  };
  const sans = render('');
  assert.ok(!sans.template.includes('fontFamily'));
  const [sansNarrow, sansWide] = widths(sans.template);
  assert.notEqual(sansNarrow, sansWide);

  const mono = render("fontFamily: 'mono';");
  assert.equal(mono.template.match(/fontFamily: mono;/g).length, 2);
  const [monoNarrow, monoWide] = widths(mono.template);
  assert.equal(monoNarrow, monoWide);
  assert.notDeepEqual(mono.pixels, sans.pixels);

  assert.throws(
    () =>
      compileComponents(
        project("fontFamily: 'serif';"),
        'ui/App.ui',
        {},
        { measureText: text_metrics },
      ),
    /fontFamily/,
  );
});
