import { expect, it } from 'vitest';
import { decodeWasmBinary } from '../src/engine/wasm-binary.ts';

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
