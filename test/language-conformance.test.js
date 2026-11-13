// Compares the two implementations of `.ui` layout properties on the same values.
//
// The Studio compiler lays out a root Column itself and validates its properties in JavaScript.
// A root Frame without a grid is handed to the Rust runtime, which validates them again by its
// own rules. Until there is one source of truth the differences are recorded here, so that a new
// one is noticed and a fixed one is removed from the record. Run with FORMA_BLESS=1 to record an
// intended change.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { compileComponents } from '../src/components.js';

const button = `component Button { width: 200; height: 50; text: 'Base';
 Rectangle { Frame { ContentPresenter { key: 'content'; Text { text: props.text; } } } PointerArea { clicked -> events.clicked(); } } }`;
const project = (root, property) => ({
  'components/Button.ui': button,
  'ui/Demo.ui': `component Demo { ${root} { ${property} Button { } Button { } } }`,
});
const metrics = { measureText: () => [80, 20] };

const properties = [
  'padding: 0;',
  'padding: 8;',
  'padding: 8 16;',
  'padding: 1 2 3;',
  'padding: 1 2 3 4;',
  'padding: 1 2 3 4 5;',
  'padding: -1;',
  'padding: 5000;',
  "padding: 'wide';",
  'gap: 0;',
  'gap: 8;',
  'gap: 8 16;',
  'gap: 1 2 3;',
  'gap: -1;',
  'gap: 5000;',
  'width: 0;',
  'width: 360;',
  'width: 4096;',
  'width: 4097;',
  'width: 5000;',
  'width: -1;',
  'height: 4096;',
  'height: 4097;',
  'clip: true;',
  'clip: false;',
  'clip: 3;',
];

test(
  'layout property values are accepted alike by the compiler and the runtime, except as recorded',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const verdict = check => {
      try {
        check();
        return 'accepts';
      } catch {
        return 'rejects';
      }
    };
    const rows = properties.map(property => {
      const compiler = verdict(() =>
        compileComponents(project('Column', property), 'ui/Demo.ui', {}, metrics),
      );
      const native = verdict(() => {
        const out = compileComponents(project('Frame', property), 'ui/Demo.ui', {}, metrics);
        const instance = new runtime.Runtime();
        try {
          instance.load(out.source, out.template);
        } finally {
          instance.free();
        }
      });
      const mark = compiler === native ? ' ' : '!';
      return `${mark} ${property.padEnd(22)} compiler ${compiler}, runtime ${native}`;
    });
    const actual = rows.join('\n') + '\n';
    const golden = new URL('./golden/language-conformance.txt', import.meta.url);
    if (process.env.FORMA_BLESS) writeFileSync(golden, actual);
    assert.equal(actual, readFileSync(golden, 'utf8'));
  },
);
