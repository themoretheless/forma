import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { JSDOM } from 'jsdom';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
const runtime = process.argv[5]
  ? pathToFileURL(resolve(process.argv[5], 'dist/package') + '/')
  : new URL('../dist/package/', import.meta.url);
const {
  captureReactiveContext,
  createResource,
  effect,
  getCurrentOwner,
  getGraphTelemetry,
  getWasmCore,
  h,
  onCleanup,
  signal,
  createContext,
  useContext,
} = await import(new URL('index.js', runtime));
const { renderToStringAsync } = await import(new URL('server.js', runtime));
assert.equal(typeof global.gc, 'function', 'Run node --expose-gc');
const output = process.argv[2],
  durationMs = Number(process.argv[3] ?? 60000),
  width = 24;
assert(Number.isSafeInteger(durationMs) && durationMs >= 1000 && durationMs <= 3600000);
const context = createContext('missing');
let activeRequests = 0,
  peakRequests = 0,
  batches = 0;
const dom = new JSDOM('<!doctype html><html><body></body></html>');
for (const key of ['document', 'Node', 'Element']) globalThis[key] = dom.window[key];
const probes = [],
  samples = [],
  timings = {},
  windows = Array.from({ length: 4 }, () => ({ count: 0, memory: {} }));
let checked = 0,
  cleanups = 0,
  lateRefusals = 0,
  maxGcRounds = 0,
  sampleCount = 0;
const report = {
  protocol: 'async-component-concurrent-soak-v1',
  startedAt: new Date().toISOString(),
  node: process.version,
  execArgv: process.execArgv,
  durationMs,
  width,
  samples,
  timings,
  windows,
  barriers: 0,
  scope:
    '24 simultaneous isolated success/failure/cancel requests synchronized at a resource-ready barrier; real WASM + JSDOM, bounded WeakRef batches and ring memory samples. No browser/Edge/driver memory or general plateau claim.',
};
const track = (kind, object) => probes.push({ kind, ref: new WeakRef(object) });
const gate = () => {
  let release;
  const promise = new Promise(resolve => (release = resolve));
  return { promise, release };
};
function abortSignal() {
  const controller = new AbortController(),
    hooks = new Set(),
    add = controller.signal.addEventListener.bind(controller.signal),
    remove = controller.signal.removeEventListener.bind(controller.signal);
  controller.signal.addEventListener = (type, fn, options) => {
    if (type === 'abort') hooks.add(fn);
    return add(type, fn, options);
  };
  controller.signal.removeEventListener = (type, fn, options) => {
    if (type === 'abort') hooks.delete(fn);
    return remove(type, fn, options);
  };
  return { controller, hooks };
}
async function run(mode, index, barrier, cores, controllers) {
  const ready = gate(),
    resume = gate(),
    external = abortSignal(),
    failure = Error('expected failure');
  let core;
  controllers.push(external.controller);
  const Child = async () => {
    const owner = getCurrentOwner(),
      source = signal(index);
    onCleanup(source.dispose);
    onCleanup(() => cleanups++);
    let seen;
    const observer = effect(() => {
        seen = source();
      }),
      resource = createResource(async () => index),
      capture = captureReactiveContext();
    const node = h('p', null, String(index));
    for (const [kind, object] of Object.entries({
      owner,
      source,
      observer,
      resource,
      capture,
      node,
    }))
      track(kind, object);
    assert.equal(await resource.refetch(), index);
    ready.release();
    barrier.arrive();
    await barrier.promise;
    assert.equal(useContext(context), String(index));
    assert.equal(getCurrentOwner(), owner);
    assert.equal(getWasmCore(), core);
    source.set(index + 1);
    assert.equal(seen, index + 1);
    node.textContent = String(source());
    if (mode !== 'success') {
      await resume.promise;
      try {
        h('span', null, 'late');
        throw Error('Late component resumed into retired request');
      } catch (error) {
        assert.match(String(error), /completed|disposed|reset/);
        lateRefusals++;
      }
    }
    return node;
  };
  const started = performance.now();
  const pending = renderToStringAsync(
    () => {
      core = getWasmCore();
      activeRequests++;
      peakRequests = Math.max(peakRequests, activeRequests);
      cores.add(core);
      track('core', core);
      track('memory', core.memory);
      return {
        node: h(
          'section',
          null,
          h(context.Provider, { value: String(index), children: () => h(Child, null) }),
          ...(mode === 'failure'
            ? [
                h(async () => {
                  await ready.promise;
                  await barrier.promise;
                  throw failure;
                }, null),
              ]
            : []),
        ),
      };
    },
    { signal: external.controller.signal },
  );
  // Observe rejection immediately, before waiting for request entry.
  const observed = pending.then(
    value => ({ value }),
    error => ({ error }),
  );
  try {
    const earlyFailure = observed.then(result => {
      if (result.error) throw result.error;
    });
    await Promise.race([ready.promise, earlyFailure]);
    await Promise.race([barrier.promise, earlyFailure]);
    if (mode === 'cancel') external.controller.abort(failure);
    const result = await observed;
    if (mode === 'success') {
      assert.equal(result.error, undefined);
      assert.equal(result.value.html, `<section><p>${index + 1}</p></section>`);
    } else assert.equal(result.error, failure);
    const elapsed = performance.now() - started,
      stat = (timings[mode] ??= { count: 0, min: Infinity, max: 0, mean: 0 });
    stat.count++;
    stat.min = Math.min(stat.min, elapsed);
    stat.max = Math.max(stat.max, elapsed);
    stat.mean += (elapsed - stat.mean) / stat.count;
    assert.equal(external.hooks.size, 0, 'Request retained external abort hooks');
    assert.equal(core.svet_get_node_count(), 0);
    assert.equal(core.svet_pending_effect_count(), 0);
  } finally {
    resume.release();
    if (core) activeRequests--;
  }
  await new Promise(setImmediate);
}
async function sample(cycle) {
  await new Promise(setImmediate);
  let alive = [];
  for (let round = 1; round <= 10; round++) {
    global.gc();
    await new Promise(setImmediate);
    alive = probes.filter(p => p.ref.deref() !== undefined);
    maxGcRounds = Math.max(maxGcRounds, round);
    if (!alive.length) break;
    await new Promise(setImmediate);
  }
  const row = {
    cycle,
    elapsedMs: performance.now() - started,
    ...process.memoryUsage(),
    pendingProbes: probes.length,
    alive: alive.length,
  };
  if (cycle === 0) report.baseline = row;
  samples[sampleCount++ % 32] = row;
  if (cycle > 0) {
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
  if (alive.length) report.retained = alive.map(p => p.kind);
  assert.equal(alive.length, 0, 'Retired async component objects retained after ten GC turns');
  checked += probes.length;
  probes.length = 0;
}
async function batch() {
  const gateAll = gate(),
    cores = new Set(),
    controllers = [];
  let arrived = 0;
  const barrier = {
    promise: gateAll.promise,
    arrive() {
      if (++arrived === width) {
        assert.equal(activeRequests, width);
        assert.equal(cores.size, width);
        for (const core of cores) assert(core.svet_get_node_count() > 0);
        report.barriers++;
        gateAll.release();
      }
    },
  };
  const timeout = setTimeout(() => {
    gateAll.release();
    for (const controller of controllers) controller.abort(Error('Batch deadline exceeded'));
  }, 10000);
  try {
    await Promise.all(
      Array.from({ length: width }, (_, i) =>
        run(
          ['success', 'failure', 'cancel'][i % 3],
          batches * width + i,
          barrier,
          cores,
          controllers,
        ),
      ),
    );
  } catch (error) {
    gateAll.release();
    for (const controller of controllers) controller.abort(error);
    throw error;
  } finally {
    clearTimeout(timeout);
  }
}
const started = performance.now();
try {
  await sample(0);
  while (performance.now() - started < durationMs) {
    await batch();
    batches++;
    await sample(batches);
  }
  assert.equal(cleanups, batches * width);
  assert.equal(lateRefusals, (batches * width * 2) / 3);
  assert.equal(checked, batches * width * 8);
  assert.equal(activeRequests, 0);
  assert.equal(peakRequests, width);
  assert.equal(report.barriers, batches);
  assert.equal(getGraphTelemetry().nodes, 0);
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  samples.sort((a, b) => a.cycle - b.cycle);
  Object.assign(report, {
    completedAt: new Date().toISOString(),
    checkedObjects: checked,
    cleanups,
    lateRefusals,
    maxGcRounds,
    sampleCount,
    batches,
    activeRequests,
    peakRequests,
  });
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  report.runtimeSha256 = {};
  for (const file of ['server.js', 'dom.js', 'reactivity.js', 'wasm-binary.js'])
    report.runtimeSha256[file] = createHash('sha256')
      .update(await readFile(new URL(file, runtime)))
      .digest('hex');
  if (output) await writeFile(output, JSON.stringify(report, null, 2) + '\n');
  console.log(
    JSON.stringify({
      status: report.status,
      error: report.error,
      checkedObjects: checked,
      cleanups,
      lateRefusals,
      maxGcRounds,
    }),
  );
  dom.window.close();
}
