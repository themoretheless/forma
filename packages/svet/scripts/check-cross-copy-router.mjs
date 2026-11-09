import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { JSDOM } from 'jsdom';
assert(process.argv[2] && process.argv[3], 'Expected output and installed-copy manifest');
const manifest = JSON.parse(await readFile(resolve(process.argv[3]), 'utf8')),
  build = JSON.parse(await readFile(resolve(manifest.output, 'metadata.json'), 'utf8'));
for (const [path, sha] of Object.entries(build.artifactSha256))
  assert.equal(
    createHash('sha256')
      .update(await readFile(resolve(manifest.output, path)))
      .digest('hex'),
    sha,
  );
const dom = new JSDOM('<!doctype html><body></body>', { url: 'http://svet.test/start' });
for (const key of [
  'window',
  'document',
  'Node',
  'Element',
  'HTMLElement',
  'MutationObserver',
  'history',
  'location',
])
  globalThis[key] = dom.window[key];
const a = await import(pathToFileURL(resolve(manifest.output, 'first/index.js')).href),
  b = await import(pathToFileURL(resolve(manifest.output, 'second/index.js')).href);
const report = {
  cases: [],
  build,
  scope:
    'Compatible independent installed copies; shared explicit core; JSDOM router/context/link lifecycle',
};
async function check(name, body) {
  try {
    await body();
    report.cases.push({ name, status: 'passed' });
  } catch (error) {
    report.cases.push({ name, status: 'failed', error: String(error.stack ?? error) });
  } finally {
    a.resetReactiveSystem();
    b.resetReactiveSystem();
    document.body.replaceChildren();
  }
}
for (const [label, host, foreign] of [
  ['A→B', a, b],
  ['B→A', b, a],
]) {
  await check(label + ' RouterView exposes router to foreign useRouter and Link', () => {
    const core = host.getWasmCore();
    let router,
      link,
      cleaned = 0,
      mounts = 0;
    const app = host.mount(() => {
      router = host.createRouter({
        initialPath: '/start',
        routes: [
          {
            path: '/start',
            component: () => {
              mounts++;
              return foreign.withWasmCore(core, () => {
                assert.equal(foreign.useRouter(), router);
                foreign.onCleanup(() => cleaned++);
                link = foreign.Link({ to: '/next/7?q=value#hash', children: 'next' });
                return link;
              });
            },
          },
          {
            path: '/next/:id',
            component: params =>
              foreign.withWasmCore(core, () => {
                mounts++;
                assert.equal(foreign.useRouter(), router);
                assert.equal(params.id, '7');
                return document.createTextNode('next ' + params.id);
              }),
          },
        ],
      });
      return host.RouterView({ router });
    }, document.body);
    try {
      const event = new dom.window.MouseEvent('click', {
        bubbles: true,
        cancelable: true,
        button: 0,
      });
      link.dispatchEvent(event);
      assert.equal(event.defaultPrevented, true);
      assert.equal(router.path(), '/next/7');
      assert.equal(router.query().q, 'value');
      assert.equal(router.hash(), 'hash');
      assert.equal(document.body.textContent, 'next 7');
      assert.equal(cleaned, 1);
      assert.equal(mounts, 2);
      app.unmount();
      assert.equal(host.getGraphTelemetry().nodes, 0);
      const late = new dom.window.MouseEvent('click', {
        bubbles: true,
        cancelable: true,
        button: 0,
      });
      link.dispatchEvent(late);
      assert.equal(late.defaultPrevented, false);
    } finally {
      app.unmount();
      router?.dispose();
    }
  });
}
await check('navigate batches URL publication in a non-ambient core', () => {
  const core = a.createWasmCoreInstance();
  let router, stop;
  const seen = [];
  a.withWasmCore(core, () =>
    a.createRoot(dispose => {
      stop = dispose;
      router = a.createRouter({ routes: [], initialPath: '/start' });
      a.effect(() => seen.push([router.path(), router.query().q, router.hash()]));
    }),
  );
  try {
    router.navigate('/next?q=value#hash');
    assert.deepEqual(seen, [
      ['/start', undefined, ''],
      ['/next', 'value', 'hash'],
    ]);
  } finally {
    stop();
  }
  assert.equal(core.svet_get_node_count(), 0);
});
await check('live route-table replacement and terminal matcher retirement', () => {
  const first = { path: '/first', component: () => document.createTextNode('first') },
    second = { path: '/next/:id', component: () => document.createTextNode('next') };
  const options = { routes: [first], initialPath: '/first' },
    router = a.createRouter(options);
  try {
    options.routes = [second];
    assert.equal(router.match('/next/7').route, second);
    router.navigate('/next/7');
    assert.equal(router.params().id, '7');
    router.dispose();
    Object.defineProperty(options, 'routes', {
      get() {
        throw Error('Retired options read');
      },
    });
    assert.equal(router.match('/next/8').params.id, '8');
  } finally {
    router.dispose();
  }
  assert.equal(a.getGraphTelemetry().nodes, 0);
});
dom.window.close();
report.status = report.cases.every(row => row.status === 'passed') ? 'passed' : 'failed';
report.probeSha256 = createHash('sha256')
  .update(await readFile(new URL(import.meta.url)))
  .digest('hex');
await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report.cases, null, 2));
if (report.status !== 'passed') process.exitCode = 1;
