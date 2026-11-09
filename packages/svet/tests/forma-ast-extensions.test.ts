/** @vitest-environment jsdom */
import { beforeEach, expect, it } from 'vitest';
import {
  compileFormaAst,
  getGraphTelemetry,
  parseFormaSource,
  resetReactiveSystem,
} from '../src/engine';
beforeEach(() => resetReactiveSystem());
const source = `component App { prop limit: Number = 5; state count: Number = 1; let doubled = state.count * 2; shortcut 'Mod+X' -> state.increment('count',1); Column { Text { text:doubled; } } }`;
for (const phase of ['construction', 'HMR'])
  for (const field of ['document', 'node', 'prop', 'let', 'shortcut', 'arm'])
    it(`ignores unknown ${field} getters during ${phase} snapshot`, () => {
      const ast = parseFormaSource(source);
      let reads = 0;
      const arm = { pattern: '_', propertyAssignments: {}, childNodes: [] };
      ast.rootAstNodes[0].matchArms = [arm];
      const target =
        field === 'document'
          ? ast
          : field === 'node'
            ? ast.rootAstNodes[0]
            : field === 'prop'
              ? ast.propConstraints[0]
              : field === 'let'
                ? ast.letExpressions[0]
                : field === 'shortcut'
                  ? ast.shortcuts[0]
                  : arm;
      Object.defineProperty(target, 'unusedExtension', {
        enumerable: true,
        get() {
          reads++;
          throw Error('unknown extension must not run');
        },
      });
      let inst: ReturnType<typeof compileFormaAst> | undefined;
      try {
        expect(() => {
          inst = compileFormaAst(phase === 'construction' ? ast : parseFormaSource(source));
          if (phase === 'HMR') inst.hotReload(ast);
        }).not.toThrow();
        const container = document.createElement('div'),
          handle = inst!.mount(container);
        try {
          expect(container.textContent).toBe('2');
          inst!.state.count.set(2);
          expect(container.textContent).toBe('4');
          expect(reads).toBe(0);
        } finally {
          handle.unmount();
        }
      } finally {
        inst?.dispose();
      }
      expect(getGraphTelemetry().nodes).toBe(0);
    });
