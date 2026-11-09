/** CPU-profile workload; profiling timings are not performance qualification. */
import assert from 'node:assert/strict';
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
const [runtimeArg, outputArg, iterationsArg = '20000', controlsArg = '256'] = process.argv.slice(2);
assert(runtimeArg && outputArg);
const directory = resolve(runtimeArg),
  iterations = Number(iterationsArg),
  controls = Number(controlsArg);
assert(
  Number.isInteger(iterations) && iterations > 0 && Number.isInteger(controls) && controls > 0,
);
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
async function hashes() {
  const r = {};
  for (const n of (await readdir(directory)).sort())
    if (/\.(js|wasm)$/.test(n)) r[n] = sha(await readFile(resolve(directory, n)));
  return r;
}
const runtimeSha256 = await hashes(),
  forma = await import(pathToFileURL(resolve(directory, 'forma.js')).href),
  reactive = await import(pathToFileURL(resolve(directory, 'reactivity.js')).href);
const source = `component App { state count: Number = 0; let doubled = state.count * 2; Column { ${Array.from({ length: controls }, (_, i) => `Button { text:'old-${i}'; clicked -> state.increment('count',1); }`).join(' ')} Text { text:state.count; } } }`;
const ast = forma.parseFormaSource(source),
  astSha256 = sha(JSON.stringify(ast));
for (let n = 0; n < 200; n++) forma.compileFormaAst(ast).dispose();
const started = performance.now();
for (let n = 0; n < iterations; n++) forma.compileFormaAst(ast).dispose();
const elapsedMs = performance.now() - started;
assert.equal(reactive.getGraphTelemetry().nodes, 0);
assert.equal(sha(JSON.stringify(ast)), astSha256);
assert.deepEqual(await hashes(), runtimeSha256);
await writeFile(
  resolve(outputArg),
  JSON.stringify(
    {
      scope: 'CPU sampling workload; instrumented elapsed time is not a performance benchmark',
      node: process.version,
      controls,
      iterations,
      warmup: 200,
      elapsedMs,
      astSha256,
      runtimeSha256,
      nodesAfter: reactive.getGraphTelemetry().nodes,
    },
    null,
    2,
  ) + '\n',
);
