import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
assert.equal(typeof global.gc, 'function', 'Run with --expose-gc');
assert(process.argv[2] && process.argv[3], 'Expected output and independent-copy manifest');
const manifest = JSON.parse(await readFile(resolve(process.argv[3]), 'utf8'));
const build = JSON.parse(await readFile(resolve(manifest.output, 'metadata.json'), 'utf8'));
for (const [path, sha] of Object.entries(build.artifactSha256)) {
  assert.equal(createHash('sha256').update(await readFile(resolve(manifest.output, path))).digest('hex'), sha);
}
const a = await import(pathToFileURL(resolve(manifest.output, 'first/index.js')).href);
const b = await import(pathToFileURL(resolve(manifest.output, 'second/index.js')).href);
const report = { startedAt: new Date().toISOString(), node: process.version, build, cases: [],
  scope: 'Explicit teardown of independent-copy native bridges; forced-GC reachability proof, not a natural-GC memory plateau or disposed-handle retention contract.' };
const persistentCore = a.createWasmCoreInstance();
const persistentSource = a.withWasmCore(persistentCore, () => a.signal(0));
let controls = Array.from({ length: 4 }, () => ({ bytes: new Uint8Array(4096) }));
const controlRefs = controls.map(value => new WeakRef(value));
const targets = [];
function track(kind, value) { targets.push({ kind, ref: new WeakRef(value) }); }
function allocate(mode) {
  const sourceCore = mode === 'long-lived-source' ? persistentCore : a.createWasmCoreInstance();
  const targetCore = b.createWasmCoreInstance();
  if (mode !== 'long-lived-source') { track('sourceCore', sourceCore); track('sourceMemory', sourceCore.memory); }
  track('targetCore', targetCore); track('targetMemory', targetCore.memory);
  const payload = { bytes: new Uint8Array(65536) }; track('payload', payload); track('buffer', payload.bytes.buffer);
  const source = mode === 'long-lived-source' ? persistentSource : a.withWasmCore(sourceCore, () => a.signal(0));
  const errorMode = ['computed-error', 'initial-computed-error', 'uncaught-initial-computed-error'].includes(mode);
  const failure = errorMode ? { reason: 'controlled foreign failure', payload } : null;
  if (errorMode) track('failure', failure);
  const uncaught = mode === 'uncaught-initial-computed-error';
  if (mode === 'initial-computed-error' || uncaught) source.set(1);
  const value = errorMode ? a.withWasmCore(sourceCore, () => a.computed(() => {
    if (source() % 2) throw failure;
    return 7;
  })) : source;
  const gate = b.withWasmCore(targetCore, () => b.signal(true));
  const derived = b.withWasmCore(targetCore, () => b.computed(() => gate() ? value() + payload.bytes.length : 0));
  let runs = 0;
  let failures = 0;
  const makeObserver = () => b.withWasmCore(targetCore, () => b.effect(() => {
    try { derived(); } catch (error) { if (!errorMode || uncaught) throw error; assert.equal(error, failure); failures++; }
    runs++;
  }));
  let observer;
  if (uncaught) assert.throws(makeObserver, error => error === failure);
  else observer = makeObserver();
  if (uncaught) { source.set(2); assert.equal(runs, 0); }
  else if (errorMode) {
    if (mode === 'computed-error') source.set(1);
    assert.equal(failures, 1);
    source.set(2);
    assert.equal(runs, mode === 'computed-error' ? 3 : 2);
  } else if (mode === 'branch-switch') {
    gate.set(false); const previous = runs; source.set(1); assert.equal(runs, previous);
  } else if (mode === 'source-reset') a.withWasmCore(sourceCore, () => a.resetReactiveSystem());
  else if (mode === 'target-reset') b.withWasmCore(targetCore, () => b.resetReactiveSystem());
  else if (mode === 'throwing-batch') {
    const failure = Error('controlled batch failure');
    assert.throws(() => a.withWasmCore(sourceCore, () => a.batch(() => { source.set(1); throw failure; })), error => error === failure);
  } else { source.set(source.peek() + 1); assert.equal(runs, 2); }
  observer?.dispose(); derived.dispose(); gate.dispose();
  if (errorMode) value.dispose();
  if (mode !== 'long-lived-source') source.dispose();
  assert.equal(b.withWasmCore(targetCore, () => b.getGraphTelemetry().nodes), 0);
  assert.equal(a.withWasmCore(sourceCore, () => a.getGraphTelemetry().nodes), mode === 'long-lived-source' ? 1 : 0);
}
for (const mode of ['dispose', 'branch-switch', 'source-reset', 'target-reset', 'throwing-batch', 'long-lived-source', 'computed-error', 'initial-computed-error', 'uncaught-initial-computed-error']) {
  const start = targets.length;
  for (let i = 0; i < 8; i++) allocate(mode);
  report.cases.push({ mode, targets: targets.length - start });
}
// Replace both copies' documented hot context caches with the live control core.
for (const engine of [a, b]) engine.withWasmCore(persistentCore, () => engine.getGraphTelemetry());
async function collect() {
  for (let i = 0; i < 24; i++) {
    await new Promise(resolve => setImmediate(resolve));
    global.gc();
    await new Promise(resolve => setImmediate(resolve));
  }
}
let liveSubscription = (() => {
  const core = b.createWasmCoreInstance(), payload = { bytes: new Uint8Array(1024) };
  const observer = b.withWasmCore(core, () => b.effect(() => { persistentSource(); return () => { payload.bytes.fill(0); }; }));
  return { refs: [new WeakRef(core), new WeakRef(core.memory), new WeakRef(payload)], dispose: () => observer.dispose() };
})();
const subscriptionRefs = liveSubscription.refs;
for (const engine of [a, b]) engine.withWasmCore(persistentCore, () => engine.getGraphTelemetry());
await collect();
report.liveSubscriptionHeld = subscriptionRefs.filter(ref => ref.deref()).length;
assert.equal(report.liveSubscriptionHeld, 3, 'An active bridge must keep its live subscription reachable');
const alive = targets.filter(target => target.ref.deref()).map(target => target.kind);
report.retention = { targets: targets.length, alive, controlsHeld: controlRefs.filter(ref => ref.deref()).length };
assert.deepEqual(alive, [], 'Teardown retained native bridge objects');
assert.equal(report.retention.controlsHeld, 4, 'Positive GC controls must remain reachable');
assert.equal(a.withWasmCore(persistentCore, () => a.getGraphTelemetry().nodes), 2);
persistentSource.set(2); assert.equal(persistentSource(), 2);
liveSubscription.dispose(); liveSubscription = null;
for (const engine of [a, b]) engine.withWasmCore(persistentCore, () => engine.getGraphTelemetry());
persistentSource.dispose();
controls = null;
await collect();
report.liveSubscriptionReleased = subscriptionRefs.filter(ref => ref.deref()).length;
assert.equal(report.liveSubscriptionReleased, 0);
report.retention.controlsReleased = controlRefs.filter(ref => ref.deref()).length;
assert.equal(report.retention.controlsReleased, 0);
report.status = 'passed'; report.completedAt = new Date().toISOString();
report.probeSha256 = createHash('sha256').update(await readFile(new URL(import.meta.url))).digest('hex');
await writeFile(resolve(process.argv[2]), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ status: report.status, cases: report.cases, retention: report.retention }, null, 2));
