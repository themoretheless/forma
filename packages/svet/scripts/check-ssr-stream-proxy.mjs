import assert from 'node:assert/strict';
import { createServer, request as httpRequest } from 'node:http';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { createResumptionFixture, createStreamingFixture } from './browser-fixture-endpoints.mjs';
const runtime = process.argv[3]
  ? pathToFileURL(resolve(process.argv[3], 'dist/package') + '/')
  : new URL('../dist/package/', import.meta.url);
const engine = await import(new URL('index.js', runtime)),
  renderer = await import(new URL('server.js', runtime));
const host = createResumptionFixture(engine),
  fixture = createStreamingFixture({
    ...engine,
    renderToChunksAsync: renderer.renderToChunksAsync,
  });
const origin = createServer((req, res) => fixture.handle(req, res, () => res.end()));
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const report = {
  startedAt: new Date().toISOString(),
  node: process.version,
  cases: [],
  maxBufferedBytes: 0,
  relayWrites: 0,
  relayDrains: 0,
  propagatedDisconnects: 0,
};
let originRoot;
const proxy = createServer((req, res) => {
  const url = new URL(req.url, 'http://proxy'),
    buffered = url.searchParams.get('buffered') === '1';
  const upstream = httpRequest(originRoot + url.pathname + url.search, up => {
    res.writeHead(up.statusCode, up.headers);
    void (async () => {
      try {
        if (buffered) {
          const parts = [];
          let bytes = 0;
          for await (const part of up) {
            bytes += part.length;
            assert(bytes <= 1024 * 1024, 'Proxy buffer cap');
            parts.push(part);
            report.maxBufferedBytes = Math.max(report.maxBufferedBytes, bytes);
          }
          if (!res.destroyed) res.end(Buffer.concat(parts));
        } else {
          for await (const part of up) {
            for (let offset = 0; offset < part.length; offset += 4096) {
              if (res.destroyed) throw Error('Downstream closed');
              report.relayWrites++;
              if (!res.write(part.subarray(offset, offset + 4096))) {
                report.relayDrains++;
                await new Promise((resolve, reject) => {
                  const cleanup = () => {
                    res.off('drain', done);
                    res.off('close', closed);
                  };
                  const done = () => {
                      cleanup();
                      resolve();
                    },
                    closed = () => {
                      cleanup();
                      reject(Error('Downstream closed'));
                    };
                  res.once('drain', done);
                  res.once('close', closed);
                });
              }
              await pause(2);
            }
          }
          res.end();
        }
      } catch (error) {
        upstream.destroy();
        res.destroy(error);
      }
    })();
  });
  upstream.on('error', () => res.destroy());
  res.on('error', () => {});
  res.once('close', () => {
    if (!res.writableEnded) {
      report.propagatedDisconnects++;
      upstream.destroy();
    }
  });
  upstream.end();
});
async function listen(server) {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  return `http://127.0.0.1:${server.address().port}`;
}
async function control(op, id) {
  const response = await fetch(`${originRoot}/__svet_stream/${op}?id=${id}`, {
    signal: AbortSignal.timeout(5000),
  });
  assert(response.ok);
  return response.json();
}
async function waitState(id, predicate) {
  for (let i = 0; i < 500; i++) {
    const state = await control('stats', id);
    if (predicate(state)) return state;
    await pause(2);
  }
  throw Error('Origin state deadline');
}
try {
  originRoot = await listen(origin);
  const proxyRoot = await listen(proxy);
  for (const buffered of [false, true])
    for (const cancel of [false, true]) {
      const id = `proxy-${buffered}-${cancel}`,
        abort = new AbortController();
      let firstByte = false,
        text = '';
      const pending = (async () => {
        const response = await fetch(
          `${proxyRoot}/__svet_stream/open?id=${id}&buffered=${buffered ? 1 : 0}`,
          { signal: AbortSignal.any([abort.signal, AbortSignal.timeout(10000)]) },
        );
        const reader = response.body.getReader(),
          decoder = new TextDecoder();
        try {
          while (true) {
            const part = await reader.read();
            if (part.done) break;
            firstByte = true;
            text += decoder.decode(part.value, { stream: true });
            await pause(5);
          }
          text += decoder.decode();
        } finally {
          await reader.cancel().catch(() => {});
        }
      })();
      const observed = pending.then(
        () => ({ ok: true }),
        error => ({ error: String(error) }),
      );
      const pendingState = await waitState(id, s => s.started);
      assert(!pendingState.finished && pendingState.nodes > 0);
      if (!buffered) for (let i = 0; i < 500 && !firstByte; i++) await pause(2);
      else await pause(40);
      assert.equal(firstByte, !buffered, 'Proxy early shell contract');
      if (cancel) abort.abort(Error('Client disconnected at proxy'));
      else await control('release', id);
      const result = await observed;
      if (cancel) assert(result.error);
      else {
        assert(result.ok);
        const chunks = text.trim().split('\n').map(JSON.parse);
        assert.deepEqual(
          chunks.map(c => c.sequence),
          [0, 1, 2],
        );
        assert.deepEqual(
          chunks.map(c => c.payload.type),
          ['shell', 'patch', 'complete'],
        );
        assert(chunks.every(c => c.version === 1 && c.streamId === chunks[0].streamId));
      }
      const state = await waitState(id, s => s.finished);
      assert.equal(state.nodes, 0);
      assert.equal(state.cleaned, 1);
      report.cases.push({
        buffered,
        cancel,
        earlyShell: !buffered,
        bytes: Buffer.byteLength(text),
        origin: state,
      });
      await control('forget', id);
    }
  assert.equal(report.propagatedDisconnects, 2);
  assert.equal(engine.getGraphTelemetry().nodes, 0);
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  fixture.close();
  for (const server of [proxy, origin]) {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
  host.close();
  report.completedAt = new Date().toISOString();
  report.scope =
    'Four loopback proxy cases, throttled 4KiB relay and explicitly capped full-response buffering. Not nginx/CDN, TCP saturation, compression, HTTP2/3 or TLS qualification.';
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  report.runtimeSha256 = {};
  for (const file of ['server.js', 'stream.js', 'reactivity.js'])
    report.runtimeSha256[file] = createHash('sha256')
      .update(await readFile(new URL(file, runtime)))
      .digest('hex');
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
}
