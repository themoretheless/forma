import assert from 'node:assert/strict';
import { readFile, writeFile, readdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
assert(global.gc, 'Use --expose-gc');
const [installation, output] = process.argv.slice(2);
assert(installation && output);
const directory = resolve(installation, 'dist/package'),
  engine = await import(pathToFileURL(resolve(directory, 'index.js'))),
  runtime = await import(pathToFileURL(resolve(directory, 'server.js')));
const kinds = ['explicit-dispose', 'construction-throw', 'construction-dispose', 'cleanup-throw'],
  held = [],
  refs = [],
  graph = [];
let controls = Array.from({ length: 4 }, () => ({ live: true }));
const controlRefs = controls.map(value => new WeakRef(value));
for (const kind of kinds)
  for (let n = 0; n < 4; n++)
    await runtime.runWithIsolatedContextAsync(() => {
      const core = engine.getWasmCore();
      refs.push({ kind, core: new WeakRef(core), memory: new WeakRef(core.memory) });
      let stop;
      const failure = Error('root construction failed');
      try {
        engine.createRoot(dispose => {
          stop = dispose;
          engine.computed(() => 1)();
          engine.effect(() => {});
          if (kind === 'cleanup-throw')
            engine.onCleanup(() => {
              throw Error('cleanup failed');
            });
          if (kind === 'construction-throw') throw failure;
          if (kind === 'construction-dispose') dispose();
        });
      } catch (error) {
        assert.equal(error, failure);
      }
      stop();
      held.push({ kind, value: stop });
      graph.push(engine.getGraphTelemetry().nodes);
      assert.equal(graph.at(-1), 0);
    });
let liveStop, liveCore, liveMemory;
await runtime.runWithIsolatedContextAsync(() => {
  const core = engine.getWasmCore();
  liveCore = new WeakRef(core);
  liveMemory = new WeakRef(core.memory);
  engine.createRoot(dispose => {
    liveStop = dispose;
    engine.computed(() => 1)();
  });
});
async function collect() {
  for (let n = 0; n < 16; n++) {
    await new Promise(setImmediate);
    global.gc();
    await new Promise(setImmediate);
  }
}
await collect();
assert(liveCore.deref());
assert(liveMemory.deref());
const liveRootControlsHeld = 2;
liveStop();
liveStop = null;
await collect();
assert.equal(liveCore.deref(), undefined);
assert.equal(liveMemory.deref(), undefined);
const retained = Object.fromEntries(
  kinds.map(kind => [
    kind,
    {
      cores: refs.filter(row => row.kind === kind && row.core.deref()).length,
      memories: refs.filter(row => row.kind === kind && row.memory.deref()).length,
    },
  ]),
);
assert.equal(controlRefs.filter(ref => ref.deref()).length, 4);
controls = null;
await collect();
assert.equal(controlRefs.filter(ref => ref.deref()).length, 0);
for (const row of held) {
  row.value();
  row.value();
}
const artifactSha256 = {};
for (const name of (await readdir(directory)).sort())
  if (/\.(js|wasm)$/.test(name))
    artifactSha256[name] = createHash('sha256')
      .update(await readFile(resolve(directory, name)))
      .digest('hex');
const report = {
  status: Object.values(retained).every(row => row.cores === 0 && row.memories === 0)
    ? 'passed'
    : 'retained',
  retained,
  graph,
  heldReferences: held.length,
  controlsHeld: 4,
  controlsReleased: 0,
  liveRootControlsHeld,
  liveRootControlsReleased: 0,
  artifactSha256,
  probeSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  node: process.version,
  scope:
    'Installed Node isolated contexts; retained createRoot disposers after explicit disposal, construction failure, in-construction disposal and throwing cleanup; forced GC.',
};
await writeFile(output, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ status: report.status, retained }));
if (report.status !== 'passed') process.exitCode = 1;
