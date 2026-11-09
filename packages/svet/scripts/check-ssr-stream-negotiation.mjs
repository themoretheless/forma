import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { writeFile, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { createResumptionFixture } from './browser-fixture-endpoints.mjs';
const runtime = process.argv[3]
  ? pathToFileURL(resolve(process.argv[3], 'dist/package') + '/')
  : new URL('../dist/package/', import.meta.url);
const engine = await import(new URL('index.js', runtime)),
  renderer = await import(new URL('server.js', runtime));
const host = createResumptionFixture(engine),
  report = {
    startedAt: new Date().toISOString(),
    node: process.version,
    checks: [],
    renderCalls: 0,
  };
const {
  SSR_STREAM_ACCEPT_HEADER: accept,
  SSR_STREAM_VERSION_HEADER: version,
  SSR_STREAM_CONTENT_TYPE: type,
} = engine;
const server = createServer((req, res) => {
  void (async () => {
    try {
      const offer = req.headers[accept],
        selected = engine.negotiateSsrStreamVersion(typeof offer === 'string' ? offer : null);
      res.setHeader('vary', accept);
      res.setHeader(version, '1');
      if (!selected) {
        res.writeHead(406, { 'content-type': 'text/plain' });
        res.end('No compatible Svet stream version');
        return;
      }
      if (req.url !== '/') {
        res.writeHead(200, {
          [version]: req.url === '/wrong-version' ? '2' : '1',
          'content-type': req.url === '/wrong-type' ? 'text/html' : type,
        });
        res.end('must not parse this body');
        return;
      }
      res.writeHead(200, { 'content-type': type });
      for await (const chunk of renderer.renderToChunksAsync(() => {
        report.renderCalls++;
        return { node: engine.h('main', null, 'negotiated') };
      }))
        res.write(JSON.stringify(chunk) + '\n');
      res.end();
    } catch (error) {
      res.destroy(error);
    }
  })();
});
try {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const root = `http://127.0.0.1:${server.address().port}`;
  const container = document.createElement('div');
  container.innerHTML = '<input value="preserved">';
  const original = container.firstChild;
  for (const offer of [null, '2', '1,foo']) {
    const response = await fetch(root, {
      headers: offer === null ? {} : { [accept]: offer },
      signal: AbortSignal.timeout(5000),
    });
    assert.equal(response.status, 406);
    assert.equal(response.headers.get('vary'), accept);
    assert.throws(() => engine.assertSsrStreamResponse(response));
    await response.body.cancel();
    assert.equal(container.firstChild, original);
    assert.equal(report.renderCalls, 0);
    report.checks.push(`unsupported offer ${offer}`);
  }
  for (const route of ['/wrong-version', '/wrong-type']) {
    const response = await fetch(root + route, {
      headers: { [accept]: '1' },
      signal: AbortSignal.timeout(5000),
    });
    assert.throws(() => engine.assertSsrStreamResponse(response));
    await response.body.cancel();
    assert.equal(container.firstChild, original);
    report.checks.push(route);
  }
  for (const offer of ['1', '2, 1']) {
    const response = await fetch(root, {
      headers: { [accept]: offer },
      signal: AbortSignal.timeout(5000),
    });
    engine.assertSsrStreamResponse(response);
    const receiver = engine.createSsrStreamReceiver(container);
    for (const line of (await response.text()).trim().split('\n')) receiver.apply(JSON.parse(line));
    assert.equal(container.textContent, 'negotiated');
    receiver.dispose();
    report.checks.push(`supported offer ${offer}`);
  }
  assert.equal(report.renderCalls, 2);
  assert.equal(engine.getGraphTelemetry().nodes, 0);
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  host.close();
  report.completedAt = new Date().toISOString();
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  report.runtimeSha256 = {};
  for (const file of ['server.js', 'stream.js', 'stream-protocol.js'])
    report.runtimeSha256[file] = createHash('sha256')
      .update(await readFile(new URL(file, runtime)))
      .digest('hex');
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
}
