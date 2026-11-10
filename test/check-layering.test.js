import { test } from 'node:test';
import assert from 'node:assert/strict';
import { violations } from '../scripts/check-layering.mjs';

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

test('a crate missing from the table cannot slip in unnoticed', () => {
  assert.deepEqual(violations([pkg('brand-new')]), ['brand-new: not listed in the layering table']);
});
