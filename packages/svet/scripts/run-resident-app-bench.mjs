import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, extname, normalize, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const svetRoot = resolve(scriptDir, '..');

const { values } = parseArgs({
  options: {
    'playwright-dir': { type: 'string' },
    manifest: {
      type: 'string',
      default: resolve(svetRoot, 'node_modules/.cache/svet-resident-app-bench/latest.json'),
    },
    output: { type: 'string' },
    frames: { type: 'string', default: '60' },
    rounds: { type: 'string', default: '3' },
  },
});

const playwrightDir = values['playwright-dir'] ?? process.env.PLAYWRIGHT_MODULE_DIR;
assert(playwrightDir, 'Provide --playwright-dir or PLAYWRIGHT_MODULE_DIR');
assert(values.output, 'Provide --output RAW_OUTPUT_JSON');

const { chromium } = await import(
  pathToFileURL(resolve(playwrightDir, 'node_modules/playwright/index.mjs')).href
);

const manifest = JSON.parse(await readFile(resolve(values.manifest), 'utf8'));
const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
};

function safeUnder(base, rel) {
  const cleaned = normalize(rel).replace(/^[/\\]+/, '');
  const full = resolve(base, cleaned);
  return full === base || full.startsWith(base + '/') ? full : null;
}

const server = createServer(async (req, res) => {
  try {
    const reqUrl = new URL(req.url ?? '/', 'http://127.0.0.1');
    const file = safeUnder(
      manifest.output,
      reqUrl.pathname === '/' ? 'index.html' : reqUrl.pathname.slice(1),
    );
    if (!file) {
      res.writeHead(404);
      res.end('Not found');
      return;
    }
    const body = await readFile(file);
    res.writeHead(200, {
      'content-type': MIME[extname(file)] ?? 'application/octet-stream',
      'cache-control': 'no-store',
    });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end('Not found');
  }
});

await new Promise((resolvePromise, reject) => {
  server.once('error', reject);
  server.listen(0, '127.0.0.1', resolvePromise);
});

try {
  const browser = await chromium.launch({
    channel: 'chromium',
    headless: true,
    args: ['--enable-unsafe-webgpu', '--use-angle=metal'],
  });
  try {
    const page = await browser.newPage();
    await page.goto(
      `http://127.0.0.1:${server.address().port}/tests/browser/resident-app-bench.html`,
      { waitUntil: 'networkidle' },
    );
    await page.locator('#frames').fill(String(values.frames));
    await page.locator('#rounds').fill(String(values.rounds));
    await page.locator('#run').click();
    await page.waitForFunction(
      () => {
        const text = document.querySelector('#raw-results')?.textContent ?? '';
        return text.includes('"status"');
      },
      { timeout: 120000 },
    );
    const rawText = await page.locator('#raw-results').textContent();
    const report = JSON.parse(rawText);
    assert.equal(report.status, 'passed', `Resident bench failed: ${report.error ?? rawText}`);
    await writeFile(resolve(values.output), JSON.stringify(report, null, 2) + '\n');
    console.log(
      JSON.stringify({
        status: report.status,
        adapter: report.adapter,
        workloads: report.workloads.length,
        controls: report.controls.length,
      }),
    );
  } finally {
    await browser.close();
  }
} finally {
  server.closeAllConnections?.();
  await new Promise(done => server.close(done));
}
