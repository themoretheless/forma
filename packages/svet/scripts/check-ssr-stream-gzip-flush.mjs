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
const report = { startedAt: new Date().toISOString(), node: process.version, cycles: [] };
let root;
async function control(op, id) {
  const res = await fetch(`${root}${op}?id=${id}`, { signal: AbortSignal.timeout(5000) });
  assert(res.ok);
  return res.json();
}
async function settled(id) {
  for (let i = 0; i < 1000; i++) {
    const s = await control('stats', id);
    if (s.finished && s.pipelineSettled && s.compressorClosed) return s;
    await new Promise(resolve => setTimeout(resolve, 1));
  }
  throw Error('Flush request failed to settle');
}
async function run(id, codec, inject) {
  const res = await fetch(
    `${root}open?id=${id}&gzip=1&varied=1${inject ? '&abort-flush=patch' : ''}`,
    { headers: { [e.SSR_STREAM_CODEC_HEADER]: codec }, signal: AbortSignal.timeout(10000) },
  );
  e.assertSsrStreamResponse(res, { codec });
  const reader = res.body.getReader(),
    decoder = new TextDecoder();
  let text = '';
  try {
    const first = await reader.read();
    assert(!first.done);
    text += decoder.decode(first.value, { stream: true });
    assert.equal(JSON.parse(text.trim()).payload.type, 'shell');
    await control('release', id);
    let failure;
    try {
      while (true) {
        const part = await reader.read();
        if (part.done) break;
        text += decoder.decode(part.value, { stream: true });
      }
      text += decoder.decode();
    } catch (error) {
      failure = String(error);
    }
    const state = await settled(id);
    assert.equal(state.nodes, 0);
    assert.equal(state.cleaned, 1);
    if (inject) {
      assert(failure, 'Aborted compressed response must fail');
      assert(
        state.flushInjection?.pending &&
          state.flushInjection.phase === 'flush' &&
          state.flushInjection.nodes > 0 &&
          state.flushInjection.flushes === 1,
      );
      assert.equal(state.phase, 'failed');
    } else {
      assert.equal(failure, undefined);
      const decode = codec === 'identity' ? null : e.createSsrStreamTransportDecoder();
      const chunks = text
        .trim()
        .split('\n')
        .map(line => {
          const frame = JSON.parse(line);
          return decode ? decode.decode(frame) : frame;
        });
      decode?.dispose();
      assert.deepEqual(
        chunks.map(c => c.sequence),
        [0, 1, 2],
      );
      assert.equal(state.flushes, 3);
    }
    return state;
  } finally {
    await reader.cancel().catch(() => {});
    await control('forget', id);
  }
}
try {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  root = `http://127.0.0.1:${server.address().port}/__svet_stream/`;
  for (let cycle = 0; cycle < 6; cycle++)
    for (const codec of ['identity', e.SSR_STREAM_HTML_REFERENCE_CODEC]) {
      const id = `flush-${cycle}-${codec}`,
        aborted = await run(id, codec, true),
        recovered = await run(id + '-fresh', codec, false);
      report.cycles.push({ cycle, codec, aborted, recovered });
    }
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
    'Deterministic host abort immediately after real native patch gzip.flush returns, before callback; six abort/recovery cycles per codec over loopback HTTP. Not remotely timed flush race, browser/CDN or long memory plateau.';
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(
    JSON.stringify({ status: report.status, cycles: report.cycles.length, error: report.error }),
  );
}
