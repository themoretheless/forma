import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import {
  createWasmCoreInstance,
  withWasmCore,
  createRoot,
  signal,
  effect,
  onCleanup,
  createResource,
  captureReactiveContext,
  getGraphTelemetry,
  getCurrentOwner,
  createServerAction,
  createTransform3D,
  createPerspectiveCamera3D,
  createMesh3D,
} from '../dist/package/index.js';
assert.equal(typeof global.gc, 'function', 'Use node --expose-gc');
const output = process.argv[2],
  durationMs = Number(process.argv[3] ?? 120000),
  scenario = process.argv[4] ?? 'roots';
assert(['roots', 'actions-scenes'].includes(scenario));
const integrated = scenario === 'actions-scenes',
  settlements = [];
let actionCompleted = 0,
  actionCancelled = 0,
  actionAborts = 0;
assert(Number.isSafeInteger(durationMs) && durationMs >= 1000 && durationMs <= 3600000);
const cores = [createWasmCoreInstance(), createWasmCoreInstance()],
  live = [],
  probes = [],
  samples = [];
let serial = 0,
  cleanups = 0,
  aborts = 0,
  collected = 0,
  sampleCount = 0,
  maxBacklog = 0,
  maxAgeMs = 0,
  batches = 0;
const windows = Array.from({ length: 4 }, () => ({ count: 0, memory: {} }));
const report = {
  protocol: 'timed-retention-soak-v1',
  startedAt: new Date().toISOString(),
  node: process.version,
  execArgv: process.execArgv,
  durationMs,
  scenario,
  objectsPerRoot: integrated ? 8 : 4,
  liveRoots: 64,
  samples,
  windows,
  limits: { retainedSamples: 64, pendingWeakRefs: 10000 },
  scope:
    'Separate timed retention observation, one GC/sample. Does not replace strict ten-GC gate. No DOM/GPU or universal plateau proof.',
};
const inspect = () =>
  cores.map(core =>
    withWasmCore(core, () => ({
      ...getGraphTelemetry(),
      arenaFloats: core.svet_gpu_get_arena_len(),
    })),
  );
let baseline;
function mount() {
  const id = serial++,
    core = cores[id % 2];
  let stop, owner, input, resource, context, action, transform, camera, mesh, ready, cancelled;
  withWasmCore(core, () =>
    createRoot(dispose => {
      stop = dispose;
      owner = getCurrentOwner();
      input = signal(id);
      onCleanup(() => input.dispose());
      effect(() => {
        assert.equal(input(), id);
      });
      resource = createResource(({ signal }) => {
        signal.addEventListener('abort', () => aborts++, { once: true });
        return new Promise(() => {});
      });
      context = captureReactiveContext();
      onCleanup(() => cleanups++);
      if (integrated) {
        action = createServerAction({
          rpc: (arg, signal) => {
            if (arg >= 0) return Promise.resolve(arg * 2);
            signal.addEventListener('abort', () => actionAborts++, { once: true });
            return new Promise(() => {});
          },
        });
        ready = action(id).then(value => {
          assert.equal(value, id * 2);
          actionCompleted++;
        });
        cancelled = action(-1).then(
          () => {
            throw Error('Cancelled action resolved');
          },
          error => {
            assert.equal(error.name, 'ServerActionDisposedError');
            actionCancelled++;
          },
        );
        transform = createTransform3D();
        camera = createPerspectiveCamera3D();
        mesh = createMesh3D({
          transform,
          camera,
          positions: [0, 0, 0, 1, 0, 0, 0, 1, 0],
          indices: [0, 1, 2],
        });
        transform.position.set([id % 17, 0, -2]);
        assert.equal(mesh.transformedVertices().length, 12);
      }
    }),
  );
  return {
    id,
    core,
    stop,
    owner,
    input,
    resource,
    context,
    action,
    transform,
    camera,
    mesh,
    ready,
    cancelled,
  };
}
function retire(entry) {
  for (const kind of integrated
    ? ['owner', 'input', 'resource', 'context', 'action', 'transform', 'camera', 'mesh']
    : ['owner', 'input', 'resource', 'context'])
    probes.push({
      id: entry.id,
      kind,
      retiredAt: performance.now(),
      ref: new WeakRef(entry[kind]),
    });
  withWasmCore(entry.core, entry.stop);
  assert.equal(entry.context.active, false);
  if (integrated) settlements.push(entry.cancelled);
}
async function rotate() {
  for (let i = 0; i < 16; i++) {
    const fresh = mount();
    if (integrated) await fresh.ready;
    retire(live.shift());
    live.push(fresh);
  }
}

async function sample(final = false) {
  if (integrated) await Promise.all(settlements.splice(0));
  await new Promise(setImmediate);
  global.gc();
  await new Promise(setImmediate);
  const now = performance.now();
  let write = 0,
    oldest = 0;
  for (let i = 0; i < probes.length; i++) {
    const probe = probes[i];
    if (probe.ref.deref() === undefined) {
      collected++;
      continue;
    }
    probes[write++] = probe;
    oldest = Math.max(oldest, now - probe.retiredAt);
  }
  probes.length = write;
  maxBacklog = Math.max(maxBacklog, write);
  maxAgeMs = Math.max(maxAgeMs, oldest);
  const row = {
    batches,
    elapsedMs: now - started,
    ...process.memoryUsage(),
    backlog: write,
    oldestRetainedMs: oldest,
    cores: inspect(),
    final,
  };
  samples[sampleCount++ % 64] = row;
  if (!baseline) {
    baseline = row;
    report.baseline = row;
  }
  if (!final) {
    row.cores.forEach((state, i) => {
      for (const key of [
        'nodes',
        'edges',
        'capacity',
        'wasmMemoryBytes',
        'pendingEffectCount',
        'arenaFloats',
      ])
        assert.equal(state[key], baseline.cores[i][key]);
    });
    const window = windows[Math.min(3, Math.floor((row.elapsedMs * 4) / durationMs))];
    window.count++;
    for (const key of ['rss', 'heapUsed', 'external', 'arrayBuffers']) {
      const stat = (window.memory[key] ??= {
        min: Infinity,
        max: -Infinity,
        mean: 0,
        first: row[key],
        last: 0,
      });
      stat.min = Math.min(stat.min, row[key]);
      stat.max = Math.max(stat.max, row[key]);
      stat.mean += (row[key] - stat.mean) / window.count;
      stat.last = row[key];
    }
  }
  assert(probes.length <= 10000, 'Pending retired-object budget exceeded');
}
const started = performance.now();
try {
  for (let i = 0; i < 64; i++) live.push(mount());
  if (integrated) await Promise.all(live.map(entry => entry.ready));
  await rotate();
  await sample();
  while (performance.now() - started < durationMs) {
    await rotate();
    batches++;
    if (batches % 25 === 0) await sample();
  }
  report.loadEndedMs = performance.now() - started;
  while (live.length) retire(live.pop());
  const drainStart = performance.now();
  do {
    await sample(true);
    if (!probes.length) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  } while (performance.now() - drainStart < 10000);
  assert.equal(probes.length, 0, 'Objects remain after ten-second final drain');
  for (const state of inspect()) {
    assert.equal(state.nodes, 0);
    assert.equal(state.edges, 0);
    assert.equal(state.pendingEffectCount, 0);
  }
  assert.equal(cleanups, serial);
  assert.equal(aborts, serial);
  assert.equal(collected, serial * (integrated ? 8 : 4));
  if (integrated) {
    assert.equal(actionCompleted, serial);
    assert.equal(actionCancelled, serial);
    assert.equal(actionAborts, serial);
  }
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  samples.sort((a, b) => a.elapsedMs - b.elapsedMs);
  Object.assign(report, {
    completedAt: new Date().toISOString(),
    createdRoots: serial,
    cleanups,
    aborts,
    collectedObjects: collected,
    pendingObjects: probes.length,
    maxBacklog,
    maxAgeMs,
    sampleCount,
    batches,
    actionCompleted,
    actionCancelled,
    actionAborts,
  });
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  report.runtimeSha256 = {};
  for (const file of (await readdir(new URL('../dist/package/', import.meta.url)))
    .filter(name => name.endsWith('.js'))
    .sort())
    report.runtimeSha256[file] = createHash('sha256')
      .update(await readFile(new URL('../dist/package/' + file, import.meta.url)))
      .digest('hex');
  if (output) await writeFile(output, JSON.stringify(report, null, 2) + '\n');
  console.log(
    JSON.stringify({
      status: report.status,
      error: report.error,
      createdRoots: serial,
      collectedObjects: collected,
      pendingObjects: probes.length,
      maxBacklog,
      maxAgeMs,
      sampleCount,
    }),
  );
}
