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
} = await import(new URL('index.js', runtime));
const { renderToStringAsync } = await import(new URL('server.js', runtime));
assert.equal(typeof global.gc, 'function', 'Run node --expose-gc');
const output = process.argv[2],
  cycles = Number(process.argv[3] ?? 200),
  retireCache = process.argv[4] !== 'native';
assert(Number.isSafeInteger(cycles) && cycles >= 10 && cycles <= 10000 && cycles % 10 === 0);
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
  protocol: 'async-component-churn-v1',
  startedAt: new Date().toISOString(),
  node: process.version,
  execArgv: process.execArgv,
  cycles,
  samples,
  timings,
  windows,
  retireCache,
  scope:
    'Fresh isolated request per success/failure/cancel; real WASM + JSDOM, bounded WeakRef batches and ring memory samples. No browser/Edge/driver memory or general plateau claim.',
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
async function run(mode, index) {
  const ready = gate(),
    resume = gate(),
    external = abortSignal(),
    failure = Error('expected failure');
  let core;
  const Child = async () => {
    const owner = getCurrentOwner(),
      source = signal(index);
    onCleanup(source.dispose);
    onCleanup(() => cleanups++);
    const observer = effect(() => {
        assert.equal(source(), index);
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
      track('core', core);
      track('memory', core.memory);
      return {
        node: h(
          'section',
          null,
          h(Child, null),
          ...(mode === 'failure'
            ? [
                h(async () => {
                  await ready.promise;
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
  await ready.promise;
  if (mode === 'cancel') external.controller.abort(failure);
  const result = await observed;
  if (mode === 'success') assert.equal(result.value.html, `<section><p>${index}</p></section>`);
  else assert.equal(result.error, failure);
  const elapsed = performance.now() - started,
    stat = (timings[mode] ??= { count: 0, min: Infinity, max: 0, mean: 0 });
  stat.count++;
  stat.min = Math.min(stat.min, elapsed);
  stat.max = Math.max(stat.max, elapsed);
  stat.mean += (elapsed - stat.mean) / stat.count;
  assert.equal(external.hooks.size, 0, 'Request retained external abort hooks');
  assert.equal(core.svet_get_node_count(), 0);
  assert.equal(core.svet_pending_effect_count(), 0);
  resume.release();
  await new Promise(setImmediate);
}
async function sample(cycle) {
  // The runtime intentionally caches one last context; retire the measured core
  // from that bounded cache by observing the process ambient graph first.
  if (retireCache) assert.equal(getGraphTelemetry().nodes, 0);
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
    ...process.memoryUsage(),
    pendingProbes: probes.length,
    alive: alive.length,
  };
  if (cycle === 0) report.baseline = row;
  samples[sampleCount++ % 32] = row;
  if (cycle > 0) {
    const window = windows[Math.min(3, Math.floor(((cycle - 1) * 4) / cycles))];
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
try {
  await sample(0);
  for (let cycle = 0; cycle < cycles; cycle++) {
    for (const mode of ['success', 'failure', 'cancel']) await run(mode, cycle);
    if ((cycle + 1) % 10 === 0) await sample(cycle + 1);
  }
  assert.equal(cleanups, cycles * 3);
  assert.equal(lateRefusals, cycles * 2);
  assert.equal(checked, cycles * 3 * 8);
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
