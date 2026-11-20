import vm from 'node:vm';
import { afterEach, expect, it, vi } from 'vitest';
import { decodeWasmBinary } from '../src/engine/wasm-binary';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.resetModules();
});

it('installSvetWasmModule accepts WebAssembly.Module compiled in another VM realm and rejects non-modules', async () => {
  const { installSvetWasmModule, createWasmCoreInstance } = await import('../src/engine/index');
  const raw = decodeWasmBinary();
  const foreignModule = vm.runInNewContext('new WebAssembly.Module(bytes)', {
    bytes: raw,
  }) as WebAssembly.Module;

  expect(foreignModule instanceof WebAssembly.Module).toBe(false);
  installSvetWasmModule(foreignModule);
  const core = createWasmCoreInstance();
  expect(core.svet_get_node_count()).toBe(0);
  expect(() => installSvetWasmModule({} as WebAssembly.Module)).toThrow(/precompiled/);
});

it('loadSvetWasm accepts ArrayBuffer and URL instances constructed in another VM realm', async () => {
  const raw = decodeWasmBinary();
  const foreignBuffer = vm.runInNewContext('new Uint8Array(bytes).buffer', {
    bytes: raw,
  }) as ArrayBuffer;
  expect(foreignBuffer instanceof ArrayBuffer).toBe(false);

  vi.resetModules();
  const bridgeA = await import('../src/engine/wasm-bridge');
  await bridgeA.loadSvetWasm(foreignBuffer);
  const coreA = bridgeA.createWasmCoreInstance();
  expect(coreA.svet_get_node_count()).toBe(0);

  vi.resetModules();
  const bridgeB = await import('../src/engine/wasm-bridge');
  const foreignUrl = vm.runInNewContext(
    'Object.defineProperty({ href: "https://svet.invalid/core.wasm" }, Symbol.toStringTag, { value: "URL" })',
  ) as URL;
  expect(foreignUrl instanceof URL).toBe(false);
  const seenUrls: unknown[] = [];
  vi.stubGlobal('fetch', async (input: unknown) => {
    seenUrls.push(input);
    return new Response(new Uint8Array(raw), {
      status: 200,
      headers: { 'content-type': 'application/wasm' },
    });
  });
  await bridgeB.loadSvetWasm(foreignUrl);
  expect(seenUrls).toEqual([foreignUrl]);
  expect(bridgeB.createWasmCoreInstance().svet_get_node_count()).toBe(0);
});

it('per-core WASM linear memory starts compact (<= 3 pages) and grows cleanly under 10k nodes and 64k floats', async () => {
  const { createWasmCoreInstance } = await import('../src/engine/index');
  const core = createWasmCoreInstance();
  expect(core.memory.buffer.byteLength).toBeLessThanOrEqual(196_608);

  const ids: number[] = [];
  for (let i = 0; i < 10_000; i++) {
    ids.push(core.svet_create_node(0));
  }
  expect(core.svet_get_node_count()).toBe(10_000);
  const offset = core.svet_gpu_alloc_f32(65_536);
  expect(offset).toBeGreaterThanOrEqual(0);
  core.svet_gpu_free_f32(offset, 65_536);
  for (let i = ids.length - 1; i >= 0; i--) {
    core.svet_dispose_node(ids[i]!);
  }
  expect(core.svet_get_node_count()).toBe(0);
});
