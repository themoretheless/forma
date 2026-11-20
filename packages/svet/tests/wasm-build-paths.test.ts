import { execFileSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, it } from 'vitest';
import { decodeWasmBinary } from '../src/engine/wasm-binary.ts';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(packageRoot, '../..');

function stageMinimalSvetCheckout(destRoot: string): void {
  cpSync(join(repoRoot, 'rust-toolchain.toml'), join(destRoot, 'rust-toolchain.toml'));
  for (const crateName of ['svet-graph', 'svet-reconcile', 'svet-gpu-arena']) {
    cpSync(join(repoRoot, 'crates', crateName), join(destRoot, 'crates', crateName), {
      recursive: true,
    });
  }
  const svetCrateSrc = join(packageRoot, 'crates/svet-wasm');
  const svetCrateDst = join(destRoot, 'packages/svet/crates/svet-wasm');
  cpSync(join(svetCrateSrc, 'Cargo.toml'), join(svetCrateDst, 'Cargo.toml'));
  cpSync(join(svetCrateSrc, 'Cargo.lock'), join(svetCrateDst, 'Cargo.lock'));
  cpSync(join(svetCrateSrc, 'src'), join(svetCrateDst, 'src'), { recursive: true });
  cpSync(
    join(packageRoot, 'scripts/build-wasm.mjs'),
    join(destRoot, 'packages/svet/scripts/build-wasm.mjs'),
  );
}

it('embeds no source path that depends on where the WASM was built', () => {
  const text = new TextDecoder('latin1').decode(decodeWasmBinary());
  const sources = text.match(/[\w./\\:-]+\.rs/g) ?? [];
  expect(sources.length).toBeGreaterThan(0);
  // The toolchain and the registry are already remapped to fixed prefixes; repository sources
  // must be relative. Anything else names a directory of the machine that built the binary.
  const machinePaths = sources.filter(
    path =>
      !path.startsWith('/rustc/') &&
      !path.startsWith('/rust/deps/') &&
      (path.startsWith('/') || /^[A-Za-z]:/.test(path)),
  );
  expect([...new Set(machinePaths)]).toEqual([]);
});

it('produces byte-identical svet_wasm.wasm across distinct checkout and CARGO_TARGET_DIR paths', () => {
  const sandbox = mkdtempSync(join(tmpdir(), 'svet-wasm-repro-'));
  try {
    const checkoutA = join(sandbox, 'checkout-alpha');
    const checkoutB = join(sandbox, 'longer-checkout-beta-directory');
    const targetA = join(sandbox, 'target-dir-one');
    const targetB = join(sandbox, 'target-dir-two-external');
    stageMinimalSvetCheckout(checkoutA);
    stageMinimalSvetCheckout(checkoutB);

    execFileSync(
      process.execPath,
      [join(checkoutA, 'packages/svet/scripts/build-wasm.mjs'), '--no-embed'],
      {
        cwd: checkoutA,
        env: { ...process.env, CARGO_TARGET_DIR: targetA },
        stdio: 'pipe',
      },
    );
    execFileSync(
      process.execPath,
      [join(checkoutB, 'packages/svet/scripts/build-wasm.mjs'), '--no-embed'],
      {
        cwd: checkoutB,
        env: { ...process.env, CARGO_TARGET_DIR: targetB },
        stdio: 'pipe',
      },
    );

    const wasmA = readFileSync(join(targetA, 'wasm32-unknown-unknown/release/svet_wasm.wasm'));
    const wasmB = readFileSync(join(targetB, 'wasm32-unknown-unknown/release/svet_wasm.wasm'));
    expect(wasmA.equals(wasmB)).toBe(true);
    expect(wasmA.equals(Buffer.from(decodeWasmBinary()))).toBe(true);
  } finally {
    rmSync(sandbox, { recursive: true, force: true });
  }
}, 60_000);
