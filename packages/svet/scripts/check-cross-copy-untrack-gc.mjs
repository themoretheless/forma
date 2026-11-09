import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { createHash } from 'node:crypto';
assert.equal(typeof global.gc, 'function', 'Use node --expose-gc');
const latest = JSON.parse(
  await readFile(
    process.argv[3]
      ? resolve(process.argv[3])
      : new URL('../node_modules/.cache/svet-event-instances/latest.json', import.meta.url),
    'utf8',
  ),
);
const frozenBuild = JSON.parse(await readFile(resolve(latest.output, 'metadata.json'), 'utf8'));
for (const [path, expected] of Object.entries(frozenBuild.artifactSha256)) {
  assert.equal(
    createHash('sha256')
      .update(await readFile(resolve(latest.output, path)))
      .digest('hex'),
    expected,
    'Frozen artifact changed: ' + path,
  );
}
const copies = await Promise.all(
  ['first', 'second'].map(
    name => import(pathToFileURL(resolve(latest.output, name, 'index.js')).href),
  ),
);
const sharedCore = process.argv[4] === 'shared-core';
assert(process.argv[4] === undefined || sharedCore, 'Unknown GC profile');
const references = [];
let controls = Array.from({ length: 4 }, () => ({ control: true }));
const controlRefs = controls.map(value => new WeakRef(value));
function allocate(own, foreign) {
  const core = own.createWasmCoreInstance();
  own.withWasmCore(core, () => {
    const source = own.signal(0);
    let fresh, derived, inner, seen;
    const observer = own.effect(() => {
      source();
      foreign.untrack(() => {
        own.resetReactiveSystem();
        if (sharedCore) {
          fresh = foreign.withWasmCore(core, () => foreign.signal(1));
          derived = own.computed(() => fresh() * 2);
          inner = foreign.withWasmCore(core, () =>
            foreign.effect(() => {
              seen = derived();
            }),
          );
        } else {
          fresh = own.signal(1);
          inner = own.effect(() => fresh());
        }
      });
    });
    fresh.set(2);
    assert.equal(fresh(), 2);
    if (sharedCore) assert.equal(seen, 4);
    observer.dispose();
    inner.dispose();
    derived?.dispose();
    fresh.dispose();
    source.dispose();
    assert.equal(own.getGraphTelemetry().nodes, 0);
  });
  own.getGraphTelemetry();
  foreign.getGraphTelemetry();
  references.push(
    { kind: 'core', ref: new WeakRef(core) },
    { kind: 'memory', ref: new WeakRef(core.memory) },
  );
}
for (let cycle = 0; cycle < 12; cycle++)
  for (let i = 0; i < 2; i++) allocate(copies[i], copies[1 - i]);
assert.equal(globalThis[Symbol.for('svet.reactivity.domains.v1')]?.frames.length ?? 0, 0);
assert.equal(
  globalThis[Symbol.for('svet.reactivity.observation-copies.v2')].reduce(
    (count, ref) => count + (ref.deref()?.length ?? 0),
    0,
  ),
  0,
);
async function collect() {
  let alive;
  for (let round = 0; round < 10; round++) {
    await new Promise(setImmediate);
    global.gc();
    await new Promise(setImmediate);
    alive = references.filter(row => row.ref.deref()).map(row => row.kind);
    if (!alive.length) break;
  }
  return alive;
}
const held = await collect();
assert.deepEqual(held, []);
assert.equal(controlRefs.filter(ref => ref.deref()).length, 4);
controls = undefined;
await new Promise(setImmediate);
let controlsAlive = 4;
for (let round = 0; round < 10; round++) {
  global.gc();
  await new Promise(setImmediate);
  controlsAlive = controlRefs.filter(ref => ref.deref()).length;
  if (!controlsAlive) break;
  await new Promise(setImmediate);
}
assert.equal(controlsAlive, 0);
const report = {
  status: 'passed',
  profile: sharedCore ? 'shared-core-reset-computed' : 'separate-core-untrack',
  node: process.version,
  completedAt: new Date().toISOString(),
  snapshot: latest.snapshot,
  coresCollected: 24,
  memoriesCollected: 24,
  controlsHeld: 4,
  controlsReleased: controlsAlive,
  liveContextRegistry: 0,
  build: JSON.parse(await readFile(resolve(latest.output, 'metadata.json'), 'utf8')),
  probeSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  scope:
    'Forced Node GC after cross-copy untrack/reset and fresh observer teardown; profile and build metadata identify shared/separate cores and source/installed artifacts. Not natural GC, retained user handles or browser collection.',
};
if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ status: report.status, coresCollected: 24, memoriesCollected: 24 }));
