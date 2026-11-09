/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  compileFormaAst,
  getGraphTelemetry,
  parseFormaSource,
  resetReactiveSystem,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
const source = `component App { #[range(0,10)] prop limit: Number = 5; state count: Number = 1; let doubled = state.count * 2; shortcut 'Mod+X' -> state.increment('count',1); Column { Text { text:doubled; } } }`;
for (const phase of ['construction', 'HMR'])
  for (const mutation of ['tree', 'let', 'range', 'cycle', 'shortcut'])
    it(`owns ${mutation} snapshot after ${phase} admission`, () => {
      const ast = parseFormaSource(source),
        inst = compileFormaAst(phase === 'construction' ? ast : parseFormaSource(source));
      if (phase === 'HMR') inst.hotReload(ast);
      if (mutation === 'tree') ast.rootAstNodes[0].children[0].properties.text = "'tampered'";
      if (mutation === 'let') ast.letExpressions[0].expr = 'state.count * 100';
      if (mutation === 'range') ast.propConstraints[0].range![1] = 1;
      if (mutation === 'cycle') ast.rootAstNodes[0].children.push(ast.rootAstNodes[0]);
      if (mutation === 'shortcut') ast.shortcuts[0].action = "state.increment('count',100)";
      const container = document.createElement('div'),
        handle = inst.mount(container);
      try {
        expect(container.textContent).toBe('2');
        inst.state.count.set(2);
        expect(container.textContent).toBe('4');
        inst.props.limit.set(6);
        expect(inst.isValid()).toBe(true);
        window.dispatchEvent(new KeyboardEvent('keydown', { key: 'x', ctrlKey: true }));
        expect(inst.state.count()).toBe(3);
        expect(container.textContent).toBe('6');
      } finally {
        handle.unmount();
        inst.dispose();
      }
      expect(getGraphTelemetry().nodes).toBe(0);
    });
it('isolates object default values from later caller AST edits', () => {
  const ast = parseFormaSource(`component App { prop data: Any = null; Column {} }`);
  ast.propConstraints[0].defaultValue = { name: 'seed' };
  const inst = compileFormaAst(ast);
  try {
    ast.propConstraints[0].defaultValue.name = 'tampered';
    expect(inst.props.data()).toEqual({ name: 'seed' });
  } finally {
    inst.dispose();
  }
  expect(getGraphTelemetry().nodes).toBe(0);
});
it('preserves an explicit undefined state field while detaching the state default map', () => {
  const ast = parseFormaSource(`component App { Column {} }`);
  ast.initialStateValues.maybe = undefined;
  const inst = compileFormaAst(ast);
  try {
    expect(Object.hasOwn(inst.state, 'maybe')).toBe(true);
    expect(inst.state.maybe()).toBeUndefined();
    ast.initialStateValues.maybe = 'tampered';
    inst.actions.reset('maybe');
    expect(inst.state.maybe()).toBeUndefined();
  } finally {
    inst.dispose();
  }
  expect(getGraphTelemetry().nodes).toBe(0);
});
