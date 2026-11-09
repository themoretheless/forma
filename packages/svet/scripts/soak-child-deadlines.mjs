import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createResumptionFixture } from './browser-fixture-endpoints.mjs';
const base = process.argv[4]
  ? pathToFileURL(resolve(process.argv[4], 'dist/package') + '/')
  : new URL('../dist/package/', import.meta.url);
const e = await import(new URL('index.js', base)),
  r = await import(new URL('server.js', base)),
  host = createResumptionFixture(e);
const durationMs = Number(process.argv[3] ?? 60000);
assert(Number.isSafeInteger(durationMs) && durationMs >= 1000 && durationMs <= 3600000);
assert.equal(typeof global.gc, 'function');
const nativeSet = globalThis.setTimeout,
  nativeClear = globalThis.clearTimeout,
  timers = new Set();
let peakTimers = 0;
globalThis.setTimeout = (callback, delay, ...args) => {
  const timer = nativeSet(() => {
    timers.delete(timer);
    callback(...args);
  }, delay);
  timers.add(timer);
  peakTimers = Math.max(peakTimers, timers.size);
  return timer;
};
globalThis.clearTimeout = timer => {
  timers.delete(timer);
  return nativeClear(timer);
};
const gate = () => {
  let release;
  const promise = new Promise(resolve => (release = resolve));
  return { promise, release };
};
let probes = [],
  held = [],
  batch = 0;
const track = (kind, value) => probes.push({ kind, ref: new WeakRef(value) });
const report = {
  startedAt: new Date().toISOString(),
  node: process.version,
  durationMs,
  concurrentParents: 8,
  childrenPerParent: 4,
  parents: 0,
  children: 0,
  collectedObjects: 0,
  lateRefusals: 0,
  samples: [],
  scope:
    'Node child-scope deadline churn, eight isolated parents with success/abort/timeout/rejection siblings. Process timeout instrumentation checks all newly scheduled handles; no Edge/browser/HTTP or long plateau claim.',
};
async function parent(id) {
  let core;
  const failures = [],
    rows = [];
  await r.runWithIsolatedContextAsync(async () => {
    core = e.getWasmCore();
    track('core', core);
    track('memory', core.memory);
    const source = e.signal(id);
    e.onCleanup(source.dispose);
    track('parentSignal', source);
    const wait = gate(),
      abort = new AbortController(),
      reason = { id, kind: 'caller' },
      rejection = { id, kind: 'rejection' };
    for (const mode of ['success', 'abort', 'timeout', 'rejection']) {
      const row = { mode, cleaned: 0, late: 0 };
      rows.push(row);
      row.promise = r
        .runWithOwnerScopeAsync(
          [],
          async () => {
            const local = e.signal(id);
            track('childSignal', local);
            e.onCleanup(local.dispose);
            e.onCleanup(() => row.cleaned++);
            await wait.promise;
            if (mode === 'abort' || mode === 'timeout') {
              assert.throws(() => e.signal(99), /completed|disposed/);
              row.late++;
              report.lateRefusals++;
              return;
            }
            assert.equal(e.getWasmCore(), core);
            assert.equal(local(), id);
            if (mode === 'rejection') throw rejection;
            return id;
          },
          {
            signal: mode === 'abort' ? abort.signal : undefined,
            timeoutMs: mode === 'timeout' ? 10 : 10000,
          },
        )
        .then(
          value => ({ value }),
          error => ({ error }),
        );
    }
    abort.abort(reason);
    const aborted = await rows[1].promise;
    assert.equal(aborted.error, reason);
    assert.equal(rows[1].cleaned, 1);
    const timed = await rows[2].promise;
    assert(timed.error instanceof r.SsrTimeoutError);
    assert.equal(timed.error.timeoutMs, 10);
    assert.equal(rows[2].cleaned, 1);
    assert.equal(rows[0].cleaned, 0);
    assert.equal(rows[3].cleaned, 0);
    source.set(id + 1);
    assert.equal(source(), id + 1);
    wait.release();
    const results = await Promise.all(rows.map(row => row.promise));
    await new Promise(setImmediate);
    assert.equal(results[0].value, id);
    assert.equal(results[3].error, rejection);
    assert.equal(rows[0].cleaned, 0);
    assert.equal(rows[3].cleaned, 1);
    assert.equal(rows[1].late, 1);
    assert.equal(rows[2].late, 1);
    assert.equal(await r.runWithOwnerScopeAsync([], () => source()), id + 1);
    failures.push(timed.error, aborted.error, results[3].error);
  });
  assert(rows.every(row => row.cleaned === 1));
  assert.equal(core.svet_get_node_count(), 0);
  held.push(failures);
  report.parents++;
  report.children += 4;
}
try {
  const started = performance.now();
  do {
    held = [];
    await Promise.all(Array.from({ length: 8 }, (_, i) => parent(batch * 8 + i)));
    batch++;
    assert.equal(timers.size, 0, 'Timeout handles survived parent teardown');
    await new Promise(setImmediate);
    let alive = [];
    for (let round = 0; round < 10; round++) {
      global.gc();
      await new Promise(setImmediate);
      alive = probes.filter(p => p.ref.deref());
      if (!alive.length) break;
      await new Promise(setImmediate);
    }
    assert.equal(alive.length, 0, 'Saved child errors retained request objects');
    report.collectedObjects += probes.length;
    probes = [];
    report.samples[(batch - 1) % 32] = {
      elapsedMs: performance.now() - started,
      parents: report.parents,
      ...process.memoryUsage(),
    };
  } while (performance.now() - started < durationMs);
  report.elapsedMs = performance.now() - started;
  report.activeTimers = timers.size;
  report.peakTimers = peakTimers;
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  for (const timer of timers) nativeClear(timer);
  globalThis.setTimeout = nativeSet;
  globalThis.clearTimeout = nativeClear;
  held = [];
  host.close();
  report.completedAt = new Date().toISOString();
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  report.runtimeSha256 = createHash('sha256')
    .update(await readFile(new URL('ssr-runtime.js', base)))
    .digest('hex');
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(
    JSON.stringify({
      status: report.status,
      parents: report.parents,
      children: report.children,
      error: report.error,
    }),
  );
}
