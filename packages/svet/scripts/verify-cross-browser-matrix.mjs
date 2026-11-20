import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { extname, resolve, dirname, normalize } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const svetRoot = resolve(scriptDir, '..');
const repoRoot = resolve(svetRoot, '../..');

const { values } = parseArgs({
  options: {
    'playwright-dir': { type: 'string' },
    'copy-manifest': { type: 'string' },
    'matrix-manifest': {
      type: 'string',
      default: resolve(svetRoot, 'node_modules/.cache/svet-browser-matrix/latest.json'),
    },
    output: {
      type: 'string',
      default: resolve(svetRoot, 'benchmarks/cross-browser-matrix-qualification-2026-11-20.json'),
    },
  },
});

const playwrightDir = values['playwright-dir'] ?? process.env.PLAYWRIGHT_MODULE_DIR;
assert(playwrightDir, 'Provide --playwright-dir or PLAYWRIGHT_MODULE_DIR');
const copyManifestPath = values['copy-manifest'] ?? process.env.SVET_COPY_MANIFEST;
assert(copyManifestPath, 'Provide --copy-manifest or SVET_COPY_MANIFEST');

const { chromium, webkit, firefox } = await import(
  pathToFileURL(resolve(playwrightDir, 'node_modules/playwright/index.mjs')).href
);

const matrixManifest = JSON.parse(await readFile(resolve(values['matrix-manifest']), 'utf8'));
const copyManifest = JSON.parse(await readFile(resolve(copyManifestPath), 'utf8'));
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.wasm': 'application/wasm',
  '.svg': 'image/svg+xml',
};

function startStaticServer(resolvePath, label = 'static') {
  const server = createServer(async (req, res) => {
    try {
      const reqUrl = new URL(req.url ?? '/', 'http://127.0.0.1');
      if (req.method === 'POST' && reqUrl.pathname === '/result') {
        req.resume();
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end('{}');
        return;
      }
      const target = resolvePath(reqUrl.pathname);
      if (!target) {
        console.error(`[${label} 404] ${reqUrl.pathname}${reqUrl.search}`);
        res.writeHead(404);
        res.end('Not found');
        return;
      }
      const body = await readFile(target);
      if (reqUrl.searchParams.has('raw')) {
        res.writeHead(200, {
          'content-type': 'text/javascript; charset=utf-8',
          'cache-control': 'no-store',
        });
        res.end(`export default ${JSON.stringify(body.toString('utf8'))};\n`);
        return;
      }
      res.writeHead(200, {
        'content-type': MIME[extname(target)] ?? 'application/octet-stream',
        'cache-control': 'no-store',
      });
      res.end(body);
    } catch (err) {
      console.error(`[${label} err]`, err);
      res.writeHead(404);
      res.end('Not found');
    }
  });
  return new Promise((resolvePromise, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      resolvePromise({
        url: `http://127.0.0.1:${address.port}`,
        close: () =>
          new Promise(done => {
            server.closeAllConnections?.();
            server.close(() => done());
          }),
      });
    });
  });
}

function safeUnder(base, rel) {
  const cleaned = normalize(rel).replace(/^[/\\]+/, '');
  const full = resolve(base, cleaned);
  return full === base || full.startsWith(base + '/') ? full : null;
}

const formaServer = await startStaticServer(pathname => {
  if (pathname.startsWith('/wasm/')) {
    return safeUnder(resolve(repoRoot, 'public/vector-pkg'), pathname.slice('/wasm/'.length));
  }
  if (pathname.startsWith('/vector-pkg/')) {
    return safeUnder(resolve(repoRoot, 'public/vector-pkg'), pathname.slice('/vector-pkg/'.length));
  }
  if (pathname.startsWith('/src/')) {
    return safeUnder(resolve(repoRoot, 'src'), pathname.slice('/src/'.length));
  }
  if (pathname === '/crates/forma/src/vector.wgsl') {
    return resolve(repoRoot, 'crates/forma/src/vector.wgsl');
  }
  if (pathname.startsWith('/crates/forma/tests/')) {
    return safeUnder(
      resolve(repoRoot, 'crates/forma/tests'),
      pathname.slice('/crates/forma/tests/'.length),
    );
  }
  return null;
}, 'forma');

const matrixServer = await startStaticServer(
  pathname => safeUnder(matrixManifest.output, pathname === '/' ? 'index.html' : pathname.slice(1)),
  'matrix',
);

const routerServer = await startStaticServer(pathname => {
  if (pathname === '/tests/browser/cross-copy-router.html') {
    return resolve(svetRoot, 'tests/browser/cross-copy-router.html');
  }
  return safeUnder(copyManifest.output, pathname.slice(1));
}, 'router');

async function runCrossBrowserInputProfile(browser, engineName, profilePlatform) {
  const context = await browser.newContext();
  try {
    if (profilePlatform) {
      await context.addInitScript(platform => {
        Object.defineProperty(navigator, 'platform', { get: () => platform, configurable: true });
        Object.defineProperty(navigator, 'userAgentData', {
          get: () => ({ platform }),
          configurable: true,
        });
      }, profilePlatform);
    }
    const page = await context.newPage();
    page.on('pageerror', err => console.error(`[${engineName} pageerror]`, err));
    page.on('console', msg => {
      if (msg.type() === 'error') console.error(`[${engineName} console.error]`, msg.text());
    });
    await page.goto(`${formaServer.url}/crates/forma/tests/preview-cross-browser-input.html`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(
      () => {
        const text = document.querySelector('#status')?.textContent ?? '';
        return text === 'READY' || text.startsWith('ERROR:');
      },
      { timeout: 15000 },
    );
    const statusText = await page.locator('#status').textContent();
    assert.equal(statusText, 'READY', `${engineName} status: ${statusText}`);

    // Focus the canvas/textarea via pointer tap in viewport so real keyboard events have browser focus.
    await page.evaluate(() => window.__formaInputHarness.focusKey('field'));
    await page.locator('textarea').focus();

    const synthetic = await page.evaluate(() => window.__formaInputHarness.runSyntheticMatrix());
    assert(
      synthetic.passed,
      `${engineName} (${profilePlatform ?? 'native'}) synthetic fail: ${JSON.stringify(synthetic)}`,
    );

    const apple = synthetic.apple;
    const wordMod = apple ? 'Alt' : 'Control';

    // 1. Trusted word Shift+ArrowLeft selection.
    await page.evaluate(() => window.__formaInputHarness.resetField('alpha beta'));
    await page.locator('textarea').focus();
    await page.keyboard.press(`${wordMod}+Shift+ArrowLeft`);
    const selState = await page.evaluate(() => ({
      state: window.__formaInputHarness.getState(),
      events: window.__formaInputHarness.getEvents(),
    }));
    assert.equal(selState.state.selected, 'beta', `${engineName} word Shift+ArrowLeft`);
    assert(
      selState.events.some(
        e => e.key === 'ArrowLeft' && e.isTrusted === true && e.defaultPrevented,
      ),
      `${engineName} (${profilePlatform ?? 'native'}) expected trusted ArrowLeft keydown`,
    );

    // 2. Trusted word backward deletion.
    await page.evaluate(() => window.__formaInputHarness.resetField('alpha beta'));
    await page.locator('textarea').focus();
    await page.keyboard.press(`${wordMod}+Backspace`);
    const backState = await page.evaluate(() => ({
      state: window.__formaInputHarness.getState(),
      events: window.__formaInputHarness.getEvents(),
    }));
    assert.equal(backState.state.value, 'alpha ', `${engineName} word Backspace`);
    assert(
      backState.events.some(
        e => e.key === 'Backspace' && e.isTrusted === true && e.defaultPrevented,
      ),
      `${engineName} expected trusted Backspace keydown`,
    );

    // 3. Trusted Home + word forward deletion.
    await page.evaluate(() => window.__formaInputHarness.resetField('alpha beta'));
    await page.locator('textarea').focus();
    await page.keyboard.press('Home');
    await page.keyboard.press(`${wordMod}+Delete`);
    const delState = await page.evaluate(() => window.__formaInputHarness.getState());
    assert.equal(delState.value, ' beta', `${engineName} word Delete`);

    // 4. Chromium CDP real IME protocol check when running in Chromium on default native profile.
    let cdpImeChecked = false;
    if (engineName === 'chromium' && !profilePlatform) {
      await page.evaluate(() => window.__formaInputHarness.resetField('hi '));
      await page.locator('textarea').focus();
      const cdp = await context.newCDPSession(page);
      await cdp.send('Input.imeSetComposition', {
        text: 'nihon',
        selectionStart: 5,
        selectionEnd: 5,
      });
      await cdp.send('Input.insertText', { text: '日本' });
      const imeState = await page.evaluate(() => window.__formaInputHarness.getState());
      assert(imeState.value.includes('hi 日本'), `CDP IME failed: ${JSON.stringify(imeState)}`);
      cdpImeChecked = true;
    }

    return {
      profile: profilePlatform ?? 'native',
      apple,
      syntheticCases: synthetic.cases.length,
      trustedWordSelection: selState.state.selected,
      trustedWordBackspace: backState.state.value,
      trustedWordDelete: delState.value,
      cdpImeChecked,
      status: 'passed',
    };
  } finally {
    await context.close();
  }
}

const engines = [
  ['chromium', chromium],
  ['webkit', webkit],
  ['firefox', firefox],
];

const report = {
  protocol: 'cross-browser-matrix-v2',
  startedAt: new Date().toISOString(),
  engines: {},
};

try {
  for (const [name, browserType] of engines) {
    const browser = await browserType.launch({ headless: true });
    try {
      const version = browser.version();
      const inputProfiles = [];
      for (const profile of [null, 'Linux', 'Win32']) {
        inputProfiles.push(await runCrossBrowserInputProfile(browser, name, profile));
      }

      // Run existing 17-case preview-word-browser.html fixture.
      const wordContext = await browser.newContext();
      let wordReport;
      try {
        const wordPage = await wordContext.newPage();
        wordPage.on('pageerror', err => console.error(`[${name} word pageerror]`, err));
        await wordPage.goto(`${formaServer.url}/crates/forma/tests/preview-word-browser.html`, {
          waitUntil: 'domcontentloaded',
        });
        await wordPage.waitForFunction(() => {
          const s = document.querySelector('#status')?.textContent ?? '';
          return s === 'PASSED' || s === 'FAILED';
        });
        const wordStatus = await wordPage.locator('#status').textContent();
        wordReport = JSON.parse(await wordPage.locator('#report').textContent());
        assert.equal(wordStatus, 'PASSED', `${name} word fail: ${JSON.stringify(wordReport)}`);
        assert.equal(wordReport.status, 'passed');
        assert.equal(wordReport.cases.length, 17);
      } finally {
        await wordContext.close();
      }

      // Run frozen Svet browser-matrix.html (21 iframe fixtures, 189 native assertions).
      const matrixContext = await browser.newContext();
      let matrixReport;
      try {
        const matrixPage = await matrixContext.newPage();
        await matrixPage.goto(`${matrixServer.url}/tests/browser/browser-matrix.html`, {
          waitUntil: 'networkidle',
        });
        await matrixPage.locator('#run').click();
        await matrixPage.waitForFunction(
          () => {
            const text = document.querySelector('#raw-results')?.textContent ?? '';
            return text.includes('"status"');
          },
          { timeout: 45000 },
        );
        matrixReport = JSON.parse(await matrixPage.locator('#raw-results').textContent());
        assert.equal(matrixReport.status, 'passed', JSON.stringify(matrixReport));
        assert.equal(matrixReport.cases.length, 21);
      } finally {
        await matrixContext.close();
      }

      // Run UNPACED cross-copy-router.html (19 cases, verifying S-17 coalescing).
      // WebKit enforces a strict 100 pushState/replaceState calls per 10s limit per page,
      // so on WebKit we execute both burst directions across two fresh unpaced pages (still 19/19 distinct cases).
      const routerRuns =
        name === 'webkit'
          ? [
              { query: '?burst=A%E2%86%92B', expectedCases: 13 },
              { query: '?burst=B%E2%86%92A', expectedCases: 13 },
            ]
          : [{ query: '', expectedCases: 19 }];
      const passedRouterCaseNames = new Set();
      let routerTotalCases = 0;
      for (const runSpec of routerRuns) {
        const routerContext = await browser.newContext();
        try {
          const routerPage = await routerContext.newPage();
          routerPage.on('pageerror', err => console.error(`[${name} router pageerror]`, err));
          routerPage.on('console', msg => {
            if (msg.type() === 'error') console.error(`[${name} router console.error]`, msg.text());
          });
          await routerPage.goto(
            `${routerServer.url}/tests/browser/cross-copy-router.html${runSpec.query}`,
            {
              waitUntil: 'domcontentloaded',
            },
          );
          await routerPage.locator('#run').click();
          try {
            await routerPage.waitForFunction(
              () => {
                const text = document.querySelector('#raw-results')?.textContent ?? '';
                return text.includes('"status"');
              },
              { timeout: 30000 },
            );
          } catch (err) {
            const partial = await routerPage.evaluate(() => ({
              result: document.querySelector('#result')?.textContent,
              rawLen: document.querySelector('#raw-results')?.textContent?.length,
            }));
            throw Error(`${name} router timeout: ${JSON.stringify(partial)} (${err.message})`);
          }
          const routerReport = JSON.parse(await routerPage.locator('#raw-results').textContent());
          assert.equal(
            routerReport.status,
            'passed',
            `${name} unpaced router failed: ${JSON.stringify(routerReport.cases)}`,
          );
          assert.equal(routerReport.cases.length, runSpec.expectedCases);
          for (const row of routerReport.cases) passedRouterCaseNames.add(row.name);
          routerTotalCases = passedRouterCaseNames.size;
        } finally {
          await routerContext.close();
        }
      }
      assert.equal(routerTotalCases, 19);

      console.error(`[matrix] completed engine=${name}`);

      report.engines[name] = {
        status: 'passed',
        version,
        userAgent: matrixReport.userAgent,
        previewInputProfiles: inputProfiles,
        previewWordCases: wordReport.cases.length,
        svetBrowserMatrixFixtures: matrixReport.cases.length,
        svetBrowserMatrixAssertions: matrixReport.cases.reduce((n, row) => n + row.passed, 0),
        svetCrossCopyRouterUnpacedCases: routerTotalCases,
        svetCrossCopyRouterMode:
          name === 'webkit'
            ? 'unpaced-split-per-direction-under-webkit-100calls-10s-quota'
            : 'unpaced-single-page-19of19',
      };
    } finally {
      await browser.close();
    }
  }

  report.status = 'passed';
  report.completedAt = new Date().toISOString();
  report.sha256 = {
    'src/vector-preview.js': sha256(await readFile(resolve(repoRoot, 'src/vector-preview.js'))),
    'crates/forma/tests/preview-cross-browser-input.html': sha256(
      await readFile(resolve(repoRoot, 'crates/forma/tests/preview-cross-browser-input.html')),
    ),
    'packages/svet/dist/package/advanced.js': sha256(
      await readFile(resolve(svetRoot, 'dist/package/advanced.js')),
    ),
    'packages/svet/dist/package/svet-core.wasm': sha256(
      await readFile(resolve(svetRoot, 'dist/package/svet-core.wasm')),
    ),
  };

  await writeFile(resolve(values.output), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
} finally {
  await formaServer.close();
  await matrixServer.close();
  await routerServer.close();
}
