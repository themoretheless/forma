// @ts-check
import { compileComponents } from '../compiler/components.js';
import { controlsLibrary } from '../workspace/controls-library.js';
import { stripPathPrefix } from '../shared/text.js';
import { createVectorPreview } from '../preview/vector-preview.js';
import { materializeTheme } from '../../crates/forma/controls/tokens.js';
export { keyText } from './studio-window.js';

// Studio's own chrome is a Forma project: `studio-ui/ui/*.ui` composed from the shared control
// library, compiled by the same compiler and drawn by the same Rust/WASM runtime as a user's UI.
const studioUiFiles = import.meta.glob('../../studio-ui/{components,ui}/*.ui', {
  query: '?raw',
  import: 'default',
  eager: true,
});
export const shellFiles = materializeTheme(
  {
    ...controlsLibrary,
    ...stripPathPrefix(studioUiFiles, '../../studio-ui/'),
  },
  'dark',
);

// One floating piece of chrome. The host owns its data and passes it in as `state`; the markup
// reports clicks as `actions.<name>(args)` and edits of `<->` bindings as state paths.
/** @param {{ runtime: any, entry: string, className: string, onAction?: (action: string, args: any[]) => void, onBinding?: (path: string, value: any, node: any) => void, onKey?: any }} options */
export function createShellRegion({
  runtime,
  entry,
  className,
  onAction = (_action, _args) => {},
  onBinding = (_path, _value, _node) => {},
  onKey,
}) {
  const element = document.createElement('div');
  element.className = 'forma-region ' + className;
  let signature = '';
  const preview = createVectorPreview({
    runtime,
    onError: message => console.error(`${entry}: ${message}`),
    onAction: (action, node) =>
      onAction(action.replace(/^actions\./, ''), node?.eventArgs?.clicked ?? []),
    onBindingChange: changes => {
      for (const { path, value, node } of changes)
        onBinding(path.replace(/^state\./, ''), value, node);
    },
    onControlKey: onKey,
  });
  return {
    element,
    measure: (text, size) => Math.ceil(preview.measureText(text, size)[0]),
    update(state) {
      const next = JSON.stringify(state);
      if (next === signature) return;
      signature = next;
      const linked = compileComponents(shellFiles, entry, state, {
        measureText: preview.measureText,
      });
      preview.render({ container: element, ...linked, designMode: false });
      const scene = preview.layoutSnapshot();
      if (scene) element.style.width = scene.renderWidth + 'px';
    },
    focus: key => preview.focusKey(key),
    destroy() {
      preview.destroy();
      element.remove();
    },
  };
}
