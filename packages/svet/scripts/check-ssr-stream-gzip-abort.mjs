import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { writeFile, readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { createResumptionFixture, createStreamingFixture } from './browser-fixture-endpoints.mjs';
const base = process.argv[3]
  ? pathToFileURL(resolve(process.argv[3], 'dist/package') + '/')
  : new URL('../dist/package/', import.meta.url);
const e = await import(new URL('index.js', base)),
  r = await import(new URL('server.js', base)),
  host = createResumptionFixture(e),
  fixture = createStreamingFixture({ ...e, renderToChunksAsync: r.renderToChunksAsync });
const server = createServer((req, res) => fixture.handle(req, res, () => res.end()));
const report = { startedAt: new Date().toISOString(), node: process.version, cases: [] };
let root;
const pause = () => new Promise(resolve => setTimeout(resolve, 1));
async function control(op, id) {
  const res = await fetch(`${root}${op}?id=${id}`, { signal: AbortSignal.timeout(10000) });
  assert(res.ok);
  return res.json();
}
async function wait(id, predicate) {
  for (let i = 0; i < 2000; i++) {
    const state = await control('stats', id);
    if (predicate(state)) return state;
    await pause();
  }
  throw Error('Gzip state deadline');
}
try {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  root = `http://127.0.0.1:${server.address().port}/__svet_stream/`;
  for (const codec of ['identity', e.SSR_STREAM_HTML_REFERENCE_CODEC])
    for (const mode of ['disconnect', 'external-abort']) {
      const id = `large-${codec}-${mode}`,
        res = await fetch(`${root}open?id=${id}&gzip=1&payload=4194304&varied=1`, {
          headers: { [e.SSR_STREAM_CODEC_HEADER]: codec },
          signal: AbortSignal.timeout(15000),
        });
      e.assertSsrStreamResponse(res, { codec });
      const reader = res.body.getReader();
      try {
        const shell = await reader.read();
        assert(!shell.done);
        assert.equal(
          JSON.parse(new TextDecoder().decode(shell.value).trim()).payload.type,
          'shell',
        );
        await control('release', id);
        const before = await wait(
          id,
          s =>
            ['drain', 'flush'].includes(s.phase) &&
            s.nodes > 0 &&
            s.flushes === 1 &&
            s.compressedBytes > 4096,
        );
        assert(!before.finished && before.payloadSize === 4194304);
        if (mode === 'disconnect') await reader.cancel();
        else await control('abort', id);
        const after = await wait(id, s => s.finished && s.pipelineSettled && s.compressorClosed);
        assert.equal(after.nodes, 0);
        assert.equal(after.cleaned, 1);
        assert.equal(after.phase, 'failed');
        report.cases.push({ codec, mode, before, after });
      } finally {
        await reader.cancel().catch(() => {});
        await control('forget', id);
      }
    }
  // A new compressed response must work after the cancelled large writes.
  const id = 'recovery',
    res = await fetch(`${root}open?id=${id}&gzip=1`, { signal: AbortSignal.timeout(10000) }),
    reader = res.body.getReader();
  let text = new TextDecoder().decode((await reader.read()).value);
  await control('release', id);
  const decoder = new TextDecoder();
  for (let part = await reader.read(); !part.done; part = await reader.read())
    text += decoder.decode(part.value, { stream: true });
  text += decoder.decode();
  assert.deepEqual(
    text
      .trim()
      .split('\n')
      .map(line => JSON.parse(line).sequence),
    [0, 1, 2],
  );
  report.recovery = await wait(id, s => s.finished && s.pipelineSettled);
  assert.equal(report.recovery.nodes, 0);
  assert.equal(report.recovery.flushes, 3);
  await control('forget', id);
  assert.equal(e.getGraphTelemetry().nodes, 0);
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  fixture.close();
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  host.close();
  report.completedAt = new Date().toISOString();
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  report.fixtureSha256 = createHash('sha256')
    .update(await readFile(new URL('./browser-fixture-endpoints.mjs', import.meta.url)))
    .digest('hex');
  report.scope =
    '4MiB deterministic varied HTML, observed pending compressor drain/flush, identity/reference codecs, downstream disconnect and explicit host abort; real loopback HTTP, followed by fresh compressed recovery. Not kernel socket saturation, browser/proxy or long memory plateau.';
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
}
