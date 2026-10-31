import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const wasmPath = path.resolve(
  __dirname,
  '../crates/svet-wasm/target/wasm32-unknown-unknown/release/svet_wasm.wasm'
);
const outPath = path.resolve(__dirname, '../src/engine/wasm-binary.ts');

const bytes = fs.readFileSync(wasmPath);
const base64 = bytes.toString('base64');

const content = `// AUTO-GENERATED FILE — DO NOT EDIT MANUALLY
// Built from crates/svet-wasm/src/lib.rs (${bytes.length} bytes)

export const WASM_BASE64: string = "${base64}";

export function decodeWasmBinary(): Uint8Array {
  if (typeof Buffer !== 'undefined') {
    return new Uint8Array(Buffer.from(WASM_BASE64, 'base64'));
  }
  const binString = atob(WASM_BASE64);
  const bytes = new Uint8Array(binString.length);
  for (let i = 0; i < binString.length; i++) {
    bytes[i] = binString.charCodeAt(i);
  }
  return bytes;
}
`;

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, content, 'utf-8');
console.log(`[svet] Embedded WASM binary (${bytes.length} bytes) -> src/engine/wasm-binary.ts`);
