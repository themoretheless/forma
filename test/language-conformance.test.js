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
import { compileComponents, evaluate } from '../src/compiler/components.js';
import { parse } from '../src/compiler/language.js';
import { mapRuntimeErrorToUiDiagnostic } from '../src/compiler/diagnostics.js';
import * as svet from '../packages/svet/dist/package/index.js';

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
    assert.equal(actual.includes('!'), false, 'zero layout conformance divergences expected');
  },
);

test(
  'expressions and stdlib agree across Studio, Rust runtime, and Svet',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const cases = [
      {
        name: 'arithmetic and ternary',
        expr: "2 + 3 * 4 > 10 ? 'big' : 'small'",
        state: {},
        expected: 'big',
      },
      {
        name: 'optional chaining and nullish coalesce on missing branch',
        expr: "state.user?.profile?.name ?? 'anon'",
        state: { user: null },
        expected: 'anon',
      },
      {
        name: 'optional chaining present',
        expr: "state.user?.profile?.name ?? 'anon'",
        state: { user: { profile: { name: 'Ada' } } },
        expected: 'Ada',
      },
      {
        name: 'interpolation and UTF-16 length',
        expr: '"n=${state.count},l=${len(state.emoji)}"',
        state: { count: 3, emoji: '😀' },
        expected: 'n=3,l=2',
      },
      {
        name: 'math and string stdlib',
        expr: '"${upper(trim(state.text))}:${round(clamp(state.val, 0, 10))}"',
        state: { text: '  hi ', val: 14.4 },
        expected: 'HI:10',
      },
      {
        name: 'coalesce helper',
        expr: "coalesce(state.missing, state.alt, 'ok')",
        state: { missing: null, alt: 'chosen' },
        expected: 'chosen',
      },
      {
        name: 'exclusive and inclusive ranges',
        expr: "\"${join(1..4, '-')}/${join(1..=4, '-')}\"",
        state: {},
        expected: '1-2-3/1-2-3-4',
      },
    ];

    for (const tc of cases) {
      const uiSource = `component ExprCheck { Column { Text { key: 'out'; width: 240; height: 28; text: ${tc.expr}; } } }`;
      const ast = parse(uiSource);
      const exprAst = ast.nodes[0].children[0].props.text;

      // 1. Studio evaluator
      const studioVal = String(evaluate(exprAst, {}, tc.state));
      assert.equal(studioVal, tc.expected, `Studio failed on ${tc.name}`);

      // 2. Rust WASM Runtime (.ui Document path)
      const instance = new runtime.Runtime();
      try {
        instance.load_ui_files_json(
          JSON.stringify({ 'ExprCheck.ui': uiSource }),
          'ExprCheck.ui',
          JSON.stringify(tc.state),
        );
        assert.equal(instance.control_label(0), tc.expected, `Rust failed on ${tc.name}`);
      } finally {
        instance.free();
      }

      // 3. Svet expression evaluator (string + canonical AST bridge)
      const stateSignals = Object.fromEntries(
        Object.entries(tc.state).map(([k, v]) => [k, () => v]),
      );
      const svetVal = String(
        svet.evaluateFormaExpression(tc.expr, {
          state: stateSignals,
          props: {},
          computedLets: {},
          tokens: {},
          i18n: () => ({}),
          isValid: () => true,
        }),
      );
      assert.equal(svetVal, tc.expected, `Svet string expr failed on ${tc.name}`);

      const canonicalString = svet.canonicalValueToExprString(exprAst);
      const bridgedVal = String(
        svet.evaluateFormaExpression(canonicalString, {
          state: stateSignals,
          props: {},
          computedLets: {},
          tokens: {},
          i18n: () => ({}),
          isValid: () => true,
        }),
      );
      assert.equal(bridgedVal, tc.expected, `Svet canonical AST bridge failed on ${tc.name}`);
    }
  },
);

test(
  'components, control flow, and diagnostics map consistently across Studio, Rust, and Svet',
  { skip: !existsSync(new URL('../public/vector-pkg/forma.js', import.meta.url)) },
  async () => {
    const runtime = await import('../public/vector-pkg/forma.js');
    await runtime.default({
      module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)),
    });
    const appUi = [
      'component App {',
      '  tokens { spacing.md: 12; }',
      "  i18n en { label: 'Items'; }",
      '  #[range(1, 10)]',
      '  prop limit: Number = 3;',
      '  state show: Bool = true;',
      "  state items: List = [{ id: 'a', name: 'One' }, { id: 'b', name: 'Two' }];",
      "  state query: String = 'seed';",
      '  let header = "${i18n.label}:${limit}:${tokens.spacing.md}";',
      '  Column {',
      '    width: 300; height: 220;',
      "    Text { key: 'head'; text: header; }",
      '    if state.show {',
      '      for item in state.items key item.id {',
      '        Text { text: item.name; }',
      '      } empty {',
      "        Text { key: 'empty'; text: 'none'; }",
      '      }',
      '    }',
      '    match len(state.items) {',
      "      2 => { Text { key: 'countPair'; text: 'pair'; } }",
      "      _ => { Text { key: 'countOther'; text: 'other'; } }",
      '    }',
      "    TextInput { key: 'editor'; value <-> state.query; }",
      '  }',
      '}',
    ].join('\n');

    // Studio compilation
    const compiled = compileComponents({ 'App.ui': appUi }, 'App.ui', {}, metrics);
    assert.equal(compiled.sourceMap.controls.length, 5);

    // Rust WASM runtime compilation
    const instance = new runtime.Runtime();
    try {
      instance.load_ui_files_json(JSON.stringify({ 'App.ui': appUi }), 'App.ui', '{}');
      assert.equal(instance.control_count(), 5);
      assert.equal(instance.control_label(0), 'Items:3:12');
      assert.equal(instance.control_label(1), 'One');
      assert.equal(instance.control_label(2), 'Two');
      assert.equal(instance.control_label(3), 'pair');
      assert.equal(instance.text_value(4), 'seed');
    } finally {
      instance.free();
    }

    // Svet canonical frontend registration & AOT validation
    svet.registerFormaFrontend(parse);
    try {
      const svetAst = svet.parseFormaSource(appUi);
      assert.equal(svetAst.componentName, 'App');
      assert.equal(svetAst.tokens.spacing.md, 12);
      assert.equal(svetAst.i18nDictionaries.en.label, 'Items');
      assert.equal(svetAst.letExpressions[0].name, 'header');
      assert.equal(svetAst.propConstraints[0].range[1], 10);
      assert.equal(svet.validateFormaSource(appUi).valid, true);
      const moduleJs = svet.compileFormaToJsModule(appUi);
      assert.match(moduleJs, /BridgeCard|App/);
    } finally {
      svet.registerFormaFrontend(null);
    }

    // Runtime-to-.ui error diagnostic mapping
    const badUi =
      "component Bad { Frame { width: 100; height: 100; Button { key: 'wide'; x: -12; width: 80; height: 30; } } }";
    const out = compileComponents(
      { 'components/Button.ui': button, 'ui/Bad.ui': badUi },
      'ui/Bad.ui',
      {},
      metrics,
    );
    const badRuntime = new runtime.Runtime();
    let runtimeErr = null;
    try {
      badRuntime.load(out.source, out.template);
    } catch (err) {
      runtimeErr = err;
    } finally {
      badRuntime.free();
    }
    assert.ok(runtimeErr, 'expected negative control x inside Frame to fail at runtime');
    const diagnostic = mapRuntimeErrorToUiDiagnostic(runtimeErr, out.sourceMap);
    assert.ok(diagnostic.diagnostic?.source);
    assert.equal(diagnostic.diagnostic.source.file, 'ui/Bad.ui');
    assert.equal(
      badUi.slice(diagnostic.diagnostic.source.from, diagnostic.diagnostic.source.to),
      '-12',
    );
    assert.equal(diagnostic.diagnostic.related?.file, 'components/Button.ui');
  },
);
