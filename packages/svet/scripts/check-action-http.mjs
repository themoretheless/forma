import {beforeEach, test} from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {createRoot, createServerAction, getGraphTelemetry, resetReactiveSystem, ServerActionDisposedError} from '../dist/package/index.js';

beforeEach(() => resetReactiveSystem());
const deferred = () => {
  let resolve;
  const promise = new Promise(yes => { resolve = yes; });
  return {promise, resolve};
};

async function localServer(t, handler) {
  const responses = new Set();
  const server = createServer((request, response) => {
    responses.add(response);
    response.once('close', () => responses.delete(response));
    handler(request, response);
  });
  t.after(async () => {
    for (const response of responses) response.destroy();
    server.closeAllConnections();
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  return `http://127.0.0.1:${server.address().port}`;
}

const fetchJson = (url, abort) => fetch(url, {signal: abort}).then(response => response.json());

for (const mode of ['dispose', 'reset']) test(`${mode} aborts native fetch before response headers and closes the HTTP response`, {timeout: 5000}, async t => {
  const arrived = deferred(), closed = deferred();
  let ended = true, rpcSignal;
  const url = await localServer(t, (_request, response) => {
    response.once('close', () => { ended = response.writableEnded; closed.resolve(); });
    arrived.resolve();
  });
  const action = createServerAction({rpc: (_args, abort) => { rpcSignal = abort; return fetchJson(url, abort); }});
  t.after(() => action.dispose());
  const result = action(); void result.catch(() => {});
  await arrived.promise;
  if (mode === 'reset') resetReactiveSystem(); else action.dispose();
  await assert.rejects(result, ServerActionDisposedError);
  await closed.promise;
  assert.equal(rpcSignal.aborted, true);
  assert.equal(ended, false, 'fetch abort must close the still-unfinished server response');
  // Old-generation accessors freeze their last snapshot after reset.
  assert.equal(action.pending(), mode === 'reset');
  assert.equal(getGraphTelemetry().nodes, 0);
});

test('owner teardown aborts fetch while its JSON response body is still streaming', {timeout: 5000}, async t => {
  const arrived = deferred(), bodyStarted = deferred(), closed = deferred();
  let ended = true, action, stop;
  const url = await localServer(t, (_request, response) => {
    response.once('close', () => { ended = response.writableEnded; closed.resolve(); });
    response.writeHead(200, {'content-type': 'application/json'});
    response.write('{"value":');
    arrived.resolve();
  });
  createRoot(dispose => {
    stop = dispose;
    action = createServerAction({rpc: async (_args, abort) => {
      const response = await fetch(url, {signal: abort});
      bodyStarted.resolve();
      return response.json();
    }});
  });
  t.after(() => stop());
  const result = action(); void result.catch(() => {});
  await arrived.promise; await bodyStarted.promise;
  stop();
  await assert.rejects(result, ServerActionDisposedError);
  await closed.promise;
  assert.equal(ended, false);
  assert.equal(getGraphTelemetry().nodes, 0);
});

test('overlapping native HTTP requests keep pending active until both response bodies finish', {timeout: 5000}, async t => {
  const received = new Map(), allArrived = deferred();
  const url = await localServer(t, (request, response) => {
    received.set(request.url, response);
    if (received.size === 2) allArrived.resolve();
  });
  const action = createServerAction({rpc: (id, abort) => fetchJson(`${url}/${id}`, abort)});
  t.after(() => action.dispose());
  const first = action('first'), second = action('second');
  void first.catch(() => {}); void second.catch(() => {});
  await allArrived.promise;
  received.get('/first').end('{"value":1}');
  assert.deepEqual(await first, {value: 1});
  assert.equal(action.pending(), true);
  received.get('/second').end('{"value":2}');
  assert.deepEqual(await second, {value: 2});
  assert.equal(action.pending(), false);
  assert.equal(action.error(), null);
  action.dispose();
  assert.equal(getGraphTelemetry().nodes, 0);
});
