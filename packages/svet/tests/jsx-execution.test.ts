/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import ts from 'typescript';
import { parse } from '@babel/parser';
import { originalPositionFor, TraceMap } from '@jridgewell/trace-mapping';
import { createRoot, getGraphTelemetry, h, resetReactiveSystem, signal, SvetJsxCompileError, transformSvetJsx, transformSvetJsxWithMap } from '../src/engine';

beforeEach(() => { resetReactiveSystem(); document.body.innerHTML = ''; });

it('executes compiled JSX with adjacent children, styles, attributes and preserved events', () => {
  const source = `
    const count = signal(1), label = signal('ready'), color = signal('red');
    const node = <button onClick={() => count.update(value => value + 1)} title={label()} style={{ color: color() }}>{count()}{label()}</button>;
    return { node, count, label, color };
  `;
  const compiled = transformSvetJsx(source);
  const javascript = ts.transpileModule(compiled, {
    compilerOptions: { jsx: ts.JsxEmit.React, jsxFactory: 'h', target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
  }).outputText;
  let stop!: () => void;
  const app = createRoot(dispose => {
    stop = dispose;
    return new Function('h', 'signal', javascript)(h, signal);
  });
  document.body.append(app.node);
  expect(app.node.textContent).toBe('1ready');
  app.count.set(2); app.label.set('done'); app.color.set('blue');
  expect(app.node.textContent).toBe('2done');
  expect(app.node.title).toBe('done');
  expect(app.node.style.color).toBe('blue');
  app.node.click();
  expect(app.node.textContent).toBe('3done');
  stop();
  // All generated binding effects belong to the component's root.
  expect(getGraphTelemetry().nodes).toBe(3);
});

it('preserves lazy callback shape and transforms template substitutions and optional calls', () => {
  const source = 'const node = <Widget value={api?.read?.()} text={`Hello ${name()}`} render={(() => name()) as () => string} />;';
  const code = transformSvetJsx(source);
  expect(code).toContain('value={() => (api?.read?.())}');
  expect(code).toContain('text={() => (`Hello ${name()}`)}');
  expect(code).toContain('render={(() => name()) as () => string}');
  expect(() => parse(code, { plugins: ['typescript', 'jsx'] })).not.toThrow();
});

it('leaves spreads and constructor helpers intact while transforming nested JSX', () => {
  const source = 'const node = <div {...props()}>{For({each: rows, children: row => <span>{row.label()}</span>})}</div>;';
  const code = transformSvetJsx(source);
  expect(code).toContain('{...props()}');
  expect(code).toContain('{For(');
  expect(code).toContain('<span>{() => (row.label())}</span>');
  expect(transformSvetJsx(code)).toBe(code);
});

it('maps an original expression through inserted thunk code', () => {
  const source = 'const view = <p>\n  {count()}\n</p>;';
  const { code, map } = transformSvetJsxWithMap(source, '/app/Counter.tsx');
  const column = code.split('\n')[1].indexOf('count');
  expect(originalPositionFor(new TraceMap(map.toString()), { line: 2, column })).toMatchObject({
    source: '/app/Counter.tsx', line: 2, column: 3,
  });
  expect(map.sourcesContent).toEqual([source]);
});

it('reports malformed JSX with its source filename and position', () => {
  try {
    transformSvetJsxWithMap('const view = <p>\n  {count(}\n</p>;', '/app/Broken.tsx');
    throw new Error('expected compilation to fail');
  } catch (error) {
    expect(error).toBeInstanceOf(SvetJsxCompileError);
    expect(error).toMatchObject({ filename: '/app/Broken.tsx', line: 2 });
    expect((error as Error).message).toContain('/app/Broken.tsx:2:');
  }
});
