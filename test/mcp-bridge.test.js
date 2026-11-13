import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { registerMcpBridge } from '../dev-server/mcp-bridge.js';
import { bridgeEndpoint, isBridgeUrl } from '../protocol/bridge.js';

test('the bridge is announced at the address the server actually listens on', () => {
  assert.deepEqual(bridgeEndpoint({ address: '127.0.0.1', family: 'IPv4', port: 5173 }), {
    url: 'http://127.0.0.1:5173/__forma_mcp',
    hosts: ['127.0.0.1:5173', 'localhost:5173'],
  });
  assert.deepEqual(bridgeEndpoint({ address: '::1', family: 'IPv6', port: 4000 }), {
    url: 'http://[::1]:4000/__forma_mcp',
    hosts: ['[::1]:4000', 'localhost:4000'],
  });
  assert.equal(
    bridgeEndpoint({ address: '0.0.0.0', family: 'IPv4', port: 8080 }).url,
    'http://127.0.0.1:8080/__forma_mcp',
  );
  assert.deepEqual(bridgeEndpoint({ address: '::', family: 'IPv6', port: 8080 }).hosts, [
    '127.0.0.1:8080',
    '[::1]:8080',
    'localhost:8080',
  ]);
});

test('a server that is not on loopback gets no bridge', () => {
  for (const address of [
    { address: '192.168.1.20', family: 'IPv4', port: 5173 },
    { address: 'fe80::1', family: 'IPv6', port: 5173 },
    '/tmp/forma.sock',
    null,
  ])
    assert.equal(bridgeEndpoint(address), null);
});

test('only a loopback bridge address is accepted from a descriptor', () => {
  for (const url of [
    'http://127.0.0.1:5173/__forma_mcp',
    'http://localhost:4000/__forma_mcp',
    'http://[::1]:4000/__forma_mcp',
  ])
    assert.equal(isBridgeUrl(url), true, url);
  for (const url of [
    'https://127.0.0.1:5173/__forma_mcp',
    'http://example.com:5173/__forma_mcp',
    'http://127.0.0.1.example.com:5173/__forma_mcp',
    'http://user@127.0.0.1:5173/__forma_mcp',
    'http://127.0.0.1:5173/__forma_mcp/../other',
    'http://127.0.0.1:5173/__forma_mcp?next=http://example.com',
    'http://127.0.0.1:5173/other',
    'not a url',
    undefined,
  ])
    assert.equal(isBridgeUrl(url), false, String(url));
});

/** A dev server double listening at `address`: its bridge route and its descriptor file. */
function listen(t, address) {
  const directory = mkdtempSync(join(tmpdir(), 'forma-bridge-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const httpServer = new EventEmitter();
  httpServer.address = () => address;
  let handler;
  const server = {
    httpServer,
    ws: { on() {} },
    middlewares: { use: (_, registered) => (handler = registered) },
  };
  registerMcpBridge(server, directory);
  httpServer.emit('listening');
  t.after(() => httpServer.emit('close'));
  const request = async headers => {
    const response = { setHeader() {}, end: body => (response.body = JSON.parse(body)) };
    await handler({ method: 'POST', headers }, response);
    return response.statusCode;
  };
  return { request, descriptorPath: join(directory, 'bridge.json') };
}

test('requests are accepted only under the host and token the bridge announced', async t => {
  const { request, descriptorPath } = listen(t, {
    address: '127.0.0.1',
    family: 'IPv4',
    port: 4321,
  });
  const descriptor = JSON.parse(readFileSync(descriptorPath, 'utf8'));
  assert.equal(descriptor.url, 'http://127.0.0.1:4321/__forma_mcp');
  const authorization = `Bearer ${descriptor.token}`;

  // 503: past the checks, no IDE tab is connected in this test.
  assert.equal(await request({ authorization, host: '127.0.0.1:4321' }), 503);
  assert.equal(await request({ authorization, host: 'localhost:4321' }), 503);
  assert.equal(await request({ authorization, host: '127.0.0.1:5173' }), 403);
  assert.equal(await request({ authorization, host: 'example.com:4321' }), 403);
  assert.equal(await request({ authorization, host: '127.0.0.1:4321', origin: 'http://x' }), 403);
  assert.equal(await request({ authorization: 'Bearer wrong', host: '127.0.0.1:4321' }), 403);
});

test('a server off loopback announces nothing and refuses every request', async t => {
  const { request, descriptorPath } = listen(t, {
    address: '192.168.1.20',
    family: 'IPv4',
    port: 4321,
  });
  assert.equal(existsSync(descriptorPath), false);
  assert.equal(await request({ host: '192.168.1.20:4321' }), 403);
});

/** A dev server double with a connected IDE tab that records what the bridge sends it. */
function connected(t) {
  const directory = mkdtempSync(join(tmpdir(), 'forma-bridge-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const httpServer = new EventEmitter();
  httpServer.address = () => ({ address: '127.0.0.1', family: 'IPv4', port: 4321 });
  const sockets = new EventEmitter();
  let handler;
  const currentClient = registerMcpBridge(
    {
      httpServer,
      ws: { on: (name, listener) => sockets.on(name, listener) },
      middlewares: { use: (_, registered) => (handler = registered) },
    },
    directory,
  );
  httpServer.emit('listening');
  const descriptorPath = join(directory, 'bridge.json');
  const { token } = JSON.parse(readFileSync(descriptorPath, 'utf8'));
  const tab = () => {
    const sent = [];
    return { sent, socket: { readyState: 1 }, send: (name, data) => sent.push([name, data]) };
  };
  /** Sends `chunks` as the body of a request and resolves with its status and JSON. */
  const post = (chunks, method = 'POST') => {
    const response = { setHeader() {} };
    const done = new Promise(resolve => {
      response.end = body => resolve({ status: response.statusCode, body: JSON.parse(body) });
    });
    const request = {
      method,
      headers: { authorization: `Bearer ${token}`, host: '127.0.0.1:4321' },
      async *[Symbol.asyncIterator]() {
        yield* chunks;
      },
    };
    handler(request, response);
    return done;
  };
  return { httpServer, sockets, post, tab, currentClient, descriptorPath };
}

/** Lets the bridge read the request body and forward the command. */
const forwarded = () => new Promise(resolve => setImmediate(resolve));

test('a command goes to the connected tab and its result comes back', async t => {
  const { sockets, post, tab, currentClient, httpServer } = connected(t);
  t.after(() => httpServer.emit('close'));
  const ide = tab();
  sockets.emit('forma:hello', null, ide);
  assert.equal(currentClient(), ide);

  const reply = post(['{"command":"ide_', 'status","args":{}}']);
  await forwarded();
  assert.equal(ide.sent.length, 1);
  const [name, command] = ide.sent[0];
  assert.equal(name, 'forma:command');
  assert.deepEqual({ ...command, id: null }, { id: null, command: 'ide_status', args: {} });

  // A result from a tab that is not the client, or for another id, is ignored.
  sockets.emit('forma:result', { id: command.id, value: 'spoofed' }, tab());
  sockets.emit('forma:result', { id: 'unknown', value: 'stray' }, ide);
  sockets.emit('forma:result', { id: command.id, value: 'done' }, ide);
  assert.deepEqual(await reply, { status: 200, body: { id: command.id, value: 'done' } });
});

test('the first live tab stays the client until its socket closes', async t => {
  const { sockets, tab, currentClient, httpServer } = connected(t);
  t.after(() => httpServer.emit('close'));
  const first = tab(),
    second = tab();
  sockets.emit('forma:hello', null, first);
  sockets.emit('forma:hello', null, second);
  assert.equal(currentClient(), first);
  first.socket.readyState = 3;
  sockets.emit('forma:hello', null, second);
  assert.equal(currentClient(), second);
});

test('requests the bridge cannot forward are answered without reaching the tab', async t => {
  const { sockets, post, tab, httpServer } = connected(t);
  t.after(() => httpServer.emit('close'));
  assert.deepEqual(await post(['{}']), {
    status: 503,
    body: { error: 'Open the IDE browser tab first' },
  });
  const ide = tab();
  sockets.emit('forma:hello', null, ide);

  assert.deepEqual(await post(['{}'], 'GET'), { status: 405, body: { error: 'POST required' } });
  assert.equal((await post(['not json'])).status, 500);
  assert.deepEqual(await post(['x'.repeat(1_500_000), 'x'.repeat(600_000)]), {
    status: 413,
    body: { error: 'Request too large' },
  });
  assert.deepEqual(ide.sent, []);

  ide.socket.readyState = 3;
  assert.equal((await post(['{}'])).status, 503);
});

test('stopping the server fails commands in flight and removes the descriptor', async t => {
  const { sockets, post, tab, httpServer, descriptorPath } = connected(t);
  sockets.emit('forma:hello', null, tab());
  const reply = post(['{"command":"ide_status"}']);
  await forwarded();
  assert.equal(existsSync(descriptorPath), true);
  httpServer.emit('close');
  assert.deepEqual(await reply, { status: 500, body: { error: 'IDE server stopped' } });
  assert.equal(existsSync(descriptorPath), false);
});

test('a tab that never replies fails the command and a late reply is dropped', async t => {
  const { sockets, post, tab, httpServer } = connected(t);
  t.after(() => httpServer.emit('close'));
  const ide = tab();
  sockets.emit('forma:hello', null, ide);
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const reply = post(['{"command":"file_write"}']);
  // The body is read on microtasks; the mocked clock leaves them alone.
  for (let turn = 0; turn < 10 && !ide.sent.length; turn++) await Promise.resolve();
  assert.equal(ide.sent.length, 1);
  t.mock.timers.tick(8_000);
  const { status, body } = await reply;
  assert.equal(status, 500);
  assert.match(body.error, /IDE did not reply\. Outcome may be unknown/);
  // The late result finds no pending command and is ignored.
  sockets.emit('forma:result', { id: ide.sent[0][1].id, value: 'late' }, ide);
});
