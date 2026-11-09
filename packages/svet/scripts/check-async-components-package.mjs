import assert from 'node:assert/strict';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { JSDOM } from 'jsdom';
const installed = resolve(process.argv[2]);
const engine = await import(pathToFileURL(resolve(installed, 'dist/package/index.js')).href);
const { renderToStringAsync, renderToChunksAsync } = await import(
  pathToFileURL(resolve(installed, 'dist/package/server.js')).href
);
const { h, signal, onCleanup, getWasmCore, createContext, useContext } = engine;
const dom = new JSDOM('<!doctype html><html><body></body></html>');
for (const name of ['document', 'Node', 'Element']) globalThis[name] = dom.window[name];
try {
  const context = createContext('missing');
  let cleaned = 0,
    core;
  const Child = async () => {
    await Promise.resolve();
    const value = signal(useContext(context));
    onCleanup(() => {
      cleaned++;
      value.dispose();
    });
    return h(
      'p',
      null,
      h(async () => {
        await Promise.resolve();
        return h('b', null, value());
      }, null),
    );
  };
  const output = await renderToStringAsync(() => {
    core = getWasmCore();
    const sync = h(() => h('span', null, 'sync'), null);
    assert.equal(sync.textContent, 'sync');
    return {
      node: h(
        'main',
        null,
        sync,
        h(context.Provider, { value: 'owned', children: () => h(Child, null) }),
      ),
    };
  });
  assert.equal(output.html, '<main><span>sync</span><p><b>owned</b></p></main>');
  assert.equal(cleaned, 1);
  assert.equal(core.svet_get_node_count(), 0);
  const failure = Error('failed sibling');
  let stopped = 0;
  await assert.rejects(
    renderToStringAsync(() => ({
      node: h(
        'div',
        null,
        h(async () => {
          onCleanup(() => stopped++);
          await new Promise(() => {});
          return h('i', null);
        }, null),
        h(async () => {
          throw failure;
        }, null),
      ),
    })),
    error => error === failure,
  );
  assert.equal(stopped, 1);
  let release;
  const wait = new Promise(resolve => (release = resolve)),
    named = {};
  const stream = renderToChunksAsync(() => ({
    node: h(
      'main',
      null,
      h('b', null, 'shell'),
      h(async () => {
        await wait;
        named.count = signal(7);
        onCleanup(named.count.dispose);
        return h('input', { type: 'number', 'data-svet-model': 'count', model: named.count });
      }, null),
    ),
    signals: named,
  }));
  const host = document.createElement('div'),
    receiver = engine.createSsrStreamReceiver(host);
  const shell = await stream.next();
  assert.equal(shell.value.payload.type, 'shell');
  assert.equal(shell.value.version, engine.SSR_STREAM_VERSION);
  assert.equal(shell.value.sequence, 0);
  receiver.apply(shell.value);
  assert.equal(host.textContent, 'shell');
  release();
  let restored;
  for await (const chunk of stream) {
    const state = receiver.apply(chunk);
    if (state) restored = state;
  }
  receiver.finish();
  assert.equal(restored.count(), 7);
  receiver.dispose();
  assert.equal(engine.getGraphTelemetry().nodes, 0);
  const invalidHost = document.createElement('div');
  invalidHost.innerHTML = '<input value="preserved">';
  const original = invalidHost.firstChild;
  const invalidReceiver = engine.createSsrStreamReceiver(invalidHost);
  assert.throws(
    () =>
      invalidReceiver.apply({
        version: 1,
        sequence: 0,
        streamId: 'ranges',
        payload: {
          type: 'shell',
          html: '<!--svet:ranges:1--><!--svet:ranges:2--><!--svet:ranges:1:end--><!--svet:ranges:2:end-->',
        },
      }),
    /range/,
  );
  assert.equal(invalidHost.firstChild, original);
  assert.equal(invalidReceiver.active, false);
  const eofReceiver = engine.createSsrStreamReceiver(document.createElement('div'));
  assert.throws(() => eofReceiver.finish(), /before complete/);
  assert.equal(eofReceiver.active, false);
  console.log(
    'Installed async components: nested ownership, immediate sync DOM, fail-fast and teardown passed',
  );
} finally {
  dom.window.close();
}
