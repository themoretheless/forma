/**
 * Side-effect module: makes the base64 copy of the Rust core available as the default WASM source,
 * so the engine works without any setup. The self-contained entries import it; `svet-engine/slim`
 * does not, which keeps the base64 payload out of the consumer's bundle.
 */
import { decodeWasmBinary } from './wasm-binary.ts';
import { installEmbeddedSvetWasmSource } from './wasm-bridge.ts';

installEmbeddedSvetWasmSource(decodeWasmBinary);
