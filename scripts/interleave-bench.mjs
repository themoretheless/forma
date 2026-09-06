// Sequential paired A/B experiment over two already-built perf_bench executables.
import {createHash} from 'node:crypto';
import {constants} from 'node:fs';
import {access, mkdir, readFile, writeFile} from 'node:fs/promises';
import {spawnSync} from 'node:child_process';
import {arch, cpus, platform, release, totalmem} from 'node:os';
import {basename, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

export const REPEATS = 4;
export const FRAMES = 240;
export const CASES = [
  ['image', 'gpu', 'animation', 800, 400],
  ['image', 'gpu', 'animation', 1920, 1080],
  ['image', 'gpu', 'animation', 3840, 2160],
  ['image', 'gpu', 'resize', 3840, 2160],
  ['text', 'gpu', 'animation', 1920, 1080],
  ['nested', 'gpu', 'forced', 1920, 1080],
];

export function makeSchedule() {
  const schedule = [];
  for (let repetition = 1; repetition <= REPEATS; repetition++) {
    for (const [caseIndex, spec] of CASES.entries()) {
      // Each case has two AB and two BA pairs; all children run sequentially.
      const order = (repetition - 1 + caseIndex) % 2 === 0 ? ['A', 'B'] : ['B', 'A'];
      for (const [position, variant] of order.entries()) {
        schedule.push({repetition, caseIndex, spec, pairId: `${repetition}-${caseIndex}`,
          order: order.join(''), position: position + 1, variant});
      }
    }
  }
  return schedule;
}

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const median = values => {
  const v = [...values].sort((a, b) => a - b), mid = Math.floor(v.length / 2);
  return v.length % 2 ? v[mid] : (v[mid - 1] + v[mid]) / 2;
};
const number = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : null;
const display = value => value === null ? 'n/a' : value.toFixed(3);
const percent = value => value === null ? 'n/a' : `${value > 0 ? '+' : ''}${value.toFixed(1)}%`;
const escape = value => String(value).replaceAll('|', '\\|').replaceAll('\n', ' ');

export function summarizePairs(pairs, select) {
  const a = pairs.map(pair => number(select(pair.A)));
  const b = pairs.map(pair => number(select(pair.B)));
  if (a.some(value => value === null) || b.some(value => value === null)) return null;
  const deltas = a.every(value => value > 0) ? a.map((value, i) => (b[i] / value - 1) * 100) : null;
  return {
    aMedian: median(a), bMedian: median(b),
    aRange: [Math.min(...a), Math.max(...a)], bRange: [Math.min(...b), Math.max(...b)],
    pairedMedianPercent: deltas ? median(deltas) : null,
    pairedRangePercent: deltas ? [Math.min(...deltas), Math.max(...deltas)] : null,
  };
}

export function renderReport(metadata, results) {
  const metrics = [
    ['Render frames/s', r => r.render_throughput_fps],
    ['CPU submit mean ms', r => r.cpu_submit_ms?.mean],
    ['CPU % (one core = 100%)', r => r.cpu_percent_one_core],
    ['GPU pass mean ms', r => r.gpu_pass?.mean_ms],
  ];
  const summaries = CASES.map((spec, caseIndex) => {
    const pairs = [];
    for (let repetition = 1; repetition <= REPEATS; repetition++) {
      const matches = results.filter(row => row.caseIndex === caseIndex && row.repetition === repetition);
      const A = matches.filter(row => row.variant === 'A'), B = matches.filter(row => row.variant === 'B');
      if (A.length !== 1 || B.length !== 1) throw Error(`Incomplete/duplicate pair ${repetition}-${caseIndex}`);
      if (A[0].adapter !== B[0].adapter) throw Error(`Adapter mismatch in pair ${repetition}-${caseIndex}`);
      pairs.push({A: A[0], B: B[0]});
    }
    return {spec, values: metrics.map(([, select]) => summarizePairs(pairs, select))};
  });
  const label = spec => `${spec[0]} / ${spec[2]} / ${spec[3]}×${spec[4]}`;
  const lines = [
    '# Interleaved Forma renderer A/B benchmark', '',
    `A: ${escape(metadata.binaries.A.path)} (${metadata.binaries.A.sha256}).`,
    `B: ${escape(metadata.binaries.B.path)} (${metadata.binaries.B.sha256}).`, '',
    `${escape(metadata.cpu)}; ${metadata.logicalCpus} logical CPUs; ${(metadata.systemRamBytes / 2 ** 30).toFixed(1)} GiB RAM; `
      + `${escape(metadata.platform)} ${escape(metadata.osRelease)} ${escape(metadata.arch)}. ${escape(metadata.rust)}.`,
    `${REPEATS} adjacent A/B pairs per case, ${FRAMES} measured frames + ${metadata.warmupFrames} warmup frames per process, DPI 2. `
      + 'Each case uses two AB and two BA pairs. Only one benchmark process runs at a time; no builds occur during measurement.', '',
    '**Not an isolated machine:** background applications, GPU contention, CPU scheduling, power state and thermals are not controlled. '
      + 'Pairing balances order and reduces slow time drift; it does not establish causality or statistical significance. '
      + 'Do not treat these results as a confidence interval.', '',
    '## Medians and paired changes', '',
    'Each cell: median A → median B; median of the four paired (B/A − 1) percentage changes. '
      + 'The paired delta is intentionally not the ratio of unpaired medians. Higher throughput is better; '
      + 'lower CPU submit/GPU duration is better. CPU % is utilization, not a performance score.', '',
    `| Scene / mode / target | ${metrics.map(([name]) => name).join(' | ')} |`,
    '|---|---:|---:|---:|---:|',
  ];
  for (const {spec, values} of summaries) {
    const cells = values.map(v => v ? `${display(v.aMedian)} → ${display(v.bMedian)}; ${percent(v.pairedMedianPercent)}` : 'n/a');
    lines.push(`| ${label(spec)} | ${cells.join(' | ')} |`);
  }
  lines.push('', '## Observed min–max across processes', '',
    'Each cell: A min…max / B min…max. These are observed ranges over four processes, not confidence bounds.', '',
    `| Scene / mode / target | ${metrics.map(([name]) => name).join(' | ')} |`,
    '|---|---:|---:|---:|---:|');
  for (const {spec, values} of summaries) {
    const cells = values.map(v => v ? `${v.aRange.map(display).join('…')} / ${v.bRange.map(display).join('…')}` : 'n/a');
    lines.push(`| ${label(spec)} | ${cells.join(' | ')} |`);
  }
  lines.push('', '## Paired percentage-change ranges', '',
    `| Scene / mode / target | ${metrics.map(([name]) => name).join(' | ')} |`,
    '|---|---:|---:|---:|---:|');
  for (const {spec, values} of summaries) {
    lines.push(`| ${label(spec)} | ${values.map(v => v?.pairedRangePercent
      ? v.pairedRangePercent.map(percent).join('…') : 'n/a').join(' | ')} |`);
  }
  lines.push('', '## Limits and raw evidence', '',
    '- Render frames/s is serialized offscreen submit + completion throughput, not presented FPS or pipelined throughput.',
    '- GPU timestamps are a separate profiled pass after CPU/allocator sampling. In resize mode, this is a static final-size target, not resize/texture allocation time.',
    '- GPU data is n/a if any member of the four pairs is missing/unavailable; no selective dropping. A zero A baseline has an undefined percentage delta.',
    '- Global Rust allocator instrumentation affects both binaries. CPU/allocator/RSS figures exclude the later separate timestamp profiling phase.',
    '- Each raw result and its stdout/stderr are persisted in runs/ immediately after its process exits; results.json also contains all completed results in execution order.',
    '- metadata.json records exact binary/fixture SHA-256 hashes, toolchain/machine metadata and the entire pair schedule. Binary and fixture hashes are rechecked at completion.', '');
  return `${lines.join('\n')}\n`;
}

async function main() {
  if (process.argv.length !== 6) throw Error('Usage: node scripts/interleave-bench.mjs A_EXECUTABLE B_EXECUTABLE FIXTURE_DIR OUTPUT_DIR');
  const [aPath, bPath, fixtureDirectory, outputDirectory] = process.argv.slice(2).map(path => resolve(path));
  for (const path of [aPath, bPath]) await access(path, constants.X_OK);
  for (const name of ['metadata.json', 'results.json', 'REPORT.md', 'runs']) {
    try { await access(resolve(outputDirectory, name)); }
    catch (error) { if (error.code === 'ENOENT') continue; throw error; }
    throw Error(`Refusing to overwrite existing benchmark output ${resolve(outputDirectory, name)}`);
  }
  const binaries = Object.fromEntries(await Promise.all([['A', aPath], ['B', bPath]].map(async ([name, path]) =>
    [name, {path, name: basename(path), sha256: hash(await readFile(path))}])));
  const fixturePaths = [...new Set(CASES.flatMap(([scene]) => [`${scene}.ui`, `${scene}.template.ui`]))];
  const fixtures = Object.fromEntries(await Promise.all(fixturePaths.map(async name =>
    [name, hash(await readFile(resolve(fixtureDirectory, name)))])));
  const rust = spawnSync('rustc', ['--version'], {encoding: 'utf8'});
  if (rust.status !== 0) throw Error(`Cannot identify Rust toolchain: ${rust.stderr || rust.error}`);
  const schedule = makeSchedule();
  const metadata = {
    startedAt: new Date().toISOString(), status: 'running', binaries, fixtureDirectory, fixtures,
    platform: platform(), osRelease: release(), arch: arch(), cpu: cpus()[0]?.model,
    logicalCpus: cpus().length, systemRamBytes: totalmem(), rust: rust.stdout.trim(),
    frames: FRAMES, repeats: REPEATS, warmupFrames: 20, scale: 2, schedule,
    notes: ['Sequential neighboring pairs with two AB/two BA orders per case',
      'Background machine load, thermals and frequency are uncontrolled',
      'Offscreen serialized throughput, not on-screen FPS',
      'GPU timestamp pass is separate from primary measurements'],
  };
  await mkdir(resolve(outputDirectory, 'runs'), {recursive: true});
  const saveJson = (name, data) => writeFile(resolve(outputDirectory, name), `${JSON.stringify(data, null, 2)}\n`);
  await saveJson('metadata.json', metadata);
  const results = [];
  try {
    for (const entry of schedule) {
      const [scene, backend, mode, width, height] = entry.spec;
      const name = `${String(results.length + 1).padStart(2, '0')}-${entry.pairId}-${entry.variant}`;
      console.error(`${results.length + 1}/${schedule.length}: ${entry.order} pair ${entry.pairId} ${entry.variant} ${scene} ${mode} ${width}×${height}`);
      const startedAt = new Date().toISOString();
      const run = spawnSync(binaries[entry.variant].path, [
        resolve(fixtureDirectory, `${scene}.ui`), resolve(fixtureDirectory, `${scene}.template.ui`),
        backend, mode, String(width), String(height), String(FRAMES),
      ], {encoding: 'utf8', maxBuffer: 32 * 1024 * 1024});
      await writeFile(resolve(outputDirectory, 'runs', `${name}.stdout.log`), run.stdout ?? '');
      await writeFile(resolve(outputDirectory, 'runs', `${name}.stderr.log`), run.stderr ?? '');
      if (run.status !== 0) throw Error(`${name} failed: ${run.error || run.signal || run.stderr || run.status}`);
      const row = JSON.parse(run.stdout.trim().split('\n').at(-1));
      if (row.backend !== backend || row.mode !== mode || row.width !== width || row.height !== height
        || row.scale !== 2 || row.frames !== FRAMES) throw Error(`${name}: executable returned a different scenario`);
      const result = {...row, ...entry, scene, startedAt, finishedAt: new Date().toISOString(),
        binarySha256: binaries[entry.variant].sha256};
      results.push(result);
      await saveJson(`runs/${name}.json`, result);
      await saveJson('results.json', results);
    }
    for (const binary of Object.values(binaries)) {
      if (hash(await readFile(binary.path)) !== binary.sha256) throw Error(`Binary changed during experiment: ${binary.path}`);
    }
    for (const [name, sha256] of Object.entries(fixtures)) {
      if (hash(await readFile(resolve(fixtureDirectory, name))) !== sha256) throw Error(`Fixture changed during experiment: ${name}`);
    }
    await writeFile(resolve(outputDirectory, 'REPORT.md'), renderReport(metadata, results));
    metadata.status = 'complete';
    metadata.completedAt = new Date().toISOString();
    await saveJson('metadata.json', metadata);
    console.log(resolve(outputDirectory, 'REPORT.md'));
  } catch (error) {
    metadata.status = 'failed';
    metadata.failedAt = new Date().toISOString();
    metadata.error = error.message;
    metadata.completedProcesses = results.length;
    await saveJson('metadata.json', metadata);
    throw error;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
