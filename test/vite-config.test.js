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

async function fixture(t) {
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
  const loaded = await loadConfigFromFile(
    { command: 'build', mode: 'production' },
    join(checkout, 'vite.config.js'),
  );
  return loaded.config;
}

function devRoutes(config) {
  const routes = [];
  const server = {
    middlewares: { use: (path, handler) => routes.push({ path, handler }) },
    ws: { on() {} },
    httpServer: { once() {} },
  };
  config.plugins.find(plugin => plugin.name === 'forma-mcp-bridge').configureServer(server);
  return routes.map(route => route.path);
}

test('Vite config loads on its own, with no sibling repository', async t => {
  const config = await fixture(t);
  assert.equal(config.build.target, 'esnext');
});

test('the dev server exposes no file API onto a neighbouring repository', async t => {
  const routes = devRoutes(await fixture(t));
  assert.ok(routes.length > 0, 'the bridge plugin registered its routes');
  assert.equal(routes.includes('/__forma_workspace'), false);
});
