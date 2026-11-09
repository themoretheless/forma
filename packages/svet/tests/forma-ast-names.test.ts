/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  compileFormaAst,
  getGraphTelemetry,
  parseFormaSource,
  resetReactiveSystem,
  validateFormaSource,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
const source = `component App { state count: Number = 1; Text { text:state.count; } }`;
for (const name of ['__proto__', 'constructor', 'prototype'])
  for (const field of ['state', 'let'])
    it(`rejects reserved ${field} name ${name} before allocation/HMR`, () => {
      const bad = parseFormaSource(source);
      if (field === 'state')
        Object.defineProperty(bad.initialStateValues, name, { value: 2, enumerable: true });
      else bad.letExpressions.push({ name, expr: '2' });
      expect(() => compileFormaAst(bad)).toThrow(/AST.*document/i);
      expect(getGraphTelemetry().nodes).toBe(0);
      const inst = compileFormaAst(parseFormaSource(source)),
        container = document.createElement('div'),
        handle = inst.mount(container),
        nodes = getGraphTelemetry().nodes;
      try {
        expect(() => inst.hotReload(bad)).toThrow(/AST.*document/i);
        expect(getGraphTelemetry().nodes).toBe(nodes);
        inst.state.count.set(2);
        expect(container.textContent).toBe('2');
      } finally {
        handle.unmount();
        inst.dispose();
      }
      expect(getGraphTelemetry().nodes).toBe(0);
    });
it('rejects duplicate let declarations in AST and source before allocating overwritten handles', () => {
  const invalid = `component App { let value = 1; let value = 2; Column {} }`;
  expect(() => parseFormaSource(invalid)).toThrow(/duplicate let/i);
  expect(validateFormaSource(invalid).valid).toBe(false);
  const ast = parseFormaSource(source);
  ast.letExpressions = [
    { name: 'x', expr: '1' },
    { name: 'x', expr: '2' },
  ];
  expect(() => compileFormaAst(ast)).toThrow(/duplicate let/i);
  expect(getGraphTelemetry().nodes).toBe(0);
});
for (const name of ['toString', 'valueOf', 'hasOwnProperty'])
  it(`supports valid own state/prop/let name ${name} with complete teardown`, () => {
    const ast = parseFormaSource(
      `component App { state ${name}: Number = 1; prop ${name}: Number = 2; let ${name} = 3; Text { text:state.${name}; } }`,
    );
    const inst = compileFormaAst(ast),
      container = document.createElement('div'),
      handle = inst.mount(container);
    try {
      expect(container.textContent).toBe('1');
      expect(inst.props[name]()).toBe(2);
      expect(inst.computedLets[name]()).toBe(3);
      inst.state[name].set(4);
      expect(container.textContent).toBe('4');
    } finally {
      handle.unmount();
      inst.dispose();
    }
    expect(getGraphTelemetry().nodes).toBe(0);
  });
for (const name of ['toString', 'valueOf', 'hasOwnProperty'])
  it(`resets an own state name ${name} introduced by HMR to its declared default`, () => {
    const inst = compileFormaAst(parseFormaSource(source));
    try {
      inst.hotReload(`component App { state ${name}: Number = 7; Text { text:state.${name}; } }`);
      inst.state[name].set(9);
      inst.actions.reset(name);
      expect(inst.state[name]()).toBe(7);
    } finally {
      inst.dispose();
    }
    expect(getGraphTelemetry().nodes).toBe(0);
  });
