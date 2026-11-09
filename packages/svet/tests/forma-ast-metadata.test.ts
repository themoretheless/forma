/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  compileFormaAst,
  getGraphTelemetry,
  parseFormaSource,
  resetReactiveSystem,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
const source = `component App { state count: Number = 1; Text { text:state.count; } }`;
for (const [name, change] of [
  ['component name', (a: any) => (a.componentName = 12)],
  ['tokens', (a: any) => (a.tokens = [])],
  ['state map', (a: any) => (a.initialStateValues = null)],
  ['styles', (a: any) => (a.styles.main = { color: 12 })],
  ['i18n', (a: any) => (a.i18nDictionaries.ru = { hello: 12 })],
  ['lets', (a: any) => (a.letExpressions = [{ name: 'x', expr: 12 }])],
  ['sparse lets', (a: any) => (a.letExpressions = new Array(1))],
  ['shortcuts', (a: any) => (a.shortcuts = [{ combo: 'Mod+X', action: 12 }])],
] as const)
  it(`refuses malformed AST ${name} without graph or document mutation`, () => {
    const bad = parseFormaSource(source);
    change(bad);
    expect(() => compileFormaAst(bad)).toThrow(/AST.*document/i);
    expect(getGraphTelemetry().nodes).toBe(0);
    const inst = compileFormaAst(parseFormaSource(source)),
      container = document.createElement('div'),
      handle = inst.mount(container);
    const old = container.firstChild,
      nodes = getGraphTelemetry().nodes;
    try {
      expect(() => inst.hotReload(bad)).toThrow(/AST.*document/i);
      expect(container.firstChild).toBe(old);
      expect(getGraphTelemetry().nodes).toBe(nodes);
      inst.state.count.set(2);
      expect(container.textContent).toBe('2');
    } finally {
      handle.unmount();
      inst.dispose();
    }
    expect(getGraphTelemetry().nodes).toBe(0);
  });
