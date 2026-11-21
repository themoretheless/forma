import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const engineDir = resolve(dirname(fileURLToPath(import.meta.url)), '../src/engine');

const LAYER_ORDER = ['wasm', 'reactivity', 'dom', 'gpu', 'forma', 'facade'] as const;
type LayerName = (typeof LAYER_ORDER)[number];

function classifyLayer(file: string): LayerName {
  if (
    [
      'wasm-binary.ts',
      'wasm-bridge.ts',
      'wasm-embedded.ts',
      'stream-protocol.ts',
      'stream-codec.ts',
    ].includes(file)
  ) {
    return 'wasm';
  }
  if (file === 'reactivity.ts' || file.startsWith('reactivity-')) {
    return 'reactivity';
  }
  if (
    [
      'dom.ts',
      'jsx.ts',
      'jsx-runtime.ts',
      'slim-jsx-runtime.ts',
      'advanced.ts',
      'resumable.ts',
      'ssr-runtime.ts',
      'ssr-runtime-entry.ts',
      'server.ts',
      'stream.ts',
    ].includes(file) ||
    file.startsWith('dom-') ||
    file.startsWith('advanced-') ||
    file.startsWith('ssr-')
  ) {
    return 'dom';
  }
  if (file === 'gpu.ts' || file.startsWith('gpu-')) {
    return 'gpu';
  }
  if (file === 'forma.ts' || file.startsWith('forma-')) {
    return 'forma';
  }
  if (['compiler.ts', 'devtools.ts', 'api.ts', 'index.ts', 'slim.ts'].includes(file)) {
    return 'facade';
  }
  throw new Error(`Unclassified engine module: ${file}`);
}

function collectRelativeImports(file: string, text: string): string[] {
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const deps = new Set<string>();
  const addSpecifier = (specifier: ts.Expression | undefined) => {
    if (!specifier || !ts.isStringLiteral(specifier)) return;
    if (!specifier.text.startsWith('./')) return;
    const resolved = specifier.text.slice(2);
    deps.add(resolved);
  };
  const visit = (node: ts.Node) => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      addSpecifier(node.moduleSpecifier);
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
  return [...deps].sort();
}

function measureFunctionLines(file: string, fnName: string): number {
  const fullPath = join(engineDir, file);
  const text = readFileSync(fullPath, 'utf8');
  const sf = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  for (const statement of sf.statements) {
    if (ts.isFunctionDeclaration(statement) && statement.name?.text === fnName) {
      const start = sf.getLineAndCharacterOfPosition(statement.getStart(sf)).line;
      const end = sf.getLineAndCharacterOfPosition(statement.getEnd()).line;
      return end - start + 1;
    }
  }
  throw new Error(`Function ${fnName} not found in ${file}`);
}

describe('engine module layering and decomposition', () => {
  const files = readdirSync(engineDir)
    .filter(name => name.endsWith('.ts'))
    .sort();
  const graph = new Map<string, string[]>();
  for (const file of files) {
    const text = readFileSync(join(engineDir, file), 'utf8');
    graph.set(file, collectRelativeImports(file, text));
  }

  it('assigns every engine file to an explicit layer and forbids upward imports', () => {
    const violations: string[] = [];
    for (const file of files) {
      const fromLayer = LAYER_ORDER.indexOf(classifyLayer(file));
      for (const dep of graph.get(file) ?? []) {
        expect(files).toContain(dep);
        const toLayer = LAYER_ORDER.indexOf(classifyLayer(dep));
        if (fromLayer < toLayer) {
          violations.push(`${file} (${classifyLayer(file)}) -> ${dep} (${classifyLayer(dep)})`);
        }
      }
    }
    expect(violations).toEqual([]);
  });

  it('forms a strict DAG with zero circular imports across src/engine', () => {
    const visiting = new Set<string>();
    const visited = new Set<string>();
    const cycles: string[] = [];
    const dfs = (node: string, stack: string[]) => {
      visiting.add(node);
      for (const dep of graph.get(node) ?? []) {
        if (visiting.has(dep)) {
          const cycleStart = stack.indexOf(dep);
          cycles.push([...stack.slice(cycleStart), dep].join(' -> '));
        } else if (!visited.has(dep)) {
          dfs(dep, [...stack, dep]);
        }
      }
      visiting.delete(node);
      visited.add(node);
    };
    for (const file of files) {
      if (!visited.has(file)) dfs(file, [file]);
    }
    expect(cycles).toEqual([]);
  });

  it('keeps decomposed orchestrators within complexity bounds', () => {
    expect(measureFunctionLines('forma.ts', 'instantiateForma')).toBeLessThanOrEqual(220);
    expect(measureFunctionLines('gpu-scene3d.ts', 'createCanvas3D')).toBeLessThanOrEqual(240);
    expect(measureFunctionLines('reactivity-core.ts', 'computed')).toBeLessThanOrEqual(130);
    expect(measureFunctionLines('dom-core.ts', 'bindProp')).toBeLessThanOrEqual(100);
    expect(measureFunctionLines('dom-core.ts', 'insertChild')).toBeLessThanOrEqual(160);
    expect(measureFunctionLines('dom-model.ts', 'bindModel')).toBeLessThanOrEqual(130);
    expect(measureFunctionLines('dom-for.ts', 'For')).toBeLessThanOrEqual(160);
    expect(measureFunctionLines('advanced-resource.ts', 'createResource')).toBeLessThanOrEqual(130);
    expect(measureFunctionLines('advanced-router.ts', 'createRouter')).toBeLessThanOrEqual(150);
    expect(measureFunctionLines('forma-expr-parser.ts', 'evalAstNode')).toBeLessThanOrEqual(140);
    expect(measureFunctionLines('forma-reactive.ts', 'createForm')).toBeLessThanOrEqual(130);
    expect(measureFunctionLines('ssr-runtime.ts', 'createSsrRuntime')).toBeLessThanOrEqual(280);
    const gpuLines = readFileSync(join(engineDir, 'gpu.ts'), 'utf8').split('\n').length;
    expect(gpuLines).toBeLessThanOrEqual(800);
  });
});
