import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { cpus } from 'node:os';
import { writeHeapSnapshot } from 'node:v8';
import {
  createWasmCoreInstance,
  withWasmCore,
  createRoot,
  signal,
  effect,
  onCleanup,
  captureReactiveContext,
  createResource,
  createServerAction,
  getGraphTelemetry,
  createTransform3D,
  createPerspectiveCamera3D,
  createMesh3D,
  getCurrentOwner,
} from '../dist/package/index.js';

assert.equal(typeof global.gc, 'function', 'Run node --expose-gc after build:package');
const cores = [createWasmCoreInstance(), createWasmCoreInstance()];
const cycles = Number(process.argv[3] ?? 2000);
assert(
  Number.isSafeInteger(cycles) && cycles >= 100 && cycles <= 1000000 && cycles % 100 === 0,
  'Cycle count must be a multiple of 100 in [100, 1000000]',
);
let cleanups = 0,
  resourceAborts = 0,
  actionAborts = 0,
  effectRuns = 0;
let lastCaptures = [];
const samples = [];
const probes = [];
let checkedCollectibleObjects = 0,
  sampleCount = 0,
  maxGcRounds = 0,
  initialSample,
  warmedSample;
const inspect = core =>
  withWasmCore(core, () => ({
    ...getGraphTelemetry(),
    arenaFloats: core.svet_gpu_get_arena_len(),
  }));
async function sample(cycles) {
  await new Promise(setImmediate);
  let alive = [];
  // Allow multiple collection/event-loop turns. Never dereference in the job
  // immediately before GC: WeakRef keeps that target alive for the job.
  for (let round = 0; round < 10; round++) {
    global.gc();
    await new Promise(setImmediate);
    alive = probes.filter(({ ref }) => ref.deref() !== undefined);
    maxGcRounds = Math.max(maxGcRounds, round + 1);
    if (!alive.length) break;
    await new Promise(setImmediate);
  }
  if (alive.length && process.env.SVET_LIFECYCLE_HEAP_SNAPSHOT) {
    writeHeapSnapshot(process.env.SVET_LIFECYCLE_HEAP_SNAPSHOT);
  }
  if (alive.length && process.env.SVET_LIFECYCLE_GC_DIAGNOSTICS) {
    const observations = [{ idleMilliseconds: 0, alive: alive.map(({ kind }) => kind) }];
    for (let round = 1; round <= 4; round++) {
      await new Promise(resolve => setTimeout(resolve, 250));
      global.gc();
      await new Promise(setImmediate);
      const remaining = probes.filter(({ ref }) => ref.deref() !== undefined);
      observations.push({
        idleMilliseconds: round * 250,
        alive: remaining.map(({ kind }) => kind),
      });
      if (!remaining.length) break;
    }
    await writeFile(
      process.env.SVET_LIFECYCLE_GC_DIAGNOSTICS,
      JSON.stringify(
        {
          cycles,
          node: process.version,
          execArgv: process.execArgv,
          originalGcLimit: 10,
          observations,
          cores: cores.map(inspect),
          scope: 'Diagnostic idle observations only; the original ten-GC assertion still fails.',
        },
        null,
        2,
      ) + '\n',
    );
  }
  assert.deepEqual(
    alive.map(({ kind }) => kind),
    [],
    'Disposed objects remain reachable',
  );
  checkedCollectibleObjects += probes.length;
  probes.length = 0;
  const current = { cycles, ...process.memoryUsage(), cores: cores.map(inspect) };
  if (cycles === 0) initialSample = current;
  if (cycles === 100) warmedSample = current;
  if (warmedSample)
    current.cores.forEach((state, i) => {
      assert.equal(state.capacity, warmedSample.cores[i].capacity);
      assert.equal(state.wasmMemoryBytes, warmedSample.cores[i].wasmMemoryBytes);
      assert.equal(state.arenaFloats, warmedSample.cores[i].arenaFloats);
    });
  // Bounded recording prevents the probe itself growing with cycle count.
  samples[sampleCount++ % 32] = current;
}
function track(kind, object) {
  probes.push({ kind, ref: new WeakRef(object) });
}
async function cycle(core, value) {
  let stop, input, resource, pendingResource, action, captures;
  withWasmCore(core, () =>
    createRoot(dispose => {
      stop = dispose;
      track(`owner:${value}`, getCurrentOwner());
      input = signal(value);
      // Bare signals are independent sources; explicitly give this one to the root.
      onCleanup(() => input.dispose());
      track('input', input);
      track(
        'effect',
        effect(() => {
          input();
          effectRuns++;
        }),
      );
      onCleanup(() => {
        cleanups++;
      });
      captures = Array.from({ length: 4 }, () => captureReactiveContext());
      resource = createResource(async () => value);
      track('resource', resource);
      pendingResource = createResource(({ signal }) => {
        signal.addEventListener(
          'abort',
          () => {
            resourceAborts++;
          },
          { once: true },
        );
        return new Promise(() => {});
      });
      track('pendingResource', pendingResource);
      action = createServerAction({
        rpc: (arg, signal) => {
          if (arg >= 0) return Promise.resolve(arg * 2);
          signal.addEventListener(
            'abort',
            () => {
              actionAborts++;
            },
            { once: true },
          );
          return new Promise(() => {});
        },
      });
      track('action', action);
      const transform = createTransform3D();
      const camera = createPerspectiveCamera3D();
      const mesh = createMesh3D({
        transform,
        camera,
        positions: [0, 0, 0, 1, 0, 0, 0, 1, 0],
        indices: [0, 1, 2],
      });
      track('transform', transform);
      track('camera', camera);
      track('mesh', mesh);
      transform.position.set([value % 17, 0, -2]);
      assert.equal(mesh.transformedVertices().length, 12);
      input.set(value + 1);
    }),
  );
  const completed = withWasmCore(core, () => Promise.all([resource.refetch(), action(value)]));
  assert.deepEqual(await completed, [value, value * 2]);
  const cancelledResource = withWasmCore(core, () => pendingResource.refetch());
  const cancelledAction = withWasmCore(core, () => action(-1)).then(
    () => {
      throw Error('Disposed action resolved');
    },
    error => error.name,
  );
  captures[0].dispose();
  withWasmCore(core, stop);
  assert.equal(await cancelledResource, undefined);
  assert.equal(await cancelledAction, 'ServerActionDisposedError');
  assert(captures.every(context => !context.active));
  lastCaptures = captures; // Retain disposed tokens only until the next cycle.
  const state = inspect(core);
  assert.equal(state.nodes, 0);
  assert.equal(state.edges, 0);
  assert.equal(state.pendingEffectCount, 0);
}
await sample(0);
for (let i = 0; i < cycles; i++) {
  await cycle(cores[i % 2], i);
  if ((i + 1) % 100 === 0) await sample(i + 1);
}
assert.equal(cleanups, cycles);
assert.equal(effectRuns, cycles * 2);
assert.equal(resourceAborts, cycles * 2); // refetch supersedes initial, then owner teardown
assert.equal(actionAborts, cycles);
assert(lastCaptures.every(context => !context.active));
assert.equal(checkedCollectibleObjects, cycles * 9);
const result = {
  measuredAt: new Date().toISOString(),
  node: process.version,
  execArgv: process.execArgv,
  platform: process.platform,
  arch: process.arch,
  cpu: cpus()[0]?.model,
  embeddedModuleSha256: createHash('sha256')
    .update(await readFile(new URL('../dist/package/wasm-binary.js', import.meta.url)))
    .digest('hex'),
  probeSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  cycles,
  isolatedCores: 2,
  cleanups,
  effectRuns,
  resourceAborts,
  actionAborts,
  recording: {
    maxRetainedSamples: 34,
    ringSamples: 32,
    baselineSamples: 2,
    maxPendingWeakRefs: 900,
    maxGcRounds,
    sampleCount,
  },
  checkedCollectibleObjects,
  initialSample,
  warmedSample,
  samples: samples.sort((a, b) => a.cycles - b.cycles),
  scope:
    'Node real WASM; alternating cores, no reset. Settled and ' +
    'uncooperative cancelled work, captured tokens and CPU scene construction. ' +
    'No DOM, physical GPU, parallel pending roots or long-term RSS guarantee.',
};
if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
