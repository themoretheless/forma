import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { writeFile } from 'node:fs/promises';
import { JSDOM } from 'jsdom';
const dom = new JSDOM('<!doctype html><body></body>');
for (const key of ['document', 'Node', 'Element']) globalThis[key] = dom.window[key];
const runtime = process.argv[3]
  ? pathToFileURL(resolve(process.argv[3], 'dist/package') + '/')
  : new URL('../dist/package/', import.meta.url);
const { getWasmCore, h } = await import(new URL('index.js', runtime));
const { renderToChunksAsync } = await import(new URL('server.js', runtime));
assert.equal(typeof global.gc, 'function');
const handles = [],
  probes = [];
function track(kind, value) {
  probes.push({ kind, ref: new WeakRef(value) });
}
async function run(mode) {
  const marker = { mode };
  track('view-capture', marker);
  const view = () => {
    assert.equal(marker.mode, mode);
    const core = getWasmCore();
    track('core', core);
    track('memory', core.memory);
    const node = h('main', null, mode);
    track('node', node);
    if (mode === 'error') throw Error('expected stream failure');
    return { node };
  };
  track('view', view);
  const controller = new AbortController();
  const iterator = renderToChunksAsync(view, { signal: controller.signal });
  handles.push(iterator);
  if (mode === 'abort-idle' || mode === 'abort-started') {
    if (mode === 'abort-started') await iterator.next();
    controller.abort('probe abort');
    await new Promise(setImmediate);
    return;
  }
  if (mode === 'error') await assert.rejects(iterator.next(), /expected stream failure/);
  else if (mode === 'return') {
    assert.equal((await iterator.next()).done, false);
    await iterator.return();
  } else for await (const chunk of iterator) assert.equal(chunk.version, 1);
  assert.equal((await iterator.next()).done, true);
}
for (let i = 0; i < 8; i++)
  for (const mode of ['complete', 'return', 'error', 'abort-idle', 'abort-started'])
    await run(mode);
async function collect() {
  await new Promise(setImmediate);
  for (let i = 0; i < 10; i++) {
    global.gc();
    await new Promise(setImmediate);
  }
  const alive = {};
  for (const probe of probes)
    if (probe.ref.deref()) alive[probe.kind] = (alive[probe.kind] ?? 0) + 1;
  return alive;
}
const retained = await collect();
handles.length = 0;
const released = await collect();
assert.deepEqual(released, {}, 'Control: resources must collect after releasing iterator handles');
const result = {
  measuredAt: new Date().toISOString(),
  node: process.version,
  iterations: 40,
  retained,
  released,
  status: Object.keys(retained).length ? 'retention-reproduced' : 'passed',
  scope:
    'Caller-retained stream iterators after complete/return/error/external abort before and after first read; Node isolated WASM and JSDOM; no RSS or browser GC claim.',
};
if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(result, null, 2) + '\n');
console.log(JSON.stringify(result, null, 2));
dom.window.close();
assert.deepEqual(retained, {}, 'Terminal iterator handles retain request resources');
