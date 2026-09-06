import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {validateRuns, renderComparison} from '../scripts/compare-bench.mjs';

const sha256 = text => createHash('sha256').update(text).digest('hex');

// Entirely in-memory completed runs: no local .forma artifacts, GPU or shell.
function runFixture(repeats = 3) {
  const metadata = {
    createdAt: '2026-01-01T00:00:00.000Z', revision: 'before-revision', dirty: false,
    frames: 20, repeats, platform: 'darwin', osRelease: 'test-os', arch: 'arm64',
    cpu: 'Synthetic test CPU', logicalCpus: 8, systemRamBytes: 16 * 2 ** 30,
    rust: 'rustc test-version',
  };
  const results = [];
  for (const [backend, mode] of [['gpu', 'animation'], ['cpu', 'animation'], ['gpu', 'idle']]) {
    for (let repetition = 1; repetition <= repeats; repetition++) {
      const idle = mode === 'idle';
      results.push({
        scene: 'image', backend, mode, repetition, width: 800, height: 400, scale: 2,
        adapter: backend === 'gpu' ? 'Synthetic GPU' : 'CPU retained XRGB',
        frames: idle ? 0 : metadata.frames,
        render_throughput_fps: idle ? 0 : repetition * 100,
        completed_ms: {p95: idle ? 0 : repetition * 4},
        cpu_submit_ms: {mean: idle ? 0 : repetition * 2},
        gpu_pass: backend === 'gpu' && !idle ? {mean_ms: repetition * 2} : null,
        cpu_percent_one_core: idle ? 0 : repetition * 25,
        rss_after_bytes: 32 * 2 ** 20,
        owned_gpu_buffer_bytes: backend === 'gpu' ? 4096 : 0,
        rust_allocations: idle ? 0 : repetition * 20,
        rust_reallocations: idle ? 0 : repetition * 10,
        rust_requested_bytes: idle ? 0 : repetition * 3000,
      });
    }
  }
  return {
    directory: '/synthetic/before', metadata, results,
    fixtures: {
      'image.ui': sha256('component Demo { Frame {} }'),
      'image.template.ui': sha256('component Button { Rectangle {} }'),
    },
  };
}

function pair(repeats) {
  const before = runFixture(repeats);
  const after = structuredClone(before);
  after.directory = '/synthetic/after';
  after.metadata.createdAt = '2026-01-02T00:00:00.000Z';
  after.metadata.revision = 'after-revision';
  after.metadata.dirty = true;
  return {before, after};
}

function caseRows(report, section, match = 'image / gpu / animation') {
  const body = report.split(`## ${section}\n\n`)[1]?.split('\n## ')[0];
  assert.ok(body, `missing report section ${section}`);
  return body.split('\n').filter(line => line.startsWith(`| ${match} /`));
}

test('completed matching runs accept different source revisions and preserve inputs', () => {
  const {before, after} = pair();
  const original = structuredClone({before, after});
  after.results.reverse(); // Ordering is not part of comparability.
  original.after.results.reverse();
  const groups = validateRuns(before, after);
  assert.equal(groups.beforeGroups.size, 3);
  assert.equal(groups.afterGroups.size, 3);
  renderComparison(before, after);
  assert.deepEqual({before, after}, original);
});

test('all comparable machine, compiler and sample metadata must match', () => {
  for (const field of ['frames', 'repeats', 'platform', 'osRelease', 'arch', 'cpu',
    'logicalCpus', 'systemRamBytes', 'rust']) {
    const {before, after} = pair();
    after.metadata[field] = typeof after.metadata[field] === 'number'
      ? after.metadata[field] + 1 : `${after.metadata[field]}-different`;
    assert.throws(() => validateRuns(before, after), new RegExp(`metadata\\.${field} differs`), field);
  }
  const {before, after} = pair();
  delete after.metadata.cpu;
  assert.throws(() => validateRuns(before, after), /invalid metadata\.cpu/);
});

test('fixture SHA-256 content changes and different fixture sets reject comparisons', () => {
  const {before, after} = pair();
  after.fixtures['image.ui'] = sha256('different markup');
  assert.throws(() => validateRuns(before, after), /different SHA-256\/content/);
  after.fixtures = {...before.fixtures, 'extra.ui': sha256('extra')};
  assert.throws(() => validateRuns(before, after), /fixture file sets differ/);
  after.fixtures = {...before.fixtures};
  delete after.fixtures['image.template.ui'];
  assert.throws(() => validateRuns(before, after), /fixture file sets differ/);
  delete before.fixtures['image.template.ui'];
  assert.throws(() => validateRuns(before, after), /missing source\/template fixture/);
});

test('case mode, target, DPI and adapter identity must match', () => {
  for (const [field, value] of [['mode', 'resize'], ['width', 900], ['height', 500], ['scale', 1]]) {
    const {before, after} = pair();
    for (const row of after.results) if (row.backend === 'gpu' && row.mode === 'animation') row[field] = value;
    assert.throws(() => validateRuns(before, after), /case sets differ/, field);
  }
  const {before, after} = pair();
  for (const row of after.results) if (row.backend === 'gpu') row.adapter = 'Different GPU';
  assert.throws(() => validateRuns(before, after), /adapter differs/);
});

test('missing, duplicate and out-of-range repetitions cannot be pooled silently', () => {
  for (const [mutate, error] of [
    [run => run.results.pop(), /incomplete repetitions/],
    [run => run.results.push(structuredClone(run.results[0])), /duplicate repetition/],
    [run => { run.results[0].repetition = 0; }, /invalid repetition/],
    [run => { run.results[0].repetition = run.metadata.repeats + 1; }, /invalid repetition/],
    [run => { run.results[0].frames--; }, /frame count mismatch/],
  ]) {
    const {before, after} = pair();
    mutate(after);
    assert.throws(() => validateRuns(before, after), error);
  }
});

test('an entirely missing case and adapter changes inside a case are rejected', () => {
  const {before, after} = pair();
  after.results = after.results.filter(row => row.mode !== 'idle');
  assert.throws(() => validateRuns(before, after), /case sets differ/);
  const mismatchedAdapter = structuredClone(before);
  mismatchedAdapter.results[0].adapter = 'Different GPU';
  assert.throws(() => validateRuns(before, mismatchedAdapter), /adapter changes within/);
});

test('report contains medians and signed changes for timing, memory and allocations', () => {
  const {before, after} = pair();
  for (const row of after.results) {
    row.render_throughput_fps *= 1.25;
    row.completed_ms.p95 *= 0.8;
    row.cpu_submit_ms.mean *= 0.5;
    row.cpu_percent_one_core *= 0.5;
    row.rss_after_bytes *= 0.5;
    row.owned_gpu_buffer_bytes *= 0.5;
    row.rust_allocations *= 0.5;
    row.rust_reallocations *= 0.5;
    row.rust_requested_bytes *= 0.25;
    if (row.gpu_pass) row.gpu_pass.mean_ms *= 0.5;
  }
  const report = renderComparison(before, after);
  const [timing] = caseRows(report, 'Timing');
  assert.match(timing, /200\.00 → 250\.00 \(\+25\.0%\)/);
  assert.match(timing, /8\.00 → 6\.40 \(-20\.0%\)/);
  assert.match(timing, /4\.00 → 2\.00 \(-50\.0%\)/);
  const [memory] = caseRows(report, 'CPU and memory');
  assert.match(memory, /50\.00 → 25\.00 \(-50\.0%\)/);
  assert.match(memory, /32\.00 → 16\.00 \(-50\.0%\)/);
  assert.match(memory, /4,096 → 2,048 \(-50\.0%\)/);
  const [allocation] = caseRows(report, 'Rust allocation traffic');
  assert.match(allocation, /3\.00 → 1\.50 \(-50\.0%\)/);
  assert.match(allocation, /300\.00 → 75\.00 \(-75\.0%\)/);
});

test('even repeat counts use the arithmetic midpoint of the two central values', () => {
  const {before, after} = pair(2);
  const [timing] = caseRows(renderComparison(before, after), 'Timing');
  assert.match(timing, /150\.00 → 150\.00 \(0\.0%\)/);
  assert.match(timing, /6\.00 → 6\.00 \(0\.0%\)/);
});

test('idle renders no per-frame measurements and zero baselines have no percentage ratio', () => {
  const {before, after} = pair();
  for (const row of before.results) row.rust_requested_bytes = 0;
  const report = renderComparison(before, after);
  const [timing] = caseRows(report, 'Timing', 'image / gpu / idle');
  assert.equal((timing.match(/n\/a → n\/a \(n\/a\)/g) ?? []).length, 4);
  const [allocations] = caseRows(report, 'Rust allocation traffic', 'image / gpu / idle');
  assert.equal((allocations.match(/n\/a → n\/a \(n\/a\)/g) ?? []).length, 2);
  assert.match(report, /0\.00 → 300\.00 \(Δ n\/a: baseline 0\)/);
  assert.doesNotMatch(report, /Infinity|NaN/);
});

test('missing or partly unavailable GPU timestamps stay n/a instead of dropping repetitions', () => {
  const {before, after} = pair();
  before.results[0].gpu_pass = null;
  const [oneMissing] = caseRows(renderComparison(before, after), 'Timing');
  assert.match(oneMissing, /n\/a → 4\.00 \(n\/a\)/);
  delete after.results[0].gpu_pass;
  const report = renderComparison(before, after);
  const [bothMissing] = caseRows(report, 'Timing');
  assert.match(bothMissing, /n\/a → n\/a \(n\/a\)/);
  const [cpuMemory] = caseRows(report, 'CPU and memory', 'image / cpu / animation');
  assert.match(cpuMemory, /n\/a → n\/a \(n\/a\)/);
  assert.doesNotMatch(report, /Infinity|NaN/);
});

test('invalid numeric measurements are not formatted as valid benchmark results', () => {
  for (const value of [-1, Number.NaN, Number.POSITIVE_INFINITY, '100']) {
    const {before, after} = pair();
    after.results[0].cpu_submit_ms.mean = value;
    assert.throws(() => renderComparison(before, after), /invalid numeric metric/);
  }
});

test('report discloses background load and non-FPS, allocator and residency limitations', () => {
  const {before, after} = pair();
  const report = renderComparison(before, after);
  assert.match(report, /not isolated or load-controlled/);
  assert.match(report, /not proof of causality or statistical significance/);
  assert.match(report, /not presented FPS/);
  assert.match(report, /not a pooled percentile/);
  assert.match(report, /exclude Objective-C and driver allocations/);
  assert.match(report, /not total VRAM residency/);
  assert.match(report, /Do not add GPU resources to RSS/);
  assert.match(report, /static final-size target/);
});
