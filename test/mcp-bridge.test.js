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
