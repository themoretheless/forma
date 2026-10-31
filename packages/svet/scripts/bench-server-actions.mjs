import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFile, readdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {parseArgs} from 'node:util';

const {values} = parseArgs({options: {
  baseline: {type: 'string'}, candidate: {type: 'string'}, output: {type: 'string'},
  rounds: {type: 'string', default: '7'}, calls: {type: 'string', default: '5000'},
}});
assert.ok(values.baseline, 'Pass --baseline DIR and optionally --candidate DIR --output FILE');
const rounds = Number(values.rounds), calls = Number(values.calls);
assert.ok(Number.isSafeInteger(rounds) && rounds > 0 && Number.isSafeInteger(calls) && calls > 0);
const variants = {}, compiledHashes = {};
for (const name of ['baseline', 'candidate']) if (values[name]) {
  const directory = resolve(values[name]);
  variants[name] = await import(pathToFileURL(resolve(directory, 'index.js')).href);
  compiledHashes[name] = {};
  for (const file of (await readdir(directory)).filter(file => file.endsWith('.js')).sort()) {
    compiledHashes[name][file] = createHash('sha256').update(await readFile(resolve(directory, file))).digest('hex');
  }
}

async function sample(engine, mode) {
  const terminal = Object.freeze({value: 7}), failure = new Error('expected RPC failure');
  let rollbacks = 0, result;
  const action = engine.createServerAction({
    rpc: mode === 'primitive' ? async () => 7
      : mode === 'object' ? async () => terminal
      : mode === 'nested' ? () => ({then(resolve) { resolve({then(next) { next(7); }}); }})
      : () => Promise.reject(failure),
    optimisticUpdate: mode === 'rejection' ? () => () => { rollbacks++; } : undefined,
  });
  let elapsedMs;
  try {
    const start = performance.now();
    if (mode === 'rejection') {
      for (let i = 0; i < calls; i++) {
        try { await action(); throw new Error('Expected rejection'); }
        catch (error) { if (error !== failure) throw error; }
      }
    } else for (let i = 0; i < calls; i++) result = await action();
    elapsedMs = performance.now() - start;
    if (mode === 'rejection') { assert.equal(rollbacks, calls); assert.equal(action.error(), failure); }
    else { assert.equal(result, mode === 'object' ? terminal : 7); assert.equal(action.error(), null); }
    assert.equal(action.pending(), false);
    assert.equal(engine.getGraphTelemetry().nodes, 2);
  } finally { action.dispose(); }
  assert.equal(engine.getGraphTelemetry().nodes, 0);
  return elapsedMs;
}

const median = data => [...data].sort((a, b) => a - b)[Math.floor(data.length / 2)];
const records = [];
for (const mode of ['primitive', 'object', 'nested', 'rejection']) {
  const samples = Object.fromEntries(Object.keys(variants).map(name => [name, []]));
  for (let round = -2; round < rounds; round++) {
    for (const name of round % 2 === 0 ? Object.keys(variants) : Object.keys(variants).reverse()) {
      const elapsed = await sample(variants[name], mode);
      if (round >= 0) samples[name].push(elapsed);
    }
  }
  records.push({mode, calls, variants: Object.fromEntries(Object.entries(samples).map(([name, data]) =>
    [name, {medianMs: median(data), samplesMs: data}]))});
}
const report = {completedAt: new Date().toISOString(), node: process.version, platform: process.platform,
  arch: process.arch, rounds, warmups: 2, compiledHashes, records};
if (values.output) await writeFile(resolve(values.output), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
