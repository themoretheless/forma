import { test } from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadConfigFromFile } from 'vite';

const root = fileURLToPath(new URL('..', import.meta.url));
const stubs = {
  'rust-runner.js': 'export function createRustRunner(){return {};}\n',
  'native-runner.js': 'export function createNativeRunner(){return {};}\n',
  'design-data-runner.js': 'export async function evaluateDesignData(){}\n',
  'vector-runner.js': 'export function createVectorRunner(){return {};}\n',
};

async function fixture(t, { sibling = false } = {}) {
  mkdirSync(join(root, '.forma'), { recursive: true });
  const directory = mkdtempSync(join(root, '.forma/vite-config-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const checkout = join(directory, 'forma');
  mkdirSync(checkout);
  copyFileSync(join(root, 'vite.config.js'), join(checkout, 'vite.config.js'));
  for (const [file, source] of Object.entries(stubs)) writeFileSync(join(checkout, file), source);
  // Real shared modules the config imports, copied as they are rather than stubbed.
  mkdirSync(join(checkout, 'protocol'));
  copyFileSync(join(root, 'protocol/enums.js'), join(checkout, 'protocol/enums.js'));
  mkdirSync(join(checkout, 'dev-server'));
  copyFileSync(
    join(root, 'dev-server/workspace-paths.js'),
    join(checkout, 'dev-server/workspace-paths.js'),
  );
  if (sibling) {
    const workspace = join(directory, 'open-scad-viewer');
    mkdirSync(workspace);
    writeFileSync(join(workspace, 'model.scad'), 'cube(1);\n');
  }
  const loaded = await loadConfigFromFile(
    { command: 'build', mode: 'production' },
    join(checkout, 'vite.config.js'),
  );
  return loaded.config;
}

function workspaceHandler(config) {
  const routes = [];
  const server = {
    middlewares: { use: (path, handler) => routes.push({ path, handler }) },
    ws: { on() {} },
    httpServer: { once() {} },
  };
  config.plugins.find(plugin => plugin.name === 'forma-mcp-bridge').configureServer(server);
  return routes.find(route => route.path === '/__forma_workspace').handler;
}

async function listWorkspace(handler) {
  const req = { method: 'GET', url: '/list?dir=', headers: { host: '127.0.0.1:5173' } };
  const res = { setHeader() {} };
  let body;
  res.end = value => {
    body = value;
  };
  await handler(req, res);
  return { status: res.statusCode, body: JSON.parse(body) };
}

test('Vite config loads without the optional open-scad-viewer sibling', async t => {
  const config = await fixture(t);
  assert.equal(config.build.target, 'esnext');
  assert.deepEqual(await listWorkspace(workspaceHandler(config)), {
    status: 503,
    body: { error: 'open-scad-viewer workspace is unavailable' },
  });
});

test('open-scad-viewer workspace integration remains available when the sibling exists', async t => {
  const config = await fixture(t, { sibling: true });
  const response = await listWorkspace(workspaceHandler(config));
  assert.equal(response.status, 200);
  assert.deepEqual(response.body, {
    dir: '',
    entries: [{ name: 'model.scad', path: 'model.scad', kind: 'file' }],
  });
});
