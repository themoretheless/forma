/**
 * # Svet Engine — slim entry (`svet-engine/slim`)
 *
 * The same API as `svet-engine`, minus what a bundler cannot drop on its own:
 *
 * - the base64 copy of the WASM core: load `svet-engine/core.wasm` with `loadSvetWasm()` before first use;
 * - the 3D Forma tags (`Scene3D`, `OrbitCube3D`, `Viewport3D`): opt in with `import 'svet-engine/forma-3d'`.
 *
 * Point the JSX runtime (`jsxImportSource`) and the Forma compiler (`runtimeImportSource`) at
 * `svet-engine/slim` too, otherwise they pull the self-contained entry back in.
 */
export * from './api.ts';
