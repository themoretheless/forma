import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import vm from 'node:vm';
import { AsyncLocalStorage } from 'node:async_hooks';
import { JSDOM } from 'jsdom';

assert.equal(typeof global.gc, 'function', 'Run with node --expose-gc');
assert(process.argv[2] && process.argv[3], 'Usage: soak-multi-realm-gc.mjs OUTPUT COPY_MANIFEST');

const outputPath = resolve(process.argv[2]);
const manifest = JSON.parse(await readFile(resolve(process.argv[3]), 'utf8'));
const cycles = Number(process.argv[4] ?? 120);
assert(Number.isSafeInteger(cycles) && cycles >= 12 && cycles <= 1000);

const hostDom = new JSDOM('<!doctype html><body></body>', { url: 'http://svet-host.test/' });
for (const key of [
  'window',
  'document',
  'Node',
  'Element',
  'HTMLElement',
  'DocumentFragment',
  'Comment',
  'Text',
  'MutationObserver',
  'Event',
  'CustomEvent',
  'getComputedStyle',
]) {
  globalThis[key] = hostDom.window[key];
}

const first = await import(pathToFileURL(resolve(manifest.output, 'first/index.js')).href);
const second = await import(pathToFileURL(resolve(manifest.output, 'second/index.js')).href);
const ssrRuntime = await import(
  pathToFileURL(resolve(manifest.output, 'first/ssr-runtime-entry.js')).href
);

const runtime = ssrRuntime.createSsrRuntime({
  createAsyncContext: () => new AsyncLocalStorage(),
  randomUUID: () => globalThis.crypto.randomUUID(),
});

// Verify cross-realm precompiled WASM installation from a separate VM realm.
const wasmBytes = await readFile(resolve(manifest.output, 'first/svet-core.wasm'));
const foreignVmModule = vm.runInNewContext('new WebAssembly.Module(bytes)', {
  bytes: wasmBytes,
});
assert.equal(foreignVmModule instanceof WebAssembly.Module, false);
second.installSvetWasmModule(foreignVmModule);

const refs = [];
let controls = Array.from({ length: 4 }, (_, index) => ({ control: index + 1 }));
const controlRefs = controls.map(item => new WeakRef(item));

async function runCycle(index) {
  const host = index % 2 === 0 ? first : second;
  const foreign = index % 2 === 0 ? second : first;
  const foreignDom = new JSDOM('<!doctype html><body><button>foreign</button></body>', {
    url: `http://svet-realm-${index}.test/`,
  });
  const foreignButton = foreignDom.window.document.querySelector('button');
  refs.push(
    { cycle: index, kind: 'realmWindow', ref: new WeakRef(foreignDom.window) },
    { cycle: index, kind: 'realmDocument', ref: new WeakRef(foreignDom.window.document) },
    { cycle: index, kind: 'foreignNode', ref: new WeakRef(foreignButton) },
  );

  // Exercise synchronous cross-realm DOM adoption & cross-copy reactive mounts.
  const container = hostDom.window.document.createElement('div');
  hostDom.window.document.body.append(container);
  let cleaned = 0;
  const app = host.mount(() => {
    const core = host.getWasmCore();
    const label = host.signal(`item-${index}`);
    host.onCleanup(() => {
      cleaned++;
      label.dispose();
    });
    return foreign.withWasmCore(core, () => {
      foreign.onCleanup(() => cleaned++);
      return host.h(
        'section',
        null,
        foreignButton,
        host.h('span', null, () => label()),
      );
    });
  }, container);
  assert.equal(container.firstChild.firstChild, foreignButton);
  app.unmount();
  container.remove();
  assert.equal(cleaned, 2);
  foreignDom.window.close();

  // Exercise post-await async SSR owner capture across independent copies.
  const ctx = first.createContext('default');
  await runtime.runWithIsolatedContextAsync(async () => {
    const reqCore = first.getWasmCore();
    refs.push(
      { kind: 'wasmCore', ref: new WeakRef(reqCore) },
      { kind: 'wasmMemory', ref: new WeakRef(reqCore.memory) },
    );
    await runtime.runWithOwnerScopeAsync([[ctx.id, `val-${index}`]], async () => {
      const childOwner = first.getCurrentOwner();
      refs.push({ kind: 'asyncOwner', ref: new WeakRef(childOwner) });
      await Promise.resolve();
      assert.equal(first.getCurrentOwner(), childOwner);
      assert.equal(
        second.withWasmCore(reqCore, () => second.getCurrentOwner()),
        childOwner,
      );
      const foreignVal = second.withWasmCore(reqCore, () => {
        const s = second.signal(index);
        second.onCleanup(() => s.dispose());
        return `${second.useContext(ctx)}:${s()}`;
      });
      assert.equal(foreignVal, `val-${index}:${index}`);
    });
  });
}

for (let i = 0; i < cycles; i++) {
  await runCycle(i);
}

assert.equal(first.getGraphTelemetry().nodes, 0);
assert.equal(second.getGraphTelemetry().nodes, 0);

async function collectAll(list) {
  let alive;
  let rounds = 0;
  await new Promise(resolve => setTimeout(resolve, 20));
  for (; rounds < 12; rounds++) {
    await new Promise(setImmediate);
    global.gc();
    await new Promise(setImmediate);
    alive = list.filter(entry => entry.ref.deref() !== undefined);
    if (!alive.length) break;
  }
  return {
    aliveCount: alive.length,
    aliveItems: alive.map(a => `${a.kind}@${a.cycle ?? '?'}`),
    rounds: rounds + 1,
  };
}

const collected = await collectAll(refs);
assert.equal(
  collected.aliveCount,
  0,
  `Retained references after GC: ${JSON.stringify(collected.aliveItems.slice(0, 12))}`,
);
const heldControls = controlRefs.filter(ref => ref.deref() !== undefined).length;
assert.equal(heldControls, 4);
controls = undefined;
let releasedAlive = 4;
for (let r = 0; r < 12; r++) {
  await new Promise(setImmediate);
  global.gc();
  await new Promise(setImmediate);
  releasedAlive = controlRefs.filter(ref => ref.deref() !== undefined).length;
  if (releasedAlive === 0) break;
}
assert.equal(releasedAlive, 0);

hostDom.window.close();

const countsByKind = {};
for (const entry of refs) countsByKind[entry.kind] = (countsByKind[entry.kind] ?? 0) + 1;

const report = {
  status: 'passed',
  completedAt: new Date().toISOString(),
  node: process.version,
  cycles,
  snapshot: manifest.snapshot,
  trackedReferences: countsByKind,
  remainingAfterGc: collected.aliveCount,
  gcRounds: collected.rounds,
  positiveControlHeld: heldControls,
  positiveControlRemainingAfterRelease: releasedAlive,
  crossRealmModuleInstalled: true,
  defaultCoreMemoryBytes: first.createWasmCoreInstance().memory.buffer.byteLength,
  probeSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  scope:
    'Forced Node GC across 120 JSDOM realm + VM WebAssembly.Module + cross-copy synchronous DOM and post-await SSR owner cycles.',
};

await writeFile(outputPath, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
