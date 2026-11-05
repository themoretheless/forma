/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import { compileFormaAst, getGraphTelemetry, parseFormaSource, resetReactiveSystem } from '../src/engine';
beforeEach(() => resetReactiveSystem());
const source = `component App { #[range(0,10)] prop limit: Number = 5; state count: Number = 1; Button { text:state.count; clicked -> state.increment('count',1); } }`;
const changes: Array<[string, (ast: any) => void]> = [
  ['reversed range', a => a.propConstraints[0].range = [10,0]],
  ['infinite range', a => a.propConstraints[0].range = [0,Infinity]],
  ['sparse range', a => a.propConstraints[0].range = new Array(2)],
  ['extra range bound', a => a.propConstraints[0].range = [0,10,20]],
  ['string range bound', a => a.propConstraints[0].range = ['0',10]],
  ['invalid regex', a => a.propConstraints[0].patternSource = '['],
  ['unsafe regex', a => a.propConstraints[0].patternSource = '(a+)+$'],
  ['non-string regex', a => a.propConstraints[0].patternSource = 12],
  ['duplicate prop', a => a.propConstraints.push({...a.propConstraints[0]})],
];
for (const [name, change] of changes) {
  it(`refuses ${name} before construction and before HMR mutation`, () => {
    const ast = parseFormaSource(source); change(ast);
    expect(() => compileFormaAst(ast)).toThrow(/AST.*contract/i);
    expect(getGraphTelemetry().nodes).toBe(0);
    const inst = compileFormaAst(parseFormaSource(source)), container = document.createElement('div'), handle = inst.mount(container);
    const old = container.querySelector('button')!, baseline = getGraphTelemetry().nodes;
    try {
      expect(() => inst.hotReload(ast)).toThrow(/AST.*contract/i);
      expect(container.querySelector('button')).toBe(old); expect(inst.isValid()).toBe(true);
      expect(getGraphTelemetry().nodes).toBe(baseline); old.click(); expect(inst.state.count()).toBe(2);
      inst.hotReload(parseFormaSource(source)); expect(container.textContent).toBe('2');
    } finally { handle.unmount(); inst.dispose(); }
    expect(getGraphTelemetry().nodes).toBe(0);
  });
}

it('rejects duplicate prop names consistently in source parser and validation', async () => {
  const { validateFormaSource } = await import('../src/engine');
  const invalid = `component App { prop x: Number = 1; prop x: Number = 2; Column {} }`;
  expect(() => parseFormaSource(invalid)).toThrow(/duplicate prop/i);
  expect(validateFormaSource(invalid).valid).toBe(false);
});
