import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import { dirname, relative, resolve, sep } from 'node:path';
import { root } from '../scripts/versions.mjs';

// A crate nothing compiles rots in silence, and so does a module nothing boots: deleting the last
// import of a designer file leaves its own unit tests green while the browser never loads it, and
// vite reports a clean build because the module simply is not in the bundle. So the shipped graph
// is walked from the things that actually execute, and every tracked src module must turn up.
const tracked = execFileSync('git', ['ls-files'], { cwd: root, encoding: 'utf8' })
  .split('\n')
  .filter(f => f && existsSync(resolve(root, f)));
const read = f => readFileSync(resolve(root, f), 'utf8');
const SPECS =
  /(?:^|[^\w.$])(?:import|export)\s*(?:[\w$*\s,]|\{[^{}]*\})*?\bfrom\s*['"]([^'"]+)['"]|(?:^|[^\w.$])import\s*\(\s*['"]([^'"]+)['"]/g;
// Resolved the way this repo imports: relative paths only, exact file name, no extension rewrite.
function target(from, spec) {
  if (!spec?.startsWith('.')) return null;
  const path = relative(root, resolve(root, dirname(from), spec))
    .split(sep)
    .join('/');
  return tracked.includes(path) ? path : null;
}
function edges(file) {
  return [...read(file).matchAll(SPECS)].map(m => target(file, m[1] ?? m[2])).filter(Boolean);
}
// Roots are the entry points the repo runs, not a guess about the studio: the html pages, the CLI
// scripts (generate-form.mjs builds the Rust forms), and the vite config with its dev-server runners.
export function entries() {
  const html = tracked
    .filter(f => f.endsWith('.html'))
    .flatMap(file =>
      [...read(file).matchAll(/<script[^>]*\ssrc="([^"?#]+)/g)].map(m =>
        tracked.includes(m[1].replace(/^\//, ''))
          ? m[1].replace(/^\//, '')
          : target(file, m[1].replace(/^\//, './')),
      ),
    );
  const commands = tracked.filter(
    f => /^scripts\/.*\.mjs$/.test(f) || /^\w[\w-]*-runner\.js$/.test(f) || f === 'vite.config.js',
  );
  return [...new Set([...html, ...commands].filter(Boolean))];
}
export function shipped(start = entries()) {
  const seen = new Set(start),
    stack = [...start];
  while (stack.length)
    for (const next of edges(stack.pop()))
      if (!seen.has(next)) {
        seen.add(next);
        stack.push(next);
      }
  return seen;
}
export function sources() {
  return tracked.filter(f => f.startsWith('src/') && f.endsWith('.js'));
}
export function orphans(start = entries()) {
  const reach = shipped(start);
  return sources().filter(file => !reach.has(file));
}
// The same walk, but collecting the bare specifiers rollup resolves into node_modules: a package
// nothing configures still ships, and its unit tests all stay green because nothing imports it.
export function packageName(spec) {
  const parts = spec.split('/');
  return spec.startsWith('@') ? parts.slice(0, 2).join('/') : parts[0];
}
export function packageSpecs(source) {
  return [
    ...new Set(
      [...source.matchAll(SPECS)]
        .map(m => m[1] ?? m[2])
        .filter(spec => spec && !spec.startsWith('.'))
        .map(packageName),
    ),
  ];
}
export function packages(start = entries()) {
  const seen = new Set(),
    out = new Set(),
    stack = [...start];
  while (stack.length) {
    const file = stack.pop();
    if (seen.has(file)) continue;
    seen.add(file);
    for (const spec of packageSpecs(read(file))) out.add(spec);
    for (const next of edges(file)) if (!seen.has(next)) stack.push(next);
  }
  return out;
}
test('every tracked src module is reachable from something that runs', () => {
  assert.ok(sources().length > 40, 'the walk has to see the whole studio, not an empty tree');
  assert.ok(entries().includes('src/main.js'), 'index.html boots the studio through src/main.js');
  assert.deepEqual(orphans(), [], `${orphans().join(', ')} is imported by nothing that executes`);
});
test('the check reads the graph rather than the file list', () => {
  // Cut the studio's own entry and the app below it falls out of the closure. A list of names
  // could never report this, and that is the difference between a graph walk and an inventory.
  const without = orphans(entries().filter(entry => entry !== 'src/main.js'));
  assert.ok(without.includes('src/main.js'), 'cutting the entry must orphan the app');
  assert.ok(without.length > 30, 'the modules hanging off that entry are the studio itself');
  // The closure must be the app's, not the suite's: unit tests import half of src directly, so a
  // graph that let them in would call every module reachable forever.
  assert.deepEqual(
    [...shipped()].filter(f => f.startsWith('test/')),
    [],
  );
  assert.ok(shipped().size > 40, 'the shipped graph is the studio, not a handful of files');
});
test('the shipped graph asks for no CodeMirror package nothing configures', () => {
  const specs = packageSpecs(`import {basicSetup} from 'codemirror';
import {linter} from '@codemirror/lint';
import {lineNumbers} from '@codemirror/view/dist/index.js';
import {EditorState} from "./state";
const glue=await import('./vector-pkg/forma.js');`);
  assert.deepEqual(
    specs.sort(),
    ['@codemirror/lint', '@codemirror/view', 'codemirror'],
    'bare names only, scoped names whole, subpaths folded',
  );
  const shipped = packages();
  // basicSetup hard-imports the lint keymap, so one 'codemirror' line ships a package whose two
  // bindings can only ever open an empty panel. The sizes are in PRODUCTION_READINESS.md.
  assert.equal(
    shipped.has('codemirror'),
    false,
    'the meta package drags the whole basicSetup back in',
  );
  assert.equal(
    shipped.has('@codemirror/lint'),
    false,
    'no linter is installed, so there is nothing for lint to panel or gutter',
  );
  assert.ok(
    shipped.has('@codemirror/search'),
    'search stays: Ctrl-F and Mod-D are features the editor uses',
  );
  assert.ok(
    shipped.has('@codemirror/view') && shipped.size > 8,
    'the walk has to read imports, not a list of names',
  );
});

export const STUDIO_LAYERS = {
  shared: [],
  compiler: ['shared'],
  output: ['shared'],
  editor: ['shared', 'compiler'],
  inspector: ['shared', 'compiler'],
  canvas: ['shared', 'compiler'],
  preview: ['shared', 'compiler'],
  workspace: ['shared', 'compiler'],
  tree: ['shared', 'compiler', 'workspace'],
  shell: ['shared', 'compiler', 'workspace', 'preview'],
  app: [
    'shared',
    'compiler',
    'output',
    'editor',
    'inspector',
    'canvas',
    'preview',
    'workspace',
    'tree',
    'shell',
  ],
};

test('Studio src modules live in strict DAG feature folders and main.js stays a pure composition entry', () => {
  const topLevel = sources().filter(f => f.split('/').length === 2);
  assert.deepEqual(topLevel, ['src/main.js'], 'only src/main.js may live directly under src/');
  const mainSource = read('src/main.js');
  assert.doesNotMatch(mainSource, /<header/, 'src/main.js must not embed duplicate header HTML');
  assert.ok(
    mainSource.split('\n').length <= 240,
    `src/main.js grew too large (${mainSource.split('\n').length} lines)`,
  );

  const violations = [];
  const adj = new Map();
  for (const file of sources()) {
    const parts = file.split('/');
    if (parts.length < 3) continue;
    const layer = parts[1];
    if (!Object.hasOwn(STUDIO_LAYERS, layer)) {
      violations.push(`${file}: unknown feature folder '${layer}'`);
      continue;
    }
    const deps = edges(file).filter(dep => dep.startsWith('src/'));
    adj.set(file, deps);
    for (const dep of deps) {
      const targetParts = dep.split('/');
      if (targetParts.length < 3) {
        violations.push(`${file} -> ${dep}: feature module may not import top-level src entry`);
        continue;
      }
      const targetLayer = targetParts[1];
      if (targetLayer !== layer && !STUDIO_LAYERS[layer].includes(targetLayer)) {
        violations.push(`${file} (${layer}) -> ${dep} (${targetLayer}) violates layer order`);
      }
    }
  }
  assert.deepEqual(violations, [], violations.join('\n'));

  // Acyclic check across all src modules.
  const state = new Map();
  const stack = [];
  const cycles = [];
  const visit = node => {
    state.set(node, 1);
    stack.push(node);
    for (const next of adj.get(node) ?? []) {
      const s = state.get(next) ?? 0;
      if (s === 0) visit(next);
      else if (s === 1) cycles.push([...stack.slice(stack.indexOf(next)), next].join(' -> '));
    }
    stack.pop();
    state.set(node, 2);
  };
  for (const node of adj.keys()) {
    if ((state.get(node) ?? 0) === 0) visit(node);
  }
  assert.deepEqual(cycles, [], `Studio JS import cycle(s): ${cycles.join('; ')}`);
});

test('Studio JS layers maintain @ts-check coverage without regression', () => {
  const checked = new Set();
  for (const file of tracked) {
    if (!file.endsWith('.js')) continue;
    if (/^\/\/\s*@ts-check\b/m.test(read(file).slice(0, 512))) checked.add(file);
  }
  const required = tracked.filter(
    file =>
      file.endsWith('.js') &&
      (file.startsWith('protocol/') ||
        file.startsWith('dev-server/') ||
        file.startsWith('src/shared/') ||
        file.startsWith('src/output/') ||
        file.startsWith('src/workspace/') ||
        file.startsWith('src/app/') ||
        file === 'src/main.js' ||
        /^src\/[^/]+\/[^/]+-service\.js$/.test(file)),
  );
  const missing = required.filter(file => !checked.has(file));
  assert.deepEqual(
    missing,
    [],
    `Missing // @ts-check in required layer module(s): ${missing.join(', ')}`,
  );
  assert.ok(checked.size >= 119, `Expected >= 119 @ts-check modules, found ${checked.size}`);
});
