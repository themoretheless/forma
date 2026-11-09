import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { createResumptionFixture, createStreamingFixture } from './browser-fixture-endpoints.mjs';
const base = process.argv[2]
  ? pathToFileURL(resolve(process.argv[2], 'dist/package') + '/')
  : new URL('../dist/package/', import.meta.url);
const engine = await import(new URL('index.js', base)),
  serverEngine = await import(new URL('server.js', base));
const host = createResumptionFixture(engine),
  streams = createStreamingFixture({
    ...engine,
    renderToChunksAsync: serverEngine.renderToChunksAsync,
  });
const server = createServer((req, res) =>
  streams.handle(req, res, () => {
    res.writeHead(404);
    res.end();
  }),
);
const codec = process.argv[4] ?? 'identity',
  gzip = process.argv[5] === 'gzip',
  query = gzip ? '&gzip=1' : '';
const headers = { [engine.SSR_STREAM_CODEC_HEADER]: codec };
const report = {
  codec,
  gzip,
  startedAt: new Date().toISOString(),
  node: process.version,
  checks: [],
};
const bounded = async promise => {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise(
        (_, reject) =>
          (timer = setTimeout(() => reject(Error('Stream HTTP deadline exceeded')), 10000)),
      ),
    ]);
  } finally {
    clearTimeout(timer);
  }
};
async function readShell(reader) {
  const decoder = new TextDecoder();
  let text = '';
  while (!text.includes('\n')) {
    const part = await bounded(reader.read());
    assert(!part.done, 'HTTP ended before shell frame');
    text += decoder.decode(part.value, { stream: true });
  }
  const split = text.indexOf('\n');
  assert.equal(text.slice(split + 1), '', 'Unexpected frame before controlled component release');
  assert.equal(JSON.parse(text.slice(0, split)).payload.type, 'shell');
  return text;
}
try {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const root = `http://127.0.0.1:${server.address().port}/__svet_stream/`;
  const request = async (op, id) => {
    const response = await fetch(`${root}${op}?id=${id}`, { signal: AbortSignal.timeout(5000) });
    assert(response.ok);
    return response.json();
  };
  const abort = new AbortController(),
    response = await bounded(
      fetch(root + 'open?id=cancel' + query, { signal: abort.signal, headers }),
    ),
    reader = response.body.getReader();
  await readShell(reader);
  report.pending = await request('stats', 'cancel');
  assert(!report.pending.finished && report.pending.nodes > 0);
  const pending = reader.read();
  const rejection = assert.rejects(pending);
  abort.abort(Error('client body disconnected'));
  await bounded(rejection);
  await bounded(
    (async () => {
      while (true) {
        const state = await request('stats', 'cancel');
        if (state.finished) {
          report.cancelled = state;
          break;
        }
        await new Promise(setImmediate);
      }
    })(),
  );
  assert.equal(report.cancelled.nodes, 0);
  assert.equal(report.cancelled.cleaned, 1);
  report.checks.push('native HTTP body disconnect cancels pending next and releases request graph');
  await request('forget', 'cancel');
  const fresh = await bounded(fetch(root + 'open?id=fresh' + query, { headers })),
    freshReader = fresh.body.getReader(),
    decoder = new TextDecoder();
  let text = '';
  text = await readShell(freshReader);
  await request('release', 'fresh');
  for (
    let part = await bounded(freshReader.read());
    !part.done;
    part = await bounded(freshReader.read())
  )
    text += decoder.decode(part.value, { stream: true });
  text += decoder.decode();
  if (codec !== 'identity') {
    assert.throws(() => engine.assertSsrStreamResponse(fresh), /codec/);
    report.checks.push('identity client rejects encoded response headers before reading frames');
  }
  engine.assertSsrStreamResponse(fresh, { codec });
  const frames = text
    .trim()
    .split('\n')
    .map(line => JSON.parse(line));
  const transportDecoder = codec === 'identity' ? null : engine.createSsrStreamTransportDecoder();
  if (gzip) {
    assert.equal(fresh.headers.get('content-encoding'), 'gzip');
  }
  const chunks = frames.map(frame => (transportDecoder ? transportDecoder.decode(frame) : frame));
  transportDecoder?.dispose();
  report.encodedBytes = frames.map(frame => Buffer.byteLength(JSON.stringify(frame)));
  if (codec !== 'identity') assert('codec' in frames[2]);
  assert.deepEqual(
    chunks.map(c => c.payload.type),
    ['shell', 'patch', 'complete'],
  );
  assert.deepEqual(
    chunks.map(c => c.version),
    [engine.SSR_STREAM_VERSION, engine.SSR_STREAM_VERSION, engine.SSR_STREAM_VERSION],
  );
  assert.deepEqual(
    chunks.map(c => c.sequence),
    [0, 1, 2],
  );
  assert.equal(chunks[0].streamId, chunks[1].streamId);
  assert.equal(chunks[1].streamId, chunks[2].streamId);
  report.completed = await request('stats', 'fresh');
  assert(
    report.completed.finished &&
      report.completed.nodes === 0 &&
      report.completed.cleaned === 1 &&
      report.completed.drains > 0,
  );
  if (gzip) {
    assert.equal(report.completed.flushes, 3);
    assert(report.completed.compressedBytes > 0);
    report.checks.push('gzip shell delivered before gate and all three frames explicitly flushed');
  }
  report.chunkTypes = chunks.map(c => c.payload.type);
  report.chunkBytes = chunks.map(c => Buffer.byteLength(JSON.stringify(c)));
  report.checks.push(
    'fresh progressive stream succeeds with real writer drain after a cancelled request',
  );
  await request('forget', 'fresh');
  assert.equal(engine.getGraphTelemetry().nodes, 0);
  const unsupported = await fetch(root + 'open?id=unsupported-codec', {
    headers: { [engine.SSR_STREAM_CODEC_HEADER]: 'future' },
    signal: AbortSignal.timeout(5000),
  });
  assert.equal(unsupported.status, 406);
  await unsupported.body.cancel();
  assert.equal((await request('stats', 'unsupported-codec')).started, false);
  report.checks.push('unsupported codec rejected before request state or SSR creation');
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.error = String(error.stack ?? error);
  process.exitCode = 1;
} finally {
  streams.close();
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  host.close();
  report.completedAt = new Date().toISOString();
  report.sourceSha256 = createHash('sha256')
    .update(await readFile(new URL(import.meta.url)))
    .digest('hex');
  report.runtimeSha256 = {};
  for (const file of ['server.js', 'stream.js', 'dom.js', 'reactivity.js'])
    report.runtimeSha256[file] = createHash('sha256')
      .update(await readFile(new URL(file, base)))
      .digest('hex');
  if (process.argv[3]) await writeFile(process.argv[3], JSON.stringify(report, null, 2) + '\n');
  console.log(
    JSON.stringify({ status: report.status, error: report.error, checks: report.checks.length }),
  );
}
