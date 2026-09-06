// Compare completed, matching perf-bench runs. Does not build or run benchmarks.
import {createHash} from 'node:crypto';
import {readFile, readdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const comparableMetadata = [
  'frames', 'repeats', 'platform', 'osRelease', 'arch', 'cpu',
  'logicalCpus', 'systemRamBytes', 'rust',
];
const numericMetadata = new Set(['frames', 'repeats', 'logicalCpus', 'systemRamBytes']);
const modes = new Set(['animation', 'resize', 'forced', 'idle']);

export async function loadRun(directory) {
  const path = resolve(directory);
  const json = async name => JSON.parse(await readFile(resolve(path, name), 'utf8'));
  const [metadata, results, entries] = await Promise.all([
    json('metadata.json'), json('results.json'), readdir(path, {withFileTypes: true}),
  ]);
  const names = entries.filter(entry => entry.isFile() && entry.name.endsWith('.ui'))
    .map(entry => entry.name).sort();
  const fixtures = Object.fromEntries(await Promise.all(names.map(async name => [
    name, createHash('sha256').update(await readFile(resolve(path, name))).digest('hex'),
  ])));
  return {directory: path, metadata, results, fixtures};
}

function assert(condition, message) {
  if (!condition) throw new Error(`Incomparable benchmark runs: ${message}`);
}

function validateMetadata(run, side) {
  assert(run.metadata && typeof run.metadata === 'object', `${side}: missing metadata`);
  for (const field of comparableMetadata) {
    const value = run.metadata[field];
    assert(numericMetadata.has(field)
      ? Number.isSafeInteger(value) && value > 0
      : typeof value === 'string' && value.length > 0,
    `${side}: invalid metadata.${field}`);
  }
}

function groupResults(run, side) {
  assert(Array.isArray(run.results) && run.results.length > 0, `${side}: no results`);
  const groups = new Map();
  for (const row of run.results) {
    assert(row && typeof row === 'object', `${side}: invalid result`);
    assert(typeof row.scene === 'string' && /^[a-zA-Z0-9_-]+$/.test(row.scene), `${side}: invalid scene`);
    assert(['gpu', 'cpu'].includes(row.backend), `${side}: invalid backend`);
    assert(modes.has(row.mode), `${side}: invalid mode`);
    assert(Number.isSafeInteger(row.width) && row.width > 0
      && Number.isSafeInteger(row.height) && row.height > 0
      && Number.isFinite(row.scale) && row.scale > 0, `${side}: invalid target size/DPI`);
    const key = JSON.stringify([row.scene, row.backend, row.mode, row.width, row.height, row.scale]);
    assert(Number.isSafeInteger(row.repetition) && row.repetition >= 1
      && row.repetition <= run.metadata.repeats, `${side}: invalid repetition for ${key}`);
    const expectedFrames = row.mode === 'idle' ? 0 : run.metadata.frames;
    assert(row.frames === expectedFrames, `${side}: frame count mismatch for ${key}`);
    assert(typeof row.adapter === 'string' && row.adapter.length > 0, `${side}: missing adapter for ${key}`);
    assert(run.fixtures?.[`${row.scene}.ui`] && run.fixtures?.[`${row.scene}.template.ui`],
      `${side}: missing source/template fixture for ${row.scene}`);
    if (!groups.has(key)) groups.set(key, []);
    const rows = groups.get(key);
    assert(!rows.some(previous => previous.repetition === row.repetition),
      `${side}: duplicate repetition ${row.repetition} for ${key}`);
    assert(rows.every(previous => previous.adapter === row.adapter), `${side}: adapter changes within ${key}`);
    rows.push(row);
  }
  for (const [key, rows] of groups) {
    assert(rows.length === run.metadata.repeats, `${side}: incomplete repetitions for ${key}`);
  }
  return groups;
}

export function validateRuns(before, after) {
  validateMetadata(before, 'before');
  validateMetadata(after, 'after');
  for (const field of comparableMetadata) {
    assert(before.metadata[field] === after.metadata[field],
      `metadata.${field} differs (${JSON.stringify(before.metadata[field])} vs ${JSON.stringify(after.metadata[field])})`);
  }
  const beforeNames = Object.keys(before.fixtures ?? {}).sort();
  const afterNames = Object.keys(after.fixtures ?? {}).sort();
  assert(beforeNames.length > 0 && JSON.stringify(beforeNames) === JSON.stringify(afterNames),
    'source/template fixture file sets differ or are empty');
  for (const name of beforeNames) {
    assert(/^[a-f0-9]{64}$/.test(before.fixtures[name]) && before.fixtures[name] === after.fixtures[name],
      `fixture ${name} has different SHA-256/content`);
  }
  const beforeGroups = groupResults(before, 'before');
  const afterGroups = groupResults(after, 'after');
  assert(JSON.stringify([...beforeGroups.keys()].sort()) === JSON.stringify([...afterGroups.keys()].sort()),
    'scene/backend/mode/target/DPI case sets differ');
  for (const [key, rows] of beforeGroups) {
    assert(rows[0].adapter === afterGroups.get(key)[0].adapter, `adapter differs for ${key}`);
  }
  return {beforeGroups, afterGroups};
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function optionalNumber(value) {
  if (value == null) return null;
  assert(typeof value === 'number' && Number.isFinite(value) && value >= 0,
    `invalid numeric metric ${JSON.stringify(value)}`);
  return value;
}

function perFrame(row, field) {
  const value = optionalNumber(row[field]);
  return row.frames > 0 && value !== null ? value / row.frames : null;
}

function allocationsPerFrame(row) {
  const alloc = perFrame(row, 'rust_allocations');
  const realloc = perFrame(row, 'rust_reallocations');
  return alloc !== null && realloc !== null ? alloc + realloc : null;
}

function medianMetric(rows, select) {
  const values = rows.map(row => optionalNumber(select(row)));
  // Do not silently compare different subsets when timestamps are unavailable.
  return values.every(value => value !== null) ? median(values) : null;
}

function metricCell(beforeRows, afterRows, select, digits = 2) {
  const before = medianMetric(beforeRows, select);
  const after = medianMetric(afterRows, select);
  const format = value => value === null ? 'n/a' : value.toLocaleString('en-US', {
    minimumFractionDigits: digits, maximumFractionDigits: digits,
  });
  if (before === null || after === null) return `${format(before)} → ${format(after)} (n/a)`;
  // A zero baseline has no defined relative change, including zero → zero.
  if (before === 0) return `${format(before)} → ${format(after)} (Δ n/a: baseline 0)`;
  const delta = (after / before - 1) * 100;
  const percent = Math.abs(delta) < 0.05 ? '0.0%' : `${delta > 0 ? '+' : ''}${delta.toFixed(1)}%`;
  return `${format(before)} → ${format(after)} (${percent})`;
}

const escaped = value => String(value).replaceAll('\\', '\\\\').replaceAll('|', '\\|')
  .replaceAll('`', '\\`').replaceAll('\n', ' ');

export function renderComparison(before, after) {
  const {beforeGroups, afterGroups} = validateRuns(before, after);
  const meta = before.metadata;
  const lines = [
    '# Forma benchmark comparison', '',
    `Before: ${escaped(before.directory)} (${escaped(before.metadata.createdAt ?? 'time unknown')}).`,
    `After: ${escaped(after.directory)} (${escaped(after.metadata.createdAt ?? 'time unknown')}).`, '',
    `CPU: ${escaped(meta.cpu)}; ${meta.logicalCpus} logical CPUs; ${(meta.systemRamBytes / 2 ** 30).toFixed(1)} GiB RAM. `
      + `${escaped(meta.platform)} / ${escaped(meta.arch)} / ${escaped(meta.osRelease)}; ${escaped(meta.rust)}.`,
    `${meta.repeats} separate benchmark processes per case, ${meta.frames} measured frames after 20 warmup frames. `
      + 'Machine/runtime metadata, complete case/repetition sets, adapters, DPI and SHA-256 of every .ui fixture match.', '',
    '**Each cell is before → after (relative change).** Values are medians across processes; '
      + 'completed p95 is the median of each process’s p95, not a pooled percentile. '
      + 'Higher throughput is better; lower latency, allocation traffic and memory are generally better. '
      + 'CPU % alone is not a speed score: faster unpaced rendering can occupy a CPU more fully.', '',
    '**Load caveat:** the machine is not isolated or load-controlled. Separate benchmark processes '
      + 'do not isolate GPU contention, background applications, power state or thermal conditions. '
      + 'Changes are observations, not proof of causality or statistical significance.', '',
  ];

  const sections = [
    ['Timing', [
      ['Render frames/s', row => row.mode === 'idle' ? null : row.render_throughput_fps],
      ['Completed p95 ms', row => row.mode === 'idle' ? null : row.completed_ms?.p95],
      ['CPU submit mean ms', row => row.mode === 'idle' ? null : row.cpu_submit_ms?.mean],
      ['GPU pass mean ms', row => row.mode === 'idle' || row.backend !== 'gpu' ? null : row.gpu_pass?.mean_ms],
    ]],
    ['CPU and memory', [
      ['CPU % (one core = 100%)', row => row.cpu_percent_one_core],
      ['RSS after MiB', row => {
        const bytes = optionalNumber(row.rss_after_bytes);
        return bytes === null ? null : bytes / 2 ** 20;
      }],
      ['Owned GPU buffer bytes', row => row.backend === 'gpu' ? row.owned_gpu_buffer_bytes : null, 0],
    ]],
    ['Rust allocation traffic', [
      ['Alloc + realloc / frame', allocationsPerFrame],
      ['Requested bytes / frame', row => perFrame(row, 'rust_requested_bytes')],
    ]],
  ];
  for (const [heading, metrics] of sections) {
    lines.push(`## ${heading}`, '',
      `| Scene / backend / mode / target @ DPI | ${metrics.map(([name]) => name).join(' | ')} |`,
      `|---|${metrics.map(() => '---:').join('|')}|`);
    for (const [key, beforeRows] of beforeGroups) {
      const row = beforeRows[0];
      const label = `${row.scene} / ${row.backend} / ${row.mode} / ${row.width}×${row.height} @ ${row.scale}`;
      const cells = metrics.map(([, select, digits]) => metricCell(beforeRows, afterGroups.get(key), select, digits));
      lines.push(`| ${escaped(label)} | ${cells.join(' | ')} |`);
    }
    lines.push('');
  }
  lines.push('## Interpretation limits', '',
    '- Render frames/s is serialized offscreen render throughput, not presented FPS; there is no OS window/compositor or pipelined frame overlap.',
    '- GPU timestamps come from a separate profiled pass. Resize timestamps measure its static final-size target, not texture replacement or window resize.',
    '- Idle performs no rendering. Frame-based metrics are n/a, not zero-cost frames. Missing/null GPU timing is unavailable, never zero. A zero baseline has no percentage ratio.',
    '- Rust allocation counts/traffic exclude Objective-C and driver allocations. The counting allocator also adds instrumentation overhead; requested bytes are traffic, not retained memory.',
    '- RSS is current process memory after the phase, not a phase peak. Owned GPU buffers are requested live resource capacities, not total VRAM residency; they exclude the target texture, swapchain, staging and driver heaps. Do not add GPU resources to RSS on unified-memory systems.',
    '- Source revision/dirty flags are provenance, not a complete code fingerprint. See each run’s metadata.json and results.json for exact scalar measurements, lifetime peaks, targets and diagnostics.', '',
    '## Provenance', '',
    `Before revision: ${escaped(before.metadata.revision ?? 'unknown')}; dirty: ${escaped(before.metadata.dirty ?? 'unknown')}.`,
    `After revision: ${escaped(after.metadata.revision ?? 'unknown')}; dirty: ${escaped(after.metadata.dirty ?? 'unknown')}.`, '',
    '| Fixture | Matching SHA-256 |', '|---|---|');
  for (const name of Object.keys(before.fixtures).sort()) {
    lines.push(`| ${escaped(name)} | ${before.fixtures[name]} |`);
  }
  return `${lines.join('\n')}\n`;
}

async function main() {
  if (process.argv.length !== 4) throw new Error('Usage: node scripts/compare-bench.mjs BEFORE_DIR AFTER_DIR');
  const [before, after] = await Promise.all(process.argv.slice(2).map(loadRun));
  const report = renderComparison(before, after);
  const output = resolve(after.directory, 'COMPARISON.md');
  await writeFile(output, report);
  console.log(output);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
