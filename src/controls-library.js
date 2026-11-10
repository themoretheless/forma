import { stripPathPrefix } from './shared/text.js';
// The bundled control library every built-in project starts from, keyed by its path inside
// crates/forma/controls. Vite resolves the glob at build time, so it lives in one module.
const files = import.meta.glob('../crates/forma/controls/{components,assets}/*', {
  query: '?raw',
  import: 'default',
  eager: true,
});
export const controlsLibrary = stripPathPrefix(files, '../crates/forma/controls/');
