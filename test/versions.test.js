import { test } from 'node:test';
import assert from 'node:assert/strict';
import { versions, validVersion, release } from '../scripts/versions.mjs';

test('one release tag must match every manifest version', () => {
  const current = { runtime: '0.2.0', studio: '0.5.1', svet: '0.2.0' };
  // One tag covers the library, Studio and svet, so it has to match every manifest.
  assert.throws(() => release('v0.2.0', current), /does not match studio/);
  assert.throws(() => release('v0.5.1', current), /does not match runtime/);
  assert.throws(
    () => release('v0.2.0', { runtime: '0.2.0', studio: '0.2.0', svet: '1.0.0' }),
    /does not match svet/,
  );
  assert.equal(
    release('v0.2.0', { runtime: '0.2.0', studio: '0.2.0', svet: '0.2.0' }).version,
    '0.2.0',
  );
  // The per-component tags of earlier releases are no longer release tags.
  assert.throws(() => release('forma-v0.2.0', current), /Expected vX\.Y\.Z/);
  assert.throws(() => release('studio-v0.5.1', current), /Expected vX\.Y\.Z/);
  for (const tag of ['v0.2.0', 'runtime-v0.2.0', 'forma-0.2.0', 'v0.2.0/extra', 'v0.2.0+build'])
    assert.throws(() => release(tag, current));
});
test('prereleases are explicit and malformed semver cannot trigger a release', () => {
  assert.equal(
    release('v0.2.0-rc.1', { runtime: '0.2.0-rc.1', studio: '0.2.0-rc.1', svet: '0.2.0-rc.1' })
      .prerelease,
    true,
  );
  assert.equal(
    release('v0.2.0', { runtime: '0.2.0', studio: '0.2.0', svet: '0.2.0' }).prerelease,
    false,
  );
  for (const v of [
    '01.2.3',
    '1.02.3',
    '1.2',
    '1.2.3-',
    '1.2.3-rc.01',
    '1.2.3-01',
    '1.2.3\n',
    '1.2.3+build',
  ])
    assert.equal(validVersion(v), false, v);
  assert.equal(validVersion('0.1.0-alpha.0'), true);
});
test('committed manifest and lockfile versions agree', () => {
  const v = versions();
  assert.ok(validVersion(v.runtime));
  assert.ok(validVersion(v.studio));
  assert.match(v.wasmBindgen, /^\d+\.\d+\.\d+$/);
});

test('gpu-runtime, gpu-shader and gpu-compute resolve cleanly in the workspace Cargo.lock without duplicate windows crates', async () => {
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const { root } = await import('../scripts/versions.mjs');
  const blocks = new Map(
    readFileSync(resolve(root, 'Cargo.lock'), 'utf8')
      .split('[[package]]')
      .flatMap(block => {
        const name = /^name = "([^"]+)"/m.exec(block)?.[1],
          version = /^version = "([^"]+)"/m.exec(block)?.[1];
        return name && version ? [[`${name}@${version}`, block]] : [];
      }),
  );
  const single = name => {
    const found = [...blocks.keys()]
      .filter(key => key.startsWith(`${name}@`))
      .map(key => key.slice(name.length + 1));
    assert.equal(
      found.length,
      1,
      `the lock must hold one ${name}, found ${found.join(', ') || 'none'}`,
    );
    return found[0];
  };
  for (const pkg of ['gpu-runtime', 'gpu-shader', 'gpu-compute'])
    assert.equal(single(pkg), '0.1.0');
});

test('version command updates only its component and all associated lockfiles', async () => {
  const { mkdtempSync, mkdirSync, copyFileSync, readFileSync, rmSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join, dirname } = await import('node:path');
  const { spawnSync } = await import('node:child_process');
  const { root } = await import('../scripts/versions.mjs');
  const folder = mkdtempSync(join(tmpdir(), 'forma-versions-'));
  try {
    for (const path of [
      'scripts/versions.mjs',
      'package.json',
      'package-lock.json',
      'crates/forma/Cargo.toml',
      'Cargo.lock',
      'native-app/Cargo.toml',
      'native-app/Cargo.lock',
      'packages/svet/package.json',
      'packages/svet/package-lock.json',
      'packages/svet/crates/svet-wasm/Cargo.toml',
      'packages/svet/crates/svet-wasm/Cargo.lock',
      'crates/svet-graph/Cargo.toml',
      'crates/svet-reconcile/Cargo.toml',
      'crates/svet-gpu-arena/Cargo.toml',
    ]) {
      const dest = join(folder, path);
      mkdirSync(dirname(dest), { recursive: true });
      copyFileSync(join(root, path), dest);
    }
    // The CLI uses ESM; its copied root package.json already sets type=module.
    const run = (...args) =>
      spawnSync(process.execPath, [join(folder, 'scripts/versions.mjs'), ...args], {
        encoding: 'utf8',
      });
    const before = JSON.parse(run('check').stdout);
    const runtime = run('set', 'runtime', '0.8.0-rc.1');
    assert.equal(runtime.status, 0, runtime.stderr);
    assert.deepEqual(JSON.parse(runtime.stdout), { ...before, runtime: '0.8.0-rc.1' });
    const studio = run('set', 'studio', '0.9.1');
    assert.equal(studio.status, 0, studio.stderr);
    assert.deepEqual(JSON.parse(studio.stdout), {
      ...before,
      runtime: '0.8.0-rc.1',
      studio: '0.9.1',
    });
    const svet = run('set', 'svet', '0.9.2');
    assert.equal(svet.status, 0, svet.stderr);
    assert.deepEqual(JSON.parse(svet.stdout), {
      ...before,
      runtime: '0.8.0-rc.1',
      studio: '0.9.1',
      svet: '0.9.2',
    });
    // `all` moves every component to one release version.
    const all = run('set', 'all', '1.4.0');
    assert.equal(all.status, 0, all.stderr);
    assert.deepEqual(JSON.parse(all.stdout), {
      ...before,
      runtime: '1.4.0',
      studio: '1.4.0',
      svet: '1.4.0',
    });
    for (const [path, pattern] of [
      ['packages/svet/package.json', /"version": "1\.4\.0"/],
      ['packages/svet/crates/svet-wasm/Cargo.toml', /^version = "1\.4\.0"$/m],
      ['packages/svet/crates/svet-wasm/Cargo.lock', /name = "svet-wasm"\nversion = "1\.4\.0"/],
      ['crates/svet-graph/Cargo.toml', /^version = "1\.4\.0"$/m],
      ['crates/svet-reconcile/Cargo.toml', /^version = "1\.4\.0"$/m],
      ['crates/svet-gpu-arena/Cargo.toml', /^version = "1\.4\.0"$/m],
      ['Cargo.lock', /name = "svet-graph"\nversion = "1\.4\.0"/],
    ])
      assert.match(readFileSync(join(folder, path), 'utf8'), pattern, path);
    const snapshot = readFileSync(join(folder, 'package.json'), 'utf8');
    assert.notEqual(run('set', 'studio', '0.9.01').status, 0);
    assert.equal(
      readFileSync(join(folder, 'package.json'), 'utf8'),
      snapshot,
      'invalid input must not write files',
    );
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});
