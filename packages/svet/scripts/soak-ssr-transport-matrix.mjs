import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile, writeFile, rename } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { createResumptionFixture, createStreamingFixture } from './browser-fixture-endpoints.mjs';
const e = await import('../dist/package/index.js'),
  r = await import('../dist/package/server.js');
const durationMs = Number(process.argv[3] ?? 60000);
assert(Number.isSafeInteger(durationMs) && durationMs >= 1000 && durationMs <= 86400000);
const host = createResumptionFixture(e),
  fixture = createStreamingFixture({ ...e, renderToChunksAsync: r.renderToChunksAsync });
const server = createServer((req, res) => fixture.handle(req, res, () => res.end()));
const modes = ['complete', 'shell-disconnect', 'patch-disconnect', 'patch-abort'];
const variants = ['identity', e.SSR_STREAM_HTML_REFERENCE_CODEC].flatMap(codec =>
  [false, true].flatMap(gzip => modes.map(mode => ({ codec, gzip, mode }))),
);
const report = {
  status: 'running',
  startedAt: new Date().toISOString(),
  node: process.version,
  durationMs,
  concurrency: 4,
  forcedGCs: 0,
  rounds: 0,
  requests: 0,
  cases: {},
  samples: [],
  scope:
    'Loopback fetch delayed reads, both codecs, gzip on/off, shell disconnect and observed pending large patch disconnect/host abort. No sustained kernel saturation, remote proxy or universal memory bound claim.',
};
let root,
  lastCheckpoint = 0,
  sampleIndex = 0;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const hash = async path =>
  createHash('sha256')
    .update(await readFile(new URL(path, import.meta.url)))
    .digest('hex');
async function identities() {
  return Object.fromEntries(
    await Promise.all(
      [
        './soak-ssr-transport-matrix.mjs',
        './browser-fixture-endpoints.mjs',
        '../dist/package/server.js',
        '../dist/package/stream.js',
        '../dist/package/reactivity.js',
      ].map(async path => [path, await hash(path)]),
    ),
  );
}
async function checkpoint() {
  if (!process.argv[2]) return;
  report.updatedAt = new Date().toISOString();
  const path = process.argv[2] + '.progress.json';
  await writeFile(path + '.tmp', JSON.stringify(report, null, 2) + '\n');
  await rename(path + '.tmp', path);
}
async function control(op, id) {
  const response = await fetch(`${root}${op}?id=${id}`, { signal: AbortSignal.timeout(15000) });
  assert(response.ok);
  return response.json();
}
async function wait(id, predicate) {
  for (let i = 0; i < 3000; i++) {
    const state = await control('stats', id);
    if (predicate(state)) return state;
    await pause(1);
  }
  throw Error('Transport state deadline: ' + id);
}
async function run(variant, id) {
  const { codec, gzip, mode } = variant,
    key = `${codec}/${gzip ? 'gzip' : 'plain'}/${mode}`,
    pending = mode.startsWith('patch-');
  const response = await fetch(
    `${root}open?id=${id}&gzip=${gzip ? 1 : 0}&payload=${pending ? 4194304 : 262144}&varied=1`,
    { headers: { [e.SSR_STREAM_CODEC_HEADER]: codec }, signal: AbortSignal.timeout(30000) },
  );
  e.assertSsrStreamResponse(response, { codec });
  assert.equal(response.headers.get('content-encoding'), gzip ? 'gzip' : null);
  const reader = response.body.getReader(),
    textDecoder = new TextDecoder();
  let text = '';
  try {
    while (!text.includes('\n')) {
      const part = await reader.read();
      assert(!part.done);
      text += textDecoder.decode(part.value, { stream: true });
    }
    const wireDecoder = codec === 'identity' ? null : e.createSsrStreamTransportDecoder();
    try {
      const shellFrame = JSON.parse(text.slice(0, text.indexOf('\n'))),
        shell = wireDecoder ? wireDecoder.decode(shellFrame) : shellFrame;
      assert.equal(shell.payload.type, 'shell');
      if (mode === 'shell-disconnect') await reader.cancel();
      else {
        await control('release', id);
        if (pending) {
          await wait(
            id,
            s =>
              ['drain', 'flush'].includes(s.phase) &&
              s.nodes > 0 &&
              (!gzip || (s.flushes === 1 && s.compressedBytes > 4096)),
          );
          if (mode === 'patch-abort') await control('abort', id);
          else await reader.cancel();
        } else {
          while (true) {
            await pause(5);
            const part = await reader.read();
            if (part.done) break;
            text += textDecoder.decode(part.value, { stream: true });
          }
          text += textDecoder.decode();
          const frames = text.trim().split('\n').map(JSON.parse),
            chunks = [shell];
          for (const frame of frames.slice(1))
            chunks.push(wireDecoder ? wireDecoder.decode(frame) : frame);
          assert(
            chunks.every(c => c.version === e.SSR_STREAM_VERSION && c.streamId === shell.streamId),
          );
          assert.deepEqual(
            chunks.map(c => c.sequence),
            [0, 1, 2],
          );
          assert.equal(chunks[2].payload.type, 'complete');
          assert.equal(JSON.parse(chunks[2].payload.stateJson).signals.count, 7);
          assert(chunks[2].payload.html.includes('&lt;script&gt;unsafe text&lt;/script&gt;'));
        }
      }
    } finally {
      wireDecoder?.dispose();
    }
    const state = await wait(
      id,
      s => s.finished && (!gzip || (s.pipelineSettled && s.compressorClosed)),
    );
    assert.equal(state.nodes, 0);
    assert.equal(state.cleaned, 1);
    assert.equal(state.phase, mode === 'complete' ? 'finished' : 'failed');
    const row = (report.cases[key] ??= { count: 0, drains: 0, flushes: 0 });
    row.count++;
    row.drains += state.drains;
    row.flushes += state.flushes;
    report.requests++;
  } finally {
    await reader.cancel().catch(() => {});
    await control('forget', id);
  }
}
try {
  report.identities = await identities();
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  root = `http://127.0.0.1:${server.address().port}/__svet_stream/`;
  const started = performance.now();
  await checkpoint();
  do {
    // Each wave mixes independent cases; every round covers the full matrix.
    for (let offset = 0; offset < variants.length; offset += 4)
      await Promise.all(
        variants
          .slice(offset, offset + 4)
          .map((v, i) => run(v, `matrix-${report.rounds}-${offset + i}`)),
      );
    report.rounds++;
    assert.equal(e.getGraphTelemetry().nodes, 0);
    report.elapsedMs = performance.now() - started;
    report.samples[sampleIndex++ % 32] = {
      elapsedMs: report.elapsedMs,
      requests: report.requests,
      ...process.memoryUsage(),
    };
    if (report.elapsedMs - lastCheckpoint >= 5000) {
      await checkpoint();
      lastCheckpoint = report.elapsedMs;
    }
  } while (report.elapsedMs < durationMs);
  assert.equal(Object.keys(report.cases).length, variants.length);
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
  report.finalIdentities = await identities();
  report.artifactsUnchanged =
    JSON.stringify(report.identities) === JSON.stringify(report.finalIdentities);
  if (!report.artifactsUnchanged) {
    report.status = 'failed';
    report.error = 'Artifacts changed during soak';
    process.exitCode = 1;
  }
  report.completedAt = new Date().toISOString();
  await checkpoint();
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
}
