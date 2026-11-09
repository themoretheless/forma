import { createHash } from 'node:crypto';
import assert from 'node:assert/strict';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { JSDOM } from 'jsdom';
assert(global.gc, 'Use --expose-gc');
const [installed, output] = process.argv.slice(2);
assert(installed && output);
const profile = process.argv[4] ?? 'both';
assert(['both', 'boundary'].includes(profile));
const kinds = profile === 'both' ? ['boundary', 'router'] : ['boundary'];
const artifactSha256 = {};
for (const file of (await readdir(resolve(installed, 'dist/package'))).filter(x =>
  /\.(js|wasm)$/.test(x),
))
  artifactSha256[file] = createHash('sha256')
    .update(await readFile(resolve(installed, 'dist/package', file)))
    .digest('hex');
const metadata = JSON.parse(await readFile(resolve(installed, 'package.json'), 'utf8'));
const engine = await import(pathToFileURL(resolve(installed, metadata.exports['.'].import)).href);
const runtime = await import(
  pathToFileURL(resolve(installed, metadata.exports['./server'].import)).href
);
const dom = new JSDOM('<!doctype html><body></body>', { url: 'http://svet.test/' });
for (const key of ['window', 'document', 'Node', 'Element']) globalThis[key] = dom.window[key];
const held = [],
  refs = [],
  unmountNodes = { boundary: [], router: [] };
let controls = Array.from({ length: 4 }, () => ({ live: true }));
const controlRefs = controls.map(x => new WeakRef(x));
for (const kind of kinds)
  for (let i = 0; i < 4; i++)
    await runtime.runWithIsolatedContextAsync(() => {
      const core = engine.getWasmCore();
      refs.push({ kind, core: new WeakRef(core), memory: new WeakRef(core.memory) });
      const container = document.createElement('div');
      const app = engine.mount(() => {
        if (kind === 'boundary')
          return engine.ErrorBoundary({
            children: () => {
              throw Error('controlled');
            },
            fallback: (_, retry) => {
              held.push(retry);
              return document.createTextNode('error');
            },
          });
        const router = engine.createRouter({ routes: [], initialPath: '/start' });
        held.push(
          router,
          router.beforeEach(() => {}),
        );
        router.navigate('/finished?q=last#done');
        return engine.RouterView({ router });
      }, container);
      app.unmount();
      unmountNodes[kind].push(engine.getGraphTelemetry().nodes);
    });
async function collect() {
  for (let i = 0; i < 12; i++) {
    await new Promise(setImmediate);
    global.gc();
    await new Promise(setImmediate);
  }
  return Object.fromEntries(
    kinds.map(kind => [
      kind,
      refs.filter(r => r.kind === kind && (r.core.deref() || r.memory.deref())).length,
    ]),
  );
}
const retained = await collect();
assert.equal(controlRefs.filter(x => x.deref()).length, 4);
controls = null;
await collect();
assert.equal(controlRefs.filter(x => x.deref()).length, 0);
for (const item of held)
  if (typeof item === 'function') item();
  else {
    assert.equal(item.navigate('/late'), false);
    assert.equal(item.path(), '/finished');
    assert.equal(item.path.peek(), '/finished');
    assert.equal(item.query().q, 'last');
    assert.equal(item.hash(), 'done');
    assert.equal(item.match('/late?q=x#h').query.q, 'x');
    item.beforeEach(() => {
      throw Error('retired guard invoked');
    })();
    item.dispose();
    item.path.dispose();
    assert.equal(item.path(), '/finished');
  }
const report = {
  unmountNodes,
  profile,
  artifactSha256,
  probeSha256: createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex'),
  status:
    Object.values(retained).every(x => x === 0) &&
    kinds.every(kind => unmountNodes[kind].every(x => x === 0))
      ? 'passed'
      : 'retained',
  retained,
  handles: held.length,
  controlsHeld: 4,
  controlsReleased: 0,
  node: process.version,
  scope:
    'Installed Node ALS+JSDOM; retained retry functions and disposed router objects; four requests per kind',
};
await writeFile(output, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report));
dom.window.close();
if (report.status !== 'passed') process.exitCode = 1;
