import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { checkTokenizer, linkTokenizerPackage, lockedTokenizer } from '../scripts/tokenizer.mjs';

const entry = source =>
  `[[package]]\nname = "serde_json"\nversion = "1.0.0"\n\n[[package]]\nname = "themoretheless-tokenizer-forma"\nversion = "0.8.0"\n${source}dependencies = [\n "serde_json",\n]\n`;
const REV = '6bc24a607a5e11c38da4ad4ba466a9a67173504d';
test('the tokenizer revision comes from the exact rev Cargo.lock pins', () => {
  assert.deepEqual(
    lockedTokenizer(
      entry(`source = "git+https://github.com/themoretheless/tokenizer?rev=${REV}#${REV}"\n`),
    ),
    { url: 'https://github.com/themoretheless/tokenizer', rev: REV, version: '0.8.0' },
  );
});
test('a path, registry, moving branch or mismatched tokenizer revision is not a reproducible pin', () => {
  // A path dependency is what left CI unable to build: actions/checkout fetches only forma.
  for (const source of [
    '',
    'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
    `source = "git+https://github.com/themoretheless/tokenizer?branch=release#${REV}"\n`,
    `source = "git+https://github.com/themoretheless/tokenizer?rev=${REV}#d1c9ff7e7e69c903e02715645069e642872f14df"\n`,
  ])
    assert.throws(() => lockedTokenizer(entry(source)), /exact tokenizer revision/);
});
test('the checked-in lock, Cargo manifest and vendored npm tarball pin the same exact tokenizer release', () => {
  const manifest = readFileSync(new URL('../crates/forma/Cargo.toml', import.meta.url), 'utf8');
  const rev = manifest.match(
    /themoretheless-tokenizer-forma = \{[^\n]*rev = "([0-9a-f]{40})"/,
  )?.[1];
  assert.ok(rev, 'Cargo.toml must declare an exact tokenizer revision');
  const locked = lockedTokenizer();
  assert.equal(locked.rev, rev);
  assert.deepEqual(checkTokenizer(locked), locked);
});

test('vendored tokenizer check rejects tampered tarball checksums, lock entries and versions', () => {
  const folder = mkdtempSync(join(tmpdir(), 'forma-tokenizer-vendor-'));
  try {
    mkdirSync(join(folder, 'vendor'), { recursive: true });
    const tarballRel = 'vendor/themoretheless-tokenizer-forma-0.8.0.tgz';
    writeFileSync(join(folder, tarballRel), 'tarball-bytes');
    const sha256 = createHash('sha256').update('tarball-bytes').digest('hex');
    const locked = {
      url: 'https://github.com/themoretheless/tokenizer',
      rev: REV,
      version: '0.8.0',
    };
    const manifest = {
      name: '@themoretheless/tokenizer-forma',
      version: '0.8.0',
      url: locked.url,
      rev: REV,
      tarball: tarballRel,
      sha256,
    };
    writeFileSync(join(folder, 'vendor/tokenizer-forma.json'), JSON.stringify(manifest));
    writeFileSync(
      join(folder, 'package.json'),
      JSON.stringify({ dependencies: { '@themoretheless/tokenizer-forma': `file:${tarballRel}` } }),
    );
    writeFileSync(
      join(folder, 'package-lock.json'),
      JSON.stringify({
        packages: {
          '': { dependencies: { '@themoretheless/tokenizer-forma': `file:${tarballRel}` } },
          'node_modules/@themoretheless/tokenizer-forma': {
            version: '0.8.0',
            resolved: `file:${tarballRel}`,
            integrity: 'sha512-abc',
          },
        },
      }),
    );
    assert.deepEqual(checkTokenizer(locked, folder), locked);

    // Tamper tarball checksum
    writeFileSync(
      join(folder, 'vendor/tokenizer-forma.json'),
      JSON.stringify({ ...manifest, sha256: '0'.repeat(64) }),
    );
    assert.throws(() => checkTokenizer(locked, folder), /sha256 mismatch/);

    // Restore checksum, tamper installed copy version
    writeFileSync(join(folder, 'vendor/tokenizer-forma.json'), JSON.stringify(manifest));
    mkdirSync(join(folder, 'node_modules/@themoretheless/tokenizer-forma'), { recursive: true });
    writeFileSync(
      join(folder, 'node_modules/@themoretheless/tokenizer-forma/package.json'),
      JSON.stringify({ version: '0.7.9' }),
    );
    assert.throws(
      () => checkTokenizer(locked, folder),
      /Installed @themoretheless\/tokenizer-forma/,
    );

    // Tamper package.json specifier back to sibling checkout
    rmSync(join(folder, 'node_modules'), { recursive: true, force: true });
    writeFileSync(
      join(folder, 'package.json'),
      JSON.stringify({
        dependencies: { '@themoretheless/tokenizer-forma': 'file:../tokenizer/packages/forma' },
      }),
    );
    assert.throws(() => checkTokenizer(locked, folder), /package\.json must depend/);
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});

test('the gate helper points an existing npm link at target and puts it back', () => {
  const folder = mkdtempSync(join(tmpdir(), 'forma-tokenizer-link-'));
  try {
    for (const name of ['sibling', 'pinned']) mkdirSync(join(folder, name));
    const link = join(folder, 'link');
    symlinkSync('sibling', link);
    const restore = linkTokenizerPackage(link, join(folder, 'pinned'));
    assert.equal(readlinkSync(link), 'pinned');
    restore();
    restore();
    assert.equal(readlinkSync(link), 'sibling');
    // Already on the target, or not a link at all: nothing changes.
    linkTokenizerPackage(link, join(folder, 'sibling'))();
    assert.equal(readlinkSync(link), 'sibling');
    const plain = join(folder, 'plain');
    copyFileSync(new URL('../package.json', import.meta.url), plain);
    linkTokenizerPackage(plain, join(folder, 'pinned'))();
    assert.throws(() => readlinkSync(plain));
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
});
