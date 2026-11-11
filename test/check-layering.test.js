import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, relative, resolve, sep } from 'node:path';
import { ALLOWED, findCycles, repositoryPackages, violations } from '../scripts/check-layering.mjs';
import { manifests } from '../scripts/rust-fmt.mjs';
import { root } from '../scripts/versions.mjs';

const pkg = (name, ...deps) => ({
  name,
  dependencies: deps.map(dep => ({ name: dep, path: `/repo/crates/${dep}` })),
});

test('an edge outside the table is reported, registry dependencies are ignored', () => {
  assert.deepEqual(violations([pkg('gpu-runtime', 'gpu-shader'), pkg('gpu-shader')]), []);
  assert.deepEqual(violations([pkg('gpu-shader', 'gpu-runtime')]), [
    'gpu-shader -> gpu-runtime is not an allowed dependency',
  ]);
  assert.deepEqual(violations([pkg('forma', 'compute-core')]), [
    'forma -> compute-core is not an allowed dependency',
  ]);
  assert.deepEqual(
    violations([{ name: 'gpu-shader', dependencies: [{ name: 'gpu-runtime', path: null }] }]),
    [],
  );
});

test('a crate missing from the table or a cycle in the table cannot slip in unnoticed', () => {
  assert.deepEqual(violations([pkg('brand-new')]), ['brand-new: not listed in the layering table']);
  assert.deepEqual(violations([], { a: ['b'], b: ['a'] }), ['cycle in allowed table: a -> b -> a']);
});

test('every tracked Cargo manifest in the repository is covered by the strict crate DAG', () => {
  assert.deepEqual(findCycles(ALLOWED), []);
  const packages = repositoryPackages();
  assert.deepEqual(violations(packages), []);
  const coveredManifests = new Set(
    packages.map(p => relative(root, p.manifest_path).split(sep).join('/')),
  );
  for (const manifest of manifests()) {
    assert.ok(coveredManifests.has(manifest), `${manifest} missing from check-layering.mjs`);
  }
});

test('Studio JS and Svet TS module graphs are strict DAGs and preserve boundary invariants', () => {
  const tracked = execFileSync('git', ['ls-files'], { cwd: root, encoding: 'utf8' })
    .split('\n')
    .filter(f => f && existsSync(resolve(root, f)));
  const trackedSet = new Set(tracked);
  const SPECS =
    /(?:^|[^\w.$])(?:import|export)[\s\S]{0,160}?\bfrom\s*['"]([^'"]+)['"]|(?:^|[^\w.$])import\s*\(\s*['"]([^'"]+)['"]/g;

  const resolveRel = (from, spec) => {
    if (!spec?.startsWith('.')) return null;
    const base = relative(root, resolve(root, dirname(from), spec))
      .split(sep)
      .join('/');
    if (trackedSet.has(base)) return base;
    for (const candidate of [
      `${base}.ts`,
      `${base}.js`,
      `${base}.mjs`,
      `${base}/index.ts`,
      `${base}/index.js`,
    ]) {
      if (trackedSet.has(candidate)) return candidate;
    }
    return null;
  };

  const buildGraph = files => {
    const set = new Set(files);
    const graph = {};
    for (const file of files) {
      const src = readFileSync(resolve(root, file), 'utf8');
      const targets = new Set();
      for (const m of src.matchAll(SPECS)) {
        const next = resolveRel(file, m[1] ?? m[2]);
        if (next && next !== file && set.has(next)) targets.add(next);
      }
      graph[file] = [...targets];
    }
    return graph;
  };

  const studioFiles = tracked.filter(
    f =>
      (f.startsWith('protocol/') ||
        f.startsWith('dev-server/') ||
        f.startsWith('mcp/') ||
        f.startsWith('src/') ||
        /^[^/]+\.(?:js|mjs)$/.test(f)) &&
      /\.(?:js|mjs)$/.test(f),
  );
  const studioGraph = buildGraph(studioFiles);
  assert.deepEqual(findCycles(studioGraph), [], 'Studio JS graph must have 0 import cycles');
  for (const file of studioFiles.filter(f => f.startsWith('protocol/'))) {
    const upward = studioGraph[file].filter(dep => !dep.startsWith('protocol/'));
    assert.deepEqual(upward, [], `${file} must not import outside protocol/`);
  }

  const svetFiles = tracked.filter(
    f => f.startsWith('packages/svet/src/') && f.endsWith('.ts') && !f.endsWith('.test.ts'),
  );
  const svetGraph = buildGraph(svetFiles);
  assert.deepEqual(findCycles(svetGraph), [], 'Svet TS graph must have 0 import cycles');
  for (const file of svetFiles.filter(f => f.startsWith('packages/svet/src/engine/'))) {
    const demoDeps = svetGraph[file].filter(dep => dep.startsWith('packages/svet/src/demo/'));
    assert.deepEqual(demoDeps, [], `${file} must not import demo modules`);
  }
});
