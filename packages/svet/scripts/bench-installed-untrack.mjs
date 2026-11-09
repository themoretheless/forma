import assert from 'node:assert/strict';
import { readFile, readdir, writeFile, mkdtemp } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const [baseline, candidate, output] = process.argv.slice(2);
assert(baseline && candidate && output, 'Expected BASELINE_PACKAGE CANDIDATE_PACKAGE OUTPUT');
const iterations = process.argv[5] ?? '32';
assert(Number.isSafeInteger(Number(iterations)) && Number(iterations) > 0);
const directories = {
  baseline: resolve(baseline, 'dist/package'),
  candidate: resolve(candidate, 'dist/package'),
};
async function hashes(directory) {
  const result = {};
  for (const name of (await readdir(directory)).filter(x => /\.(js|wasm)$/.test(x)).sort())
    result[name] = createHash('sha256')
      .update(await readFile(resolve(directory, name)))
      .digest('hex');
  return result;
}
const before = {};
for (const [name, directory] of Object.entries(directories)) before[name] = await hashes(directory);
const temporary = await mkdtemp(
  resolve(dirname(fileURLToPath(import.meta.url)), '../node_modules/.cache/installed-untrack-'),
);
const samples = [];
const exec = promisify(execFile);
// Fresh workers avoid realm-wide compatible-copy registration contaminating A/B.
for (const [kind, round, order] of [
  ['AB', 0, ['baseline', 'candidate']],
  ['AB', 1, ['candidate', 'baseline']],
  ['AB', 2, ['baseline', 'candidate']],
  ['AA', 0, ['candidate', 'candidate']],
  ['AA', 1, ['candidate', 'candidate']],
]) {
  const sample = { kind, round, order, reports: [] };
  for (let index = 0; index < order.length; index++) {
    const role = order[index],
      path = resolve(temporary, `${kind}-${round}-${index}.json`);
    await exec(
      process.execPath,
      [
        resolve(dirname(fileURLToPath(import.meta.url)), 'bench-untrack-cores.mjs'),
        '--baseline',
        directories[role],
        '--output',
        path,
        '--rounds',
        '5',
        '--iterations',
        iterations,
        '--untracks',
        '4000',
      ],
      { maxBuffer: 4 * 1024 * 1024 },
    );
    sample.reports.push(JSON.parse(await readFile(path, 'utf8')));
  }
  samples.push(sample);
}
for (const [name, directory] of Object.entries(directories))
  assert.deepEqual(await hashes(directory), before[name], 'Artifact changed during benchmark');
const median = values => values.sort((a, b) => a - b)[Math.floor(values.length / 2)];
const summary = [1, 32, 128, 256].map(depth => {
  const times = { baseline: [], candidate: [] },
    aaRatios = [];
  for (const sample of samples) {
    const values = sample.reports.map(
      r => r.workloads.find(w => w.depth === depth).medians.baseline,
    );
    if (sample.kind === 'AB') sample.order.forEach((role, i) => times[role].push(values[i]));
    else aaRatios.push(values[1] / values[0]);
  }
  return {
    depth,
    baselineMedianMs: median(times.baseline),
    candidateMedianMs: median(times.candidate),
    candidateOverBaseline: median(times.candidate) / median(times.baseline),
    aaRatios,
  };
});
const report = {
  status: 'measured',
  startedArtifacts: before,
  directories,
  summary,
  samples,
  temporary,
  completedAt: new Date().toISOString(),
  driverSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  scope:
    'Fresh single-copy workers; nested computed/untrack synthetic workload, alternating AB and candidate AA controls. Host may run other work (including named long HTTP soak). No application throughput, multi-copy hotpath or uncontended qualification.',
};
await writeFile(output, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(summary, null, 2));
