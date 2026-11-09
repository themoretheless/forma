import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { JSDOM } from 'jsdom';
const [manifestPath, output] = process.argv.slice(2);
assert(manifestPath && output);
const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const metadata = JSON.parse(await readFile(resolve(manifest.output, 'metadata.json'), 'utf8'));
for (const [file, sha] of Object.entries(metadata.artifactSha256))
  assert.equal(
    createHash('sha256')
      .update(await readFile(resolve(manifest.output, file)))
      .digest('hex'),
    sha,
  );
const a = await import(pathToFileURL(resolve(manifest.output, 'first/index.js')).href),
  b = await import(pathToFileURL(resolve(manifest.output, 'second/index.js')).href);
assert.notEqual(a.compileFormaComponent, b.compileFormaComponent);
assert.notEqual(a.getWasmCore(), b.getWasmCore());
const dom = new JSDOM('<!doctype html><body></body>');
for (const key of ['window', 'document', 'Node', 'Element']) globalThis[key] = dom.window[key];
const report = { node: process.version, cases: [], metadata };
for (const [name, parent, child] of [
  ['A factory creates B', a, b],
  ['B factory creates A', b, a],
]) {
  try {
    let fail = true,
      handle;
    const source = 'component App { Column { Child {} } }',
      childSource = 'component Child { state value: Number = 2; Text { text:state.value; } }';
    const view = parent.compileFormaComponent(source, {
      components: {
        Child: () => {
          child.compileFormaComponent(childSource);
          const result = child.compileFormaComponent(childSource);
          if (fail) throw Error('factory refused');
          return result;
        },
      },
    });
    const container = document.createElement('div'),
      baseline = [a.getGraphTelemetry().nodes, b.getGraphTelemetry().nodes];
    try {
      assert.throws(() => view.mount(container), /factory refused/);
      assert.deepEqual([a.getGraphTelemetry().nodes, b.getGraphTelemetry().nodes], baseline);
      fail = false;
      handle = view.mount(container);
      assert.equal(container.textContent, '2');
    } finally {
      handle?.unmount();
      view.dispose();
    }
    assert.deepEqual([a.getGraphTelemetry().nodes, b.getGraphTelemetry().nodes], [0, 0]);
    report.cases.push({ name, status: 'passed' });
  } catch (error) {
    report.cases.push({ name, status: 'failed', error: String(error.stack ?? error) });
  }
}
report.status = report.cases.every(row => row.status === 'passed') ? 'passed' : 'failed';
await writeFile(output, JSON.stringify(report, null, 2) + '\n');
if (report.status !== 'passed') process.exitCode = 1;
