/**
 * # Svet Engine — self-contained entry (`svet-engine`)
 *
 * Works with no setup: the Rust core is embedded as base64 and the 3D Forma tags are registered.
 * Applications that care about bundle size use `svet-engine/slim` instead.
 */
import './wasm-embedded.ts';
import './forma-3d.ts';

export * from './api.ts';
