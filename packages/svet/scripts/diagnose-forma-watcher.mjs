import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, dirname } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createServer } from 'vite';
import { writeProbeSource, writeModes } from './forma-probe-writes.mjs';
const [installed, output] = process.argv.slice(2);
assert(installed && output);
const { svetVitePlugin } = await import(
  pathToFileURL(resolve(installed, 'dist/package/compiler.js'))
);
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const source = v => `component App { Text { text:'v${v}'; } }`;
const report = { node: process.version, cases: [] };
for (const mode of writeModes)
  for (const stable of [false, true])
    for (let repetition = 0; repetition < 3; repetition++) {
      const root = await realpath(await mkdtemp(resolve(tmpdir(), 'svet-watch-diagnostic-'))),
        file = resolve(root, 'View.ui'),
        events = [];
      await writeFile(file, source(1));
      const server = await createServer({
        root,
        configFile: false,
        logLevel: 'silent',
        plugins: [
          svetVitePlugin({
            runtimeImportSource: '/@fs' + resolve(installed, 'dist/package/index.js'),
          }),
        ],
        server: {
          host: '127.0.0.1',
          port: 0,
          fs: { allow: [root, dirname(installed)] },
          watch: {
            awaitWriteFinish: stable ? { stabilityThreshold: 60, pollInterval: 10 } : false,
          },
        },
      });
      try {
        await server.listen();
        await server.transformRequest('/View.ui');
        await sleep(100);
        server.watcher.on('change', path => {
          if (path === file) events.push({ at: performance.now() });
        });
        await writeProbeSource(file, source(2), mode);
        const deadline = performance.now() + 2000;
        while (!events.length && performance.now() < deadline) await sleep(1);
        assert(events.length, 'First change must be observed');
        let first;
        const transformDeadline = performance.now() + 2000;
        do {
          first = await server.transformRequest('/View.ui');
          if (first.code.includes('v2')) break;
          await sleep(1);
        } while (performance.now() < transformDeadline);
        assert(first.code.includes('v2'));
        const finalWrite = performance.now();
        await writeProbeSource(file, source(3), mode);
        await sleep(250);
        const final = await server.transformRequest('/View.ui');
        report.cases.push({
          mode,
          stable,
          repetition,
          events,
          finalWrite,
          diskIsLatest: (await readFile(file, 'utf8')) === source(3),
          transformedLatest: final.code.includes('v3'),
          validation: JSON.parse(final.code.match(/export const uiValidation = (.+);/)[1]),
        });
      } catch (error) {
        report.cases.push({ mode, stable, repetition, error: String(error.stack ?? error) });
      } finally {
        await server.close();
      }
    }
await writeFile(output, JSON.stringify(report, null, 2) + '\n');
console.log(
  JSON.stringify(
    report.cases.map(({ mode, stable, transformedLatest, events, error }) => ({
      mode,
      stable,
      transformedLatest,
      events: events?.length,
      error,
    })),
  ),
);
