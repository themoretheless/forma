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
      default: resolve(svetRoot, 'node_modules/.cache/svet-product-app-bench/latest.json'),
    },
    output: {
      type: 'string',
      default: resolve(svetRoot, 'benchmarks/product-app-bench-2026-11-20.json'),
    },
  },
});

const playwrightDir = values['playwright-dir'] ?? process.env.PLAYWRIGHT_MODULE_DIR;
assert(playwrightDir, 'Provide --playwright-dir or PLAYWRIGHT_MODULE_DIR');

const { chromium, webkit, firefox } = await import(
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

const baseUrl = `http://127.0.0.1:${server.address().port}`;
const report = {
  protocol: 'product-app-cross-browser-v1',
  startedAt: new Date().toISOString(),
  snapshot: manifest.snapshot,
  engines: {},
};

try {
  for (const [name, browserType] of [
    ['chromium', chromium],
    ['webkit', webkit],
    ['firefox', firefox],
  ]) {
    const browser = await browserType.launch({ headless: true });
    try {
      const context = await browser.newContext();
      try {
        const page = await context.newPage();
        await page.goto(`${baseUrl}/tests/browser/product-app-bench.html`, {
          waitUntil: 'networkidle',
        });
        const cdp = name === 'chromium' ? await context.newCDPSession(page) : null;
        await page.locator('#run').click();
        await page.waitForFunction(
          () => {
            const text = document.querySelector('#raw-results')?.textContent ?? '';
            return text.includes('"status"');
          },
          { timeout: 60000 },
        );
        const result = JSON.parse(await page.locator('#raw-results').textContent());
        assert.equal(result.status, 'passed', `${name} bench failed: ${JSON.stringify(result)}`);
        const cdpHeap = cdp ? await cdp.send('Runtime.getHeapUsage') : null;
        report.engines[name] = {
          status: result.status,
          version: browser.version(),
          userAgent: result.userAgent,
          startup: result.startup,
          interactions: result.interactions,
          frameUpdate: result.frameUpdate,
          frameInterval: result.frameInterval,
          memory: result.memory,
          cdpHeapUsageAfterTeardown: cdpHeap,
        };
      } finally {
        await context.close();
      }
    } finally {
      await browser.close();
    }
  }

  report.status = 'passed';
  report.completedAt = new Date().toISOString();
  await writeFile(resolve(values.output), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} finally {
  server.closeAllConnections?.();
  await new Promise(done => server.close(done));
}
