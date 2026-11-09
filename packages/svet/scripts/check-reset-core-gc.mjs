import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import {
  createWasmCoreInstance,
  getWasmCore,
  withWasmCore,
  resetReactiveSystem,
  untrack,
  signal,
  computed,
  effect,
  getGraphTelemetry,
} from '../dist/package/index.js';

assert.equal(typeof global.gc, 'function', 'Run node --expose-gc after build:package');
const ambient = getWasmCore();
let checked = 0,
  maxGcRounds = 0;
function allocate() {
  const core = createWasmCoreInstance();
  withWasmCore(core, () => {
    const retired = effect(() =>
      withWasmCore(ambient, () =>
        untrack(() =>
          withWasmCore(core, () => {
            // The old JS observer frame survives reset; new observers share its core.
            resetReactiveSystem();
            const source = signal(1),
              derived = computed(() => source() * 2);
            let runs = 0;
            const fresh = effect(() => {
              derived();
              runs++;
            });
            source.set(2);
            assert.equal(derived.peek(), 4);
            assert.equal(runs, 2);
            fresh.dispose();
            derived.dispose();
            source.dispose();
          }),
        ),
      ),
    );
    retired.dispose();
    assert.equal(getGraphTelemetry().nodes, 0);
  });
  // The engine intentionally caches its last context; point that cache elsewhere.
  withWasmCore(ambient, () => getGraphTelemetry());
  return new WeakRef(core);
}
for (let batch = 0; batch < 8; batch++) {
  const refs = Array.from({ length: 8 }, allocate);
  await new Promise(setImmediate);
  let alive = refs.length;
  for (let round = 0; round < 10; round++) {
    global.gc();
    await new Promise(setImmediate);
    alive = refs.filter(ref => ref.deref() !== undefined).length;
    maxGcRounds = Math.max(maxGcRounds, round + 1);
    if (!alive) break;
    await new Promise(setImmediate);
  }
  assert.equal(alive, 0, 'A retired observer frame retained an isolated core');
  checked += refs.length;
}
const result = {
  measuredAt: new Date().toISOString(),
  node: process.version,
  execArgv: process.execArgv,
  collectedCores: checked,
  maxGcRounds,
  scope:
    '64 isolated WASM exports objects after reset during a foreign untrack callback. ' +
    'Checks object collection and graph teardown, not RSS or immediate allocator reclamation.',
};
if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
