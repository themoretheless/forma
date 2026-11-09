/** Node/jsdom diagnostic of AOT snapshot cost; the bypass variant is unsafe for production. */
import assert from 'node:assert/strict';
import { readFile, writeFile, readdir, cp, mkdir, mkdtemp } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { tmpdir, cpus, loadavg, platform, arch } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { JSDOM } from 'jsdom';
const script = fileURLToPath(import.meta.url),
  root = resolve(dirname(script), '..');
const sha = value => createHash('sha256').update(value).digest('hex');
const median = values => {
  const a = [...values].sort((x, y) => x - y),
    m = a.length >> 1;
  return a.length % 2 ? a[m] : (a[m - 1] + a[m]) / 2;
};
async function hashes(directory) {
  const result = {};
  for (const name of (await readdir(directory)).sort())
    if (/\.(js|wasm)$/.test(name)) result[name] = sha(await readFile(resolve(directory, name)));
  return result;
}
if (process.argv[2] === '--worker') {
  const [directory, label, roundText, samplesText] = process.argv.slice(3),
    samples = Number(samplesText);
  const dom = new JSDOM('<!doctype html><body></body>');
  for (const key of ['window', 'document', 'Node', 'Element']) globalThis[key] = dom.window[key];
  const forma = await import(pathToFileURL(resolve(directory, 'forma.js')).href),
    reactive = await import(pathToFileURL(resolve(directory, 'reactivity.js')).href);
  const startHashes = await hashes(directory),
    rows = [];
  for (const controls of [8, 32, 96, 256]) {
    const source = version =>
      `component App { state count: Number = 0; let doubled = state.count * 2; Column { ${Array.from({ length: controls }, (_, i) => `Button { text:'${version}-${i}'; clicked -> state.increment('count',1); }`).join(' ')} Text { text:state.count; } } }`;
    const ast = forma.parseFormaSource(source('old')),
      next = forma.parseFormaSource(source('new')),
      identity = sha(JSON.stringify([ast, next]));
    for (const operation of ['construct-dispose', 'mount', 'HMR']) {
      const times = [];
      for (let iteration = -2; iteration < samples; iteration++) {
        let inst, handle;
        const container = document.createElement('div');
        let elapsed;
        try {
          if (operation === 'construct-dispose') {
            const start = performance.now();
            inst = forma.compileFormaAst(ast);
            inst.dispose();
            elapsed = performance.now() - start;
          } else {
            inst = forma.compileFormaAst(ast);
            if (operation === 'mount') {
              const start = performance.now();
              handle = inst.mount(container);
              elapsed = performance.now() - start;
              assert.equal(container.querySelectorAll('button').length, controls);
              assert.match(container.textContent, /old-0/);
              container.querySelector('button').click();
              assert.equal(inst.state.count(), 1);
              assert(container.textContent.endsWith('1'));
            } else {
              handle = inst.mount(container);
              const old = container.querySelector('button');
              inst.state.count.set(3);
              const start = performance.now();
              inst.hotReload(next);
              elapsed = performance.now() - start;
              assert.equal(container.querySelectorAll('button').length, controls);
              assert.match(container.textContent, /new-0/);
              old.click();
              assert.equal(inst.state.count(), 3);
              container.querySelector('button').click();
              assert.equal(inst.state.count(), 4);
              assert(container.textContent.endsWith('4'));
            }
          }
        } finally {
          handle?.unmount();
          inst?.dispose();
        }
        assert.equal(reactive.getGraphTelemetry().nodes, 0);
        assert(Number.isFinite(elapsed) && elapsed >= 0);
        if (iteration >= 0) times.push(elapsed);
      }
      rows.push({
        controls,
        operation,
        fixtureAstSha256: identity,
        milliseconds: times,
        medianMs: median(times),
      });
    }
    assert.equal(sha(JSON.stringify([ast, next])), identity, 'Workload mutated trusted input AST');
  }
  assert.deepEqual(await hashes(directory), startHashes);
  dom.window.close();
  console.log(
    JSON.stringify({
      label,
      round: Number(roundText),
      node: process.version,
      rows,
      runtimeSha256: startHashes,
      loadavg: loadavg(),
      memory: process.memoryUsage(),
    }),
  );
} else {
  const output = resolve(process.argv[2] ?? 'benchmarks/forma-snapshot-cost.json'),
    samples = Number(process.argv[3] ?? 6),
    rounds = Number(process.argv[4] ?? 3),
    externalBaseline = process.argv[5] ? resolve(process.argv[5]) : null;
  assert(Number.isInteger(samples) && samples >= 3 && samples <= 100);
  assert(Number.isInteger(rounds) && rounds >= 1 && rounds <= 12);
  const directory = await mkdtemp(resolve(tmpdir(), 'svet-snapshot-cost-')),
    candidate = resolve(directory, 'candidate'),
    baseline = resolve(directory, 'trusted-bypass');
  for (const target of [candidate, baseline]) {
    await cp(
      target === baseline && externalBaseline ? externalBaseline : resolve(root, 'dist/package'),
      target,
      { recursive: true },
    );
    await writeFile(resolve(target, 'package.json'), '{"type":"module"}\n');
  }
  const marker = 'function snapshotFormaAst(ast) {',
    path = resolve(baseline, 'forma.js'),
    text = await readFile(path, 'utf8');
  assert.equal(text.split(marker).length, 2);
  if (!externalBaseline)
    await writeFile(
      path,
      text.replace(
        marker,
        marker +
          '\n    assertFormaAstContracts(ast); return ast; // Diagnostic only: bypass ownership copy.',
      ),
    );
  const candidateHashes = await hashes(candidate),
    baselineHashes = await hashes(baseline);
  assert.deepEqual(
    Object.keys(candidateHashes).filter(key => candidateHashes[key] !== baselineHashes[key]),
    ['forma.js'],
  );
  const report = {
    scope: 'Node/jsdom AOT snapshot cost diagnostic; no browser/native/FPS or allocation claim',
    baseline: externalBaseline
      ? 'Preserved complete runtime before optimization; ownership and validation remain enabled.'
      : 'Current built runtime with snapshot copy and post-copy validation bypassed; input validation retained. Unsafe for production; trusted immutable benchmark AST only.',
    externalBaseline,
    node: process.version,
    platform: platform(),
    arch: arch(),
    cpu: cpus()[0]?.model,
    logicalCPUs: cpus().length,
    startedAt: new Date().toISOString(),
    loadavgBefore: loadavg(),
    samples,
    rounds,
    warmupIterationsPerCell: 2,
    forcedGCs: 0,
    artifactDirectory: directory,
    harnessSha256: sha(await readFile(script)),
    candidateHashes,
    baselineHashes,
    runs: [],
  };
  const orders = [
    ['A1', 'B', 'A2'],
    ['B', 'A2', 'A1'],
    ['A2', 'A1', 'B'],
  ];
  for (let round = 0; round < rounds; round++)
    for (const label of orders[round % orders.length]) {
      process.stderr.write(`snapshot benchmark round ${round + 1}/${rounds} ${label}\n`);
      const result = spawnSync(
        process.execPath,
        [
          script,
          '--worker',
          label === 'B' ? candidate : baseline,
          label,
          String(round),
          String(samples),
        ],
        { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 },
      );
      if (result.status !== 0) throw Error(result.stderr || result.stdout || 'worker failed');
      const run = JSON.parse(result.stdout);
      assert.deepEqual(run.runtimeSha256, label === 'B' ? candidateHashes : baselineHashes);
      assert.equal(run.label, label);
      assert.equal(run.round, round);
      report.runs.push(run);
    }
  report.summary = [];
  for (const row of report.runs[0].rows) {
    const cells = label =>
      report.runs
        .filter(run => run.label === label)
        .map(
          run =>
            run.rows.find(r => r.controls === row.controls && r.operation === row.operation)
              .medianMs,
        );
    const a1 = cells('A1'),
      a2 = cells('A2'),
      b = cells('B'),
      base = median([...a1, ...a2]);
    report.summary.push({
      controls: row.controls,
      operation: row.operation,
      baselineRoundMediansMs: { A1: a1, A2: a2 },
      candidateRoundMediansMs: b,
      baselineMedianMs: base,
      candidateMedianMs: median(b),
      candidateToBaselineRatio: median(b) / base,
      pairedCandidateMinusBaselineMs: b.map((v, i) => v - (a1[i] + a2[i]) / 2),
      aaMedianRatio: median(a2) / median(a1),
      aaMaxRoundRelativeSpread: Math.max(
        ...a1.map((a, i) => Math.abs(a - a2[i]) / Math.max(a, a2[i])),
      ),
    });
  }
  assert.deepEqual(await hashes(candidate), candidateHashes);
  assert.deepEqual(await hashes(baseline), baselineHashes);
  assert.equal(sha(await readFile(script)), report.harnessSha256, 'Harness changed during run');
  for (const run of report.runs)
    for (const row of run.rows) {
      const reference = report.runs[0].rows.find(
        r => r.controls === row.controls && r.operation === row.operation,
      );
      assert.equal(row.fixtureAstSha256, reference.fixtureAstSha256);
    }
  report.completedAt = new Date().toISOString();
  report.loadavgAfter = loadavg();
  report.status = 'completed';
  report.limitations = [
    'Synthetic controls in Node/jsdom, not real browser or native interaction.',
    'Few rounds and uncontrolled host; A/A scatter is reported, not removed.',
    'Construction includes dispose; mount excludes construction; HMR includes staged replacement.',
    'Timings do not establish allocation count, peak memory, p95 interaction latency or release readiness.',
  ];
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify({ output, status: report.status, cells: report.summary.length }));
}
