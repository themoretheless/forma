import assert from 'node:assert/strict';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
assert(process.argv[2] && process.argv[3], 'Expected output and installed-copy manifest');
const manifest = JSON.parse(await readFile(resolve(process.argv[3]), 'utf8')),
  build = JSON.parse(await readFile(resolve(manifest.output, 'metadata.json'), 'utf8'));
for (const [path, sha] of Object.entries(build.artifactSha256))
  assert.equal(
    createHash('sha256')
      .update(await readFile(resolve(manifest.output, path)))
      .digest('hex'),
    sha,
  );
const a = await import(pathToFileURL(resolve(manifest.output, 'first/index.js')).href),
  b = await import(pathToFileURL(resolve(manifest.output, 'second/index.js')).href),
  runtime = await import(pathToFileURL(resolve(manifest.output, 'first/server.js')).href);
const report = {
  startedAt: new Date().toISOString(),
  node: process.version,
  cases: [],
  build,
  scope:
    'Node ALS in copy A; explicit same-core primitives in copy B after await. Compatible installed copies, not implicit cross-copy core propagation or another host.',
};
async function check(name, body) {
  try {
    await body();
    report.cases.push({ name, status: 'passed' });
  } catch (error) {
    report.cases.push({ name, status: 'failed', error: String(error.stack ?? error) });
  }
}
await check('foreign owner and context after await match the active child', () =>
  runtime.runWithIsolatedContextAsync(async () => {
    const core = a.getWasmCore(),
      root = a.getCurrentOwner(),
      context = a.createContext('default');
    await runtime.runWithOwnerScopeAsync([[context.id, 'child']], async () => {
      await Promise.resolve();
      const child = a.getCurrentOwner();
      assert.notEqual(child, root);
      assert.equal(
        b.withWasmCore(core, () => b.getCurrentOwner()),
        child,
      );
      assert.equal(
        b.withWasmCore(core, () => b.useContext(context)),
        'child',
      );
    });
  }),
);
await check('failed child tears down foreign computed before parent ends', () =>
  runtime.runWithIsolatedContextAsync(async () => {
    const core = a.getWasmCore(),
      reason = Error('controlled child rejection');
    let cleaned = 0;
    await assert.rejects(
      runtime.runWithOwnerScopeAsync([], async () => {
        await Promise.resolve();
        b.withWasmCore(core, () => {
          b.computed(() => 1);
          b.onCleanup(() => cleaned++);
        });
        throw reason;
      }),
      error => error === reason,
    );
    assert.equal(cleaned, 1);
    assert.equal(a.getGraphTelemetry().nodes, 0);
  }),
);
await check('late foreign reentry refuses a closed child without poisoning its parent', () =>
  runtime.runWithIsolatedContextAsync(async () => {
    const core = a.getWasmCore(),
      root = a.getCurrentOwner();
    let release, late;
    const gate = new Promise(resolve => (release = resolve));
    await runtime.runWithOwnerScopeAsync([], async () => {
      late = (async () => {
        await gate;
        return b.withWasmCore(core, () => b.getCurrentOwner());
      })();
      late.catch(() => {});
      await Promise.resolve();
    });
    release();
    await assert.rejects(late, /completed|disposed/);
    assert.equal(
      b.withWasmCore(core, () => b.getCurrentOwner()),
      root,
    );
  }),
);
await check('captured root and unrelated core retain their explicit ownership', () =>
  runtime.runWithIsolatedContextAsync(async () => {
    const core = a.getWasmCore(),
      root = a.getCurrentOwner(),
      capture = a.captureReactiveContext(),
      foreign = b.createWasmCoreInstance();
    try {
      await runtime.runWithOwnerScopeAsync([], async () => {
        await Promise.resolve();
        assert.equal(
          capture.run(() => b.withWasmCore(core, () => b.getCurrentOwner())),
          root,
        );
        assert.equal(
          b.withWasmCore(foreign, () => b.getCurrentOwner()),
          null,
        );
      });
    } finally {
      capture.dispose();
    }
  }),
);
await check('nested explicit cores restore both copies after a throw', () => {
  const originalA = a.getWasmCore(),
    originalB = b.getWasmCore(),
    outer = a.createWasmCoreInstance(),
    inner = b.createWasmCoreInstance(),
    reason = Error('nested scope');
  assert.notEqual(originalA, originalB);
  a.withWasmCore(outer, () => {
    assert.equal(b.getWasmCore(), outer);
    assert.throws(
      () =>
        b.withWasmCore(inner, () => {
          assert.equal(a.getWasmCore(), inner);
          throw reason;
        }),
      error => error === reason,
    );
    assert.equal(a.getWasmCore(), outer);
    assert.equal(b.getWasmCore(), outer);
  });
  assert.equal(a.getWasmCore(), originalA);
  assert.equal(b.getWasmCore(), originalB);
});
await check(
  'foreign captured child can update and allocate in its original core after carrier close',
  () =>
    runtime.runWithIsolatedContextAsync(async () => {
      const core = a.getWasmCore(),
        source = a.signal(0);
      a.onCleanup(source.dispose);
      let capture, release, late;
      const gate = new Promise(resolve => (release = resolve));
      try {
        await runtime.runWithOwnerScopeAsync([], async () => {
          await Promise.resolve();
          capture = b.withWasmCore(core, () => b.captureReactiveContext());
          late = (async () => {
            await gate;
            assert.throws(() => a.getWasmCore(), /completed|disposed/);
            capture.run(() => {
              source.set(1);
              assert.equal(a.getWasmCore(), core);
              const derived = a.computed(() => source() * 2);
              assert.equal(derived(), 2);
              derived.dispose();
            });
            assert.throws(() => a.getWasmCore(), /completed|disposed/);
          })();
          late.catch(() => {});
        });
        release();
        await late;
        assert.equal(source(), 1);
        source.dispose();
        assert.equal(a.getGraphTelemetry().nodes, 0);
      } finally {
        capture?.dispose();
      }
    }),
);
await check('concurrent child owners remain request-local across copies', () =>
  Promise.all(
    [1, 2].map(id =>
      runtime.runWithIsolatedContextAsync(async () => {
        const core = a.getWasmCore();
        await runtime.runWithOwnerScopeAsync([], async () => {
          await new Promise(setImmediate);
          const child = a.getCurrentOwner();
          assert.equal(
            b.withWasmCore(core, () => b.getCurrentOwner()),
            child,
          );
          const source = b.withWasmCore(core, () => b.signal(id));
          assert.equal(source(), id);
          source.dispose();
        });
        assert.equal(a.getGraphTelemetry().nodes, 0);
      }),
    ),
  ),
);
if (typeof global.gc === 'function')
  await check('weak owner resolvers release completed request cores', async () => {
    const refs = [],
      handles = [],
      errors = [];
    let controls = Array.from({ length: 4 }, () => ({ live: true }));
    const controlRefs = controls.map(value => new WeakRef(value));
    for (let i = 0; i < 8; i++) {
      const reason = Error('retained child reason ' + i);
      errors.push(reason);
      const job = runtime.runWithIsolatedContextAsync(async () => {
        const core = a.getWasmCore();
        refs.push(new WeakRef(core), new WeakRef(core.memory));
        await assert.rejects(
          runtime.runWithOwnerScopeAsync([], async () => {
            await Promise.resolve();
            b.withWasmCore(core, () => b.computed(() => i));
            throw reason;
          }),
          error => error === reason,
        );
      });
      handles.push(job);
      await job;
    }
    async function collect(references) {
      let alive;
      for (let round = 0; round < 10; round++) {
        await new Promise(setImmediate);
        global.gc();
        await new Promise(setImmediate);
        alive = references.filter(ref => ref.deref()).length;
        if (!alive) break;
      }
      return alive;
    }
    assert.equal(await collect(refs), 0);
    assert.equal(controlRefs.filter(ref => ref.deref()).length, 4);
    controls = undefined;
    assert.equal(await collect(controlRefs), 0);
    report.gc = {
      coresCollected: 8,
      memoriesCollected: 8,
      retainedPromises: handles.length,
      retainedErrors: errors.length,
      controlsHeld: 4,
      controlsReleased: 0,
    };
  });
assert.equal(
  globalThis[Symbol.for('svet.wasm-core.overrides.v1')]?.length ?? 0,
  0,
  'Explicit core scope survived return',
);
report.status = report.cases.every(row => row.status === 'passed') ? 'passed' : 'failed';
report.completedAt = new Date().toISOString();
report.probeSha256 = createHash('sha256')
  .update(await readFile(new URL(import.meta.url)))
  .digest('hex');
await writeFile(resolve(process.argv[2]), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ status: report.status, cases: report.cases }, null, 2));
if (report.status !== 'passed') process.exitCode = 1;
