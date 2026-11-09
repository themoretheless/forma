/** Paired diagnostic; timings are not application/browser performance qualification. */
import assert from 'node:assert/strict';
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { cpus, loadavg, platform, arch } from 'node:os';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const script = fileURLToPath(import.meta.url);
const median = values => {
  const a = [...values].sort((x, y) => x - y),
    i = a.length >> 1;
  return a.length % 2 ? a[i] : (a[i - 1] + a[i]) / 2;
};
async function hashes(directory) {
  const result = {};
  for (const name of (await readdir(directory)).sort())
    if (/\.(js|wasm)$/.test(name))
      result[name] = createHash('sha256')
        .update(await readFile(resolve(directory, name)))
        .digest('hex');
  return result;
}
if (process.argv[2] === '--worker') {
  const [installation, label, roundText] = process.argv.slice(3),
    directory = resolve(installation, 'dist/package');
  const e = await import(pathToFileURL(resolve(directory, 'reactivity.js')));
  const before = await hashes(directory),
    rows = [];
  for (const operation of ['signal-writes', 'observed-writes', 'effect-churn', 'graph-reset']) {
    const times = [];
    for (let sample = -3; sample < 12; sample++) {
      e.resetReactiveSystem();
      let elapsed;
      if (operation === 'signal-writes' || operation === 'observed-writes') {
        const source = e.signal(0);
        let runs = 0,
          last = 0;
        const observer =
          operation === 'observed-writes'
            ? e.effect(() => {
                runs++;
                last = source();
              })
            : null;
        const start = performance.now();
        for (let n = 1; n <= 100000; n++) source.set(n);
        elapsed = performance.now() - start;
        assert.equal(source(), 100000);
        if (observer) {
          assert.equal(last, 100000);
          assert.equal(runs, 100001);
          observer.dispose();
        }
        source.dispose();
      } else if (operation === 'effect-churn') {
        const source = e.signal(1);
        let runs = 0,
          cleanups = 0;
        const start = performance.now();
        for (let n = 0; n < 10000; n++) {
          const handle = e.effect(register => {
            assert.equal(source(), 1);
            runs++;
            register(() => {
              cleanups++;
            });
          });
          handle.dispose();
        }
        elapsed = performance.now() - start;
        assert.equal(runs, 10000);
        assert.equal(cleanups, 10000);
        source.dispose();
      } else {
        // Exclude graph construction; measure retirement/reset of 512 live pairs.
        const source = Array.from({ length: 512 }, (_, n) => e.signal(n));
        const derived = source.map(value => e.computed(() => value() * 2));
        derived.forEach((value, n) => assert.equal(value(), n * 2));
        const start = performance.now();
        e.resetReactiveSystem();
        elapsed = performance.now() - start;
        derived.forEach((value, n) => assert.equal(value(), n * 2));
        const fresh = e.signal(9);
        source[0].set(100);
        assert.equal(fresh(), 9);
        fresh.dispose();
      }
      assert.equal(e.getGraphTelemetry().nodes, 0);
      if (sample >= 0) times.push(elapsed);
    }
    rows.push({ operation, samplesMs: times, medianMs: median(times) });
  }
  assert.deepEqual(await hashes(directory), before);
  console.log(
    JSON.stringify({
      label,
      round: Number(roundText),
      rows,
      load: loadavg(),
      artifactSha256: before,
    }),
  );
} else {
  const [baseline, candidate, output] = process.argv.slice(2);
  assert(baseline && candidate && output);
  const workers = [],
    orders = [
      ['baseline', 'control', 'candidate'],
      ['candidate', 'control', 'baseline'],
      ['control', 'baseline', 'candidate'],
      ['candidate', 'baseline', 'control'],
      ['baseline', 'candidate', 'control'],
      ['control', 'candidate', 'baseline'],
    ];
  for (let round = 0; round < orders.length; round++)
    for (const label of orders[round]) {
      const result = spawnSync(
        process.execPath,
        [script, '--worker', label === 'candidate' ? candidate : baseline, label, String(round)],
        { encoding: 'utf8', timeout: 120000, maxBuffer: 4 * 1024 * 1024 },
      );
      assert.equal(result.status, 0, result.stderr || String(result.error));
      workers.push(JSON.parse(result.stdout));
    }
  const summary = [];
  for (const operation of workers[0].rows.map(row => row.operation)) {
    const pairs = orders.map((_, round) => {
      const get = label =>
        workers
          .find(worker => worker.round === round && worker.label === label)
          .rows.find(row => row.operation === operation).medianMs;
      const a = get('baseline');
      return {
        round,
        baselineMs: a,
        controlMs: get('control'),
        candidateMs: get('candidate'),
        aaRatio: get('control') / a,
        abRatio: get('candidate') / a,
      };
    });
    summary.push({
      operation,
      pairs,
      medianAaRatio: median(pairs.map(pair => pair.aaRatio)),
      medianAbRatio: median(pairs.map(pair => pair.abRatio)),
    });
  }
  await writeFile(
    output,
    JSON.stringify(
      {
        status: 'diagnostic',
        createdAt: new Date().toISOString(),
        node: process.version,
        host: {
          platform: platform(),
          arch: arch(),
          cpus: cpus().length,
          model: cpus()[0].model,
          load: loadavg(),
        },
        warmups: 3,
        samplesPerWorkerOperation: 12,
        rounds: 6,
        workers,
        summary,
        scope:
          'Serial isolated Node workers, balanced orders, baseline A/A control and baseline/current A/B. No quiet-host, application latency, browser or memory plateau qualification.',
      },
      null,
      2,
    ) + '\n',
  );
  console.log(
    JSON.stringify(
      summary.map(({ operation, medianAaRatio, medianAbRatio }) => ({
        operation,
        medianAaRatio,
        medianAbRatio,
      })),
      null,
      2,
    ),
  );
}
