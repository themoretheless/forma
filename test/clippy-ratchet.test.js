import { test } from 'node:test';
import assert from 'node:assert/strict';
import { count, compare } from '../scripts/clippy-ratchet.mjs';

const message = (pkg, code, file, line) =>
  JSON.stringify({
    reason: 'compiler-message',
    package_id: `path+file:///repo/crates/${pkg}#0.1.0`,
    message: { code: { code }, spans: [{ file_name: file, line_start: line, column_start: 1 }] },
  });

test('a finding compiled into several targets is counted once', () => {
  const lines = [
    message('forma', 'clippy::needless_borrow', 'src/a.rs', 3),
    message('forma', 'clippy::needless_borrow', 'src/a.rs', 3),
    message('forma', 'clippy::needless_borrow', 'src/a.rs', 9),
    message('gpu-shader', 'clippy::let_and_return', 'src/b.rs', 1),
    'not json',
    JSON.stringify({ reason: 'build-finished', success: true }),
  ];
  assert.deepEqual(count(lines), {
    forma: { 'clippy::needless_borrow': 2 },
    'gpu-shader': { 'clippy::let_and_return': 1 },
  });
});

test('growth and shrinkage are both reported, each on its own side', () => {
  const baseline = { forma: { a: 2, b: 1 }, shader: { c: 1 } };
  assert.deepEqual(compare(baseline, baseline), { worse: [], better: [] });
  const { worse, better } = compare(baseline, { forma: { a: 3 }, shader: { c: 1, d: 1 } });
  assert.deepEqual(worse, ['forma: a 2 -> 3', 'shader: d 0 -> 1']);
  assert.deepEqual(better, ['forma: b 1 -> 0']);
});
