import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { cpus } from 'node:os';

const { values } = parseArgs({
  options: {
    baseline: { type: 'string' },
    candidate: { type: 'string' },
    output: { type: 'string' },
    rounds: { type: 'string', default: '7' },
    iterations: { type: 'string', default: '32' },
    untracks: { type: 'string', default: '4000' },
  },
});
assert(values.baseline, 'Pass --baseline DIR and optionally --candidate DIR');
const rounds = Number(values.rounds),
  iterations = Number(values.iterations),
  untracks = Number(values.untracks);
for (const count of [rounds, iterations]) assert(Number.isSafeInteger(count) && count > 0);
assert(Number.isSafeInteger(untracks) && untracks >= 0);
const engines = {},
  hashes = {};
for (const mode of ['baseline', 'candidate'])
  if (values[mode]) {
    const directory = resolve(values[mode]);
    engines[mode] = await import(pathToFileURL(resolve(directory, 'index.js')).href);
    hashes[mode] = {};
    for (const file of (await readdir(directory)).filter(file => file.endsWith('.js')).sort()) {
      hashes[mode][file] = createHash('sha256')
        .update(await readFile(resolve(directory, file)))
        .digest('hex');
    }
  }
if (hashes.candidate) {
  assert.deepEqual(
    Object.keys(hashes.baseline),
    Object.keys(hashes.candidate),
    'Variant module lists differ',
  );
  const changed = Object.keys(hashes.baseline).filter(
    file => hashes.baseline[file] !== hashes.candidate[file],
  );
  assert.deepEqual(changed, ['reactivity.js'], 'Only observer-context registration may differ');
}
function run(engine, depth) {
  const source = engine.signal(0),
    incidental = engine.signal(0),
    read = () => incidental();
  let evaluations = 0;
  const chain = [
    engine.computed(() => {
      evaluations++;
      for (let i = 0; i < untracks; i++) engine.untrack(read);
      return source();
    }),
  ];
  for (let i = 1; i < depth; i++) {
    const previous = chain.at(-1);
    chain.push(engine.computed(() => previous() + 1));
  }
  const top = chain.at(-1);
  try {
    assert.equal(top.peek(), depth - 1);
    // Warm this graph before timing; a whole-scenario warmup also precedes rounds.
    for (let i = 1; i <= 4; i++) {
      source.set(i);
      assert.equal(top.peek(), i + depth - 1);
    }
    const before = evaluations,
      start = performance.now();
    for (let i = 0; i < iterations; i++) {
      source.set(i + 10);
      assert.equal(top.peek(), i + 10 + depth - 1);
    }
    const milliseconds = performance.now() - start;
    assert.equal(evaluations - before, iterations);
    const after = evaluations;
    incidental.set(1);
    assert.equal(top.peek(), iterations + 9 + depth - 1);
    assert.equal(evaluations, after, 'Untracked read became a dependency');
    const telemetry = engine.getGraphTelemetry();
    assert.equal(telemetry.nodes, depth + 2);
    assert.equal(telemetry.edges, depth);
    return milliseconds;
  } finally {
    chain.reverse().forEach(value => value.dispose());
    source.dispose();
    incidental.dispose();
    assert.equal(engine.getGraphTelemetry().nodes, 0);
  }
}
const workloads = [];
for (const depth of [1, 32, 128, 256]) {
  for (let warmup = 0; warmup < 3; warmup++)
    for (const engine of Object.values(engines)) run(engine, depth);
  const samples = [];
  for (let round = 0; round < rounds; round++) {
    const order = Object.keys(engines);
    if (round % 2) order.reverse();
    const sample = { round, order };
    for (const name of order) sample[name] = run(engines[name], depth);
    samples.push(sample);
  }
  const medians = Object.fromEntries(
    Object.keys(engines).map(name => [
      name,
      samples.map(sample => sample[name]).sort((a, b) => a - b)[Math.floor(rounds / 2)],
    ]),
  );
  workloads.push({ depth, iterations, untracksPerEvaluation: untracks, medians, samples });
}
const result = {
  measuredAt: new Date().toISOString(),
  node: process.version,
  execArgv: process.execArgv,
  benchmarkSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  platform: process.platform,
  arch: process.arch,
  cpu: cpus()[0]?.model,
  hashes,
  workloads,
  scope:
    'Synthetic nested-computed invalidation with repeated untrack calls at the leaf. ' +
    'Includes source writes, graph evaluation and assertions. Both variants must preserve values, ' +
    'dependency isolation and teardown. Not application latency or general engine throughput.',
};
if (values.output) await writeFile(values.output, JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
