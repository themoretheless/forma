import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { cpus, platform, release, arch } from 'node:os';
import { performance } from 'node:perf_hooks';
import { createWasmCoreInstance } from '../dist/package/wasm-bridge.js';

// Developer measurement of a real isolated WASM allocator, not physical WebGPU.
// Run after build:package, with --expose-gc; optional first argument is JSON output.
assert.equal(typeof global.gc, 'function', 'Run node --expose-gc');
const core = createWasmCoreInstance();
const memory = [];
async function sample(phase) {
  await new Promise(setImmediate);
  global.gc();
  await new Promise(setImmediate);
  memory.push({ phase, ...process.memoryUsage(),
    wasmBytes: core.memory.buffer.byteLength,
    arenaFloats: core.svet_gpu_get_arena_len() });
}
await sample('initial');
const growthOffsets = [];
const start = performance.now();
for (let i = 0; i < 4096; i++) {
  const offset = core.svet_gpu_alloc_f32(256);
  assert.equal(offset, i * 256);
  growthOffsets.push(offset);
}
const growthMs = performance.now() - start;
await sample('grown');
for (const offset of growthOffsets) core.svet_gpu_free_f32(offset, 256);
core.svet_gpu_clear_dirty();
await sample('freed');

const sizes = Array.from({ length: 128 }, (_, i) => 1 + (i * 37 % 251));
const offsets = new Uint32Array(sizes.length);
const order = Uint32Array.from(sizes, (_, i) => i);
let seed = 0x12345678;
function cycle(marker) {
  for (let i = 0; i < sizes.length; i++) {
    const offset = core.svet_gpu_alloc_f32(sizes[i]);
    assert.notEqual(offset, 0xffffffff);
    offsets[i] = offset;
    const values = new Float32Array(core.memory.buffer,
      core.svet_gpu_get_arena_ptr() + offset * 4, sizes[i]);
    assert.equal(values[0], 0);
    assert.equal(values.at(-1), 0);
    values.fill(marker);
  }
  const arena = new Float32Array(core.memory.buffer, core.svet_gpu_get_arena_ptr(),
    core.svet_gpu_get_arena_len());
  for (let i = 0; i < sizes.length; i++) {
    assert.equal(arena[offsets[i]], marker);
    assert.equal(arena[offsets[i] + sizes[i] - 1], marker);
  }
  for (let i = order.length - 1; i > 0; i--) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    const j = seed % (i + 1);
    [order[i], order[j]] = [order[j], order[i]];
  }
  for (const i of order) core.svet_gpu_free_f32(offsets[i], sizes[i]);
  core.svet_gpu_clear_dirty();
}
for (let i = 0; i < 100; i++) cycle(i + 1);
await sample('warmed');
const warmedBytes = core.memory.buffer.byteLength;
const warmedPointer = core.svet_gpu_get_arena_ptr();
const timings = [];
for (let batch = 0; batch < 7; batch++) {
  const began = performance.now();
  for (let i = 0; i < 500; i++) cycle(i + 1);
  timings.push(performance.now() - began);
  assert.equal(core.memory.buffer.byteLength, warmedBytes);
  assert.equal(core.svet_gpu_get_arena_ptr(), warmedPointer);
  assert.equal(core.svet_gpu_get_arena_len(), 4096 * 256);
  await sample(`batch-${batch + 1}`);
}
core.svet_reset();
assert.equal(core.svet_gpu_get_arena_len(), 0);
await sample('reset');
const result = {
  measuredAt: new Date().toISOString(), node: process.version,
  platform: platform(), release: release(), arch: arch(), cpu: cpus()[0]?.model,
  embeddedModuleSha256: createHash('sha256').update(await readFile(
    new URL('../dist/package/wasm-binary.js', import.meta.url))).digest('hex'),
  growth: { allocations: 4096, floatsPerAllocation: 256, milliseconds: growthMs },
  churn: { warmupCycles: 100, batches: 7, cyclesPerBatch: 500,
    allocationsPerCycle: 128, allocationFreePairs: 3500 * 128,
    batchMilliseconds: timings, medianBatchMilliseconds: [...timings].sort((a,b)=>a-b)[3] },
  memory,
  scope: 'One isolated WASM core; timings include JS validation and float writes. '
    + 'GC outside timed batches. No baseline comparison, physical GPU, near-limit '
    + 'pressure or process-memory plateau guarantee. WASM high-water storage is retained.'
};
if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
