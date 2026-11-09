import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { gzipSync, gunzipSync } from 'node:zlib';
import { readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { cpus } from 'node:os';
const hash = value => createHash('sha256').update(value).digest('hex');
function fixture(kind) {
  let seed = 1701;
  const chars = [];
  for (let i = 0; i < 65536; i++) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    chars.push(String.fromCharCode(33 + ((seed >>> 16) % 90)));
  }
  const html =
    kind === 'small'
      ? '<p>small</p>'
      : kind === 'repeated'
        ? '<p>' + '🦀<&'.repeat(16000) + '</p>'
        : '<pre>' + chars.join('') + '</pre>';
  return [
    {
      version: 1,
      sequence: 0,
      streamId: 'benchmark',
      payload: { type: 'shell', html: '<main>shell</main>' },
    },
    {
      version: 1,
      sequence: 1,
      streamId: 'benchmark',
      payload: { type: 'patch', boundary: 1, html },
    },
    {
      version: 1,
      sequence: 2,
      streamId: 'benchmark',
      payload: { type: 'complete', html: '<main>shell' + html + '</main>', stateJson: '{}' },
    },
  ];
}
if (process.argv[2] === '--worker') {
  const kind = process.argv[3],
    mode = process.argv[4],
    chunks = fixture(kind),
    e = await import('../dist/package/index.js');
  const memoryBefore = process.memoryUsage();
  let output,
    bytes = 0,
    sampledHeapMax = memoryBefore.heapUsed,
    sampledRssMax = memoryBefore.rss;
  const run = () => {
    const enc = mode.includes('codec') ? e.createSsrStreamTransportEncoder() : null,
      dec = mode.includes('codec') ? e.createSsrStreamTransportDecoder() : null;
    try {
      const frames = chunks.map(c => (enc ? enc.encode(c) : c));
      let body = Buffer.from(frames.map(c => JSON.stringify(c)).join('\n') + '\n');
      if (mode.includes('gzip')) body = gzipSync(body, { level: 6 });
      bytes = body.length;
      if (mode.includes('gzip')) body = gunzipSync(body);
      output = body
        .toString()
        .trim()
        .split('\n')
        .map(line => {
          const frame = JSON.parse(line);
          return dec ? dec.decode(frame) : frame;
        });
    } finally {
      enc?.dispose();
      dec?.dispose();
    }
  };
  for (let i = 0; i < 5; i++) run();
  assert.deepEqual(output, chunks);
  global.gc();
  const postWarmup = process.memoryUsage(),
    times = [];
  for (let i = 0; i < 12; i++) {
    const start = performance.now();
    run();
    times.push(performance.now() - start);
    const memory = process.memoryUsage();
    sampledHeapMax = Math.max(sampledHeapMax, memory.heapUsed);
    sampledRssMax = Math.max(sampledRssMax, memory.rss);
  }
  assert.deepEqual(output, chunks);
  output = undefined;
  global.gc();
  console.log(
    JSON.stringify({
      kind,
      mode,
      bytes,
      times,
      postWarmup,
      sampledHeapMax,
      sampledRssMax,
      postGc: process.memoryUsage(),
      fixtureSha256: hash(JSON.stringify(chunks)),
    }),
  );
} else {
  const report = {
    protocol: 'isolated-codec-buffered-gzip-v1',
    startedAt: new Date().toISOString(),
    node: process.version,
    cpu: cpus()[0]?.model,
    rounds: [],
    scope:
      'Deterministic frozen wire-v1 transport workloads, isolated child per trial; serialize/compress/decompress/parse/codec cost only, no render/DOM/network. Gzip level6 buffers the whole response: not progressive HTTP compression. Heap/RSS are sampled, not allocator/GPU peaks or plateau.',
  };
  try {
    for (const kind of ['small', 'repeated', 'varied'])
      for (let round = 0; round < 4; round++) {
        const order =
          round % 2
            ? ['codec', 'plain', 'codec-gzip', 'gzip', 'plain', 'plain']
            : ['plain', 'codec', 'gzip', 'codec-gzip', 'plain', 'plain'];
        const trials = [];
        for (const mode of order) {
          const child = spawnSync(
            process.execPath,
            ['--expose-gc', new URL(import.meta.url).pathname, '--worker', kind, mode],
            { encoding: 'utf8', timeout: 30000, maxBuffer: 1024 * 1024 },
          );
          assert.equal(child.status, 0, child.stderr);
          trials.push(JSON.parse(child.stdout));
        }
        assert(trials.every(t => t.fixtureSha256 === trials[0].fixtureSha256));
        report.rounds.push({ kind, round, order, trials });
      }
    report.status = 'passed';
  } catch (error) {
    report.status = 'failed';
    report.error = String(error.stack ?? error);
    process.exitCode = 1;
  }
  report.completedAt = new Date().toISOString();
  report.sourceSha256 = hash(await readFile(new URL(import.meta.url)));
  report.runtimeSha256 = hash(
    await readFile(new URL('../dist/package/stream-codec.js', import.meta.url)),
  );
  if (process.argv[2]) await writeFile(process.argv[2], JSON.stringify(report, null, 2) + '\n');
  console.log(
    JSON.stringify({ status: report.status, rounds: report.rounds.length, error: report.error }),
  );
}
