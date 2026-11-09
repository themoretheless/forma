import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import {
  captureReactiveContext,
  computed,
  createResource,
  effect,
  getCurrentOwner,
  getGraphTelemetry,
  getWasmCore,
  onCleanup,
  runWithOwnerScope,
} from '../dist/package/index.js';
import { runWithIsolatedContextAsync, runWithOwnerScopeAsync } from '../dist/package/server.js';

assert.equal(typeof global.gc, 'function', 'Run with node --expose-gc after build:package');
const cycles = Number(process.argv[3] ?? 1000);
assert(Number.isSafeInteger(cycles) && cycles >= 100 && cycles <= 100000 && cycles % 50 === 0);
const samples = [],
  probes = [];
let checkedObjects = 0,
  maxGcRounds = 0,
  cleanups = 0,
  peakScopes = 0;
let requestCore, warmed;
function trackedSignal() {
  const controller = new AbortController(),
    callbacks = new Set();
  const add = controller.signal.addEventListener.bind(controller.signal);
  const remove = controller.signal.removeEventListener.bind(controller.signal);
  controller.signal.addEventListener = (type, callback, options) => {
    if (type === 'abort') callbacks.add(callback);
    return add(type, callback, options);
  };
  controller.signal.removeEventListener = (type, callback, options) => {
    if (type === 'abort') callbacks.delete(callback);
    return remove(type, callback, options);
  };
  return { controller, callbacks };
}
function gate() {
  let release;
  const promise = new Promise(resolve => {
    release = resolve;
  });
  return { promise, release };
}
async function createResources(value) {
  const owner = getCurrentOwner(),
    memo = computed(() => value);
  const observer = effect(() => {
    assert.equal(memo(), value);
  });
  const resource = createResource(async () => value);
  assert.equal(await resource.refetch(), value);
  const captured = captureReactiveContext();
  onCleanup(() => cleanups++);
  for (const [kind, object] of Object.entries({ owner, memo, observer, resource, captured })) {
    probes.push({ kind, ref: new WeakRef(object) });
  }
}
async function cycle(index, parent) {
  const successSignal = trackedSignal();
  const successful = runWithOwnerScope([], stop => ({
    stop,
    pending: runWithOwnerScopeAsync([], () => createResources(index), {
      signal: successSignal.controller.signal,
    }),
  }));
  await successful.pending;
  peakScopes = Math.max(peakScopes, parent.children?.size ?? 0);
  assert.equal(successSignal.callbacks.size, 0, 'Settled child retained external abort hook');
  successful.stop();

  const failedSignal = trackedSignal(),
    failure = Error('expected child failure');
  await assert.rejects(
    runWithOwnerScopeAsync(
      [],
      async () => {
        await createResources(index);
        throw failure;
      },
      { signal: failedSignal.controller.signal },
    ),
    error => error === failure,
  );
  assert.equal(failedSignal.callbacks.size, 0, 'Rejected child retained external abort hook');

  const cancelledSignal = trackedSignal(),
    started = gate(),
    wait = gate();
  const pending = runWithOwnerScopeAsync(
    [],
    async () => {
      await createResources(index);
      started.release();
      await wait.promise;
    },
    { signal: cancelledSignal.controller.signal },
  );
  await started.promise;
  const reason = Error('expected child cancellation');
  const rejected = assert.rejects(pending, error => error === reason);
  cancelledSignal.controller.abort(reason);
  await rejected;
  assert.equal(cancelledSignal.callbacks.size, 0, 'Cancelled child retained external abort hook');
  wait.release();
  await new Promise(setImmediate);
  assert.equal(parent.children?.size ?? 0, 0, 'Retired child remains attached to request');
  const graph = getGraphTelemetry();
  assert.equal(graph.nodes, 0);
  assert.equal(graph.edges, 0);
  assert.equal(graph.pendingEffectCount, 0);
}
async function sample(cycle) {
  await new Promise(setImmediate);
  let alive;
  for (let round = 1; round <= 10; round++) {
    global.gc();
    await new Promise(setImmediate);
    alive = probes.filter(({ ref }) => ref.deref() !== undefined).map(({ kind }) => kind);
    maxGcRounds = Math.max(maxGcRounds, round);
    if (!alive.length) break;
    await new Promise(setImmediate);
  }
  assert.deepEqual(alive, [], 'Retired async child objects remain reachable');
  checkedObjects += probes.length;
  probes.length = 0;
  const current = { cycle, ...process.memoryUsage(), graph: getGraphTelemetry() };
  if (cycle === 50) warmed = current;
  if (warmed) {
    assert.equal(current.graph.capacity, warmed.graph.capacity);
    assert.equal(current.graph.wasmMemoryBytes, warmed.graph.wasmMemoryBytes);
  }
  samples.push(current);
  if (samples.length > 32) samples.shift();
}
await runWithIsolatedContextAsync(async () => {
  requestCore = getWasmCore();
  const parent = getCurrentOwner();
  const initialDisposables = parent.disposables.length;
  for (let index = 1; index <= cycles; index++) {
    await cycle(index, parent);
    assert.equal(parent.disposables.length, initialDisposables);
    if (index % 50 === 0) await sample(index);
  }
});
assert.equal(requestCore.svet_get_node_count(), 0);
assert.equal(getGraphTelemetry().nodes, 0);
assert.equal(cleanups, cycles * 3);
const result = {
  measuredAt: new Date().toISOString(),
  node: process.version,
  execArgv: process.execArgv,
  probeSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  cycles,
  childScopes: cycles * 3,
  checkedObjects,
  maxGcRounds,
  cleanups,
  peakScopes,
  warmed,
  samples,
  scope:
    'Successful, rejected and cancelled async children within one live Node request. ' +
    'Checks graph teardown, owner unlinking, external abort hooks, WeakRef collection and stable native capacity. ' +
    'JS/RSS samples are observations, not a proof of global memory plateau or GPU/DOM reclamation.',
};
if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
