// @ts-check
import { createPropertyInspector } from './property-inspector.js';
import { createBulkEditor } from './bulk-edit.js';
import { createLayoutInspector } from './layout-inspector.js';
import { nativeInspectionModel } from './native-inspection.js';
import { findEntry } from '../compiler/design-states.js';
import { designReferencesInFiles } from '../compiler/design-data.js';
import { parse } from '../compiler/language.js';
import { sceneControlNodes } from '../compiler/components.js';
import { studioEventTypes } from '../shared/studio-events.js';

/**
 * Coordinates property inspector, bulk editor, layout inspector, and native inspection panel.
 * @param {{ $: (id: string) => any, root?: Document | Element, appState: any, studioEvents: any, liveSource: (path: string) => string, commitSource: (spec: any) => void, reportError: (err: any) => void, getBatchSelection: { get: () => number[], set: (starts: number[]) => void }, getVectorPreview: () => any, openCodeRange: (source: any) => void, commitLayoutEdit: (source: any, insert: string) => void, selectTemplateControl: (item: any) => void, documentObj?: Document }} options
 */
export function createInspectorService({
  $,
  root = document,
  appState,
  studioEvents,
  liveSource,
  commitSource,
  reportError,
  getBatchSelection,
  getVectorPreview,
  openCodeRange,
  commitLayoutEdit,
  selectTemplateControl,
  documentObj = document,
}) {
  const propertyInspector = createPropertyInspector({
    container: $('inspector'),
    designTokens: () => designReferencesInFiles(appState.files),
    liveSource,
    commit: commitSource,
    onError: reportError,
  });

  const bulkEditor = createBulkEditor({
    container: $('inspector'),
    commit: ({ file, source, primary, ...change }) =>
      commitSource({
        file,
        source,
        ...change,
        reselect: primary,
        after: () => {
          getBatchSelection.set(change.starts);
          renderInspector();
        },
      }),
    onError: reportError,
  });

  const layoutInspector = createLayoutInspector({
    viewport: $('canvas'),
    artboard: $('preview'),
    toolbar: root.querySelector('.canvas-tools'),
    getVisuals: () => (appState.designPresetName === 'original' ? appState.lastVisuals : []),
    getRuntime: () => getVectorPreview()?.layoutSnapshot(),
    getMode: () => appState.mode,
    getControl: () =>
      appState.lastPreviewControls.findIndex(n => n.start === appState.selected?.start),
    readSource: path => appState.files[path],
    readTracks: source => {
      const raw = appState.files[source.file].slice(source.from, source.to);
      const value = parse(`component Tracks { Frame { columns: ${raw}; } }`).nodes[0].props.columns;
      return Array.isArray(value) ? value : [value];
    },
    openSource: openCodeRange,
    edit: commitLayoutEdit,
  });

  function designStateFor(node) {
    const state = appState.scenario < 0 ? null : appState.compiled?.states?.[appState.scenario];
    const key =
      appState.selectedPath === appState.entry && appState.designPresetName === 'original'
        ? node?.props?.key
        : null;
    if (!state || typeof key !== 'string') return null;
    const source = appState.files[state.file];
    if (typeof source !== 'string') return null;
    try {
      const overridden = findEntry(source, state.name, key);
      const base = findEntry(source, null, key);
      return overridden || base
        ? { name: state.name, path: state.file, source, entry: overridden, base }
        : null;
    } catch {
      return null;
    }
  }

  function renderInspector() {
    studioEvents.emit(studioEventTypes.selection, {
      selected: !!(
        appState.selected &&
        appState.selectedPath &&
        Object.hasOwn(appState.files, appState.selectedPath)
      ),
    });
    bulkEditor?.render({
      path: '',
      source: '',
      primary: 0,
      starts: [],
      editable: false,
    });
    if (
      !appState.selected ||
      !appState.selectedPath ||
      !Object.hasOwn(appState.files, appState.selectedPath)
    ) {
      propertyInspector?.render({ node: null, editable: false });
      return;
    }
    propertyInspector?.render({
      node: appState.selected,
      path: appState.selectedPath,
      source: appState.files[appState.selectedPath],
      editable: appState.designPresetName === 'original',
      note: appState.inspectorNote,
      state: designStateFor(appState.selected),
    });
    bulkEditor?.render({
      path: appState.selectedPath,
      source: appState.files[appState.selectedPath],
      starts: appState.selectedPath === appState.entry ? (getBatchSelection.get() ?? []) : [],
      primary: appState.selected.start,
      editable: appState.designPresetName === 'original',
    });
  }

  function showNativeInspection(data) {
    let panel = /** @type {HTMLDetailsElement | null} */ ($('native-inspection'));
    if (!panel) {
      panel = documentObj.createElement('details');
      panel.id = 'native-inspection';
      panel.className = 'layout-inspector';
      root.querySelector('.inspector').append(panel);
    }
    for (const other of /** @type {NodeListOf<HTMLDetailsElement>} */ (
      root.querySelectorAll('.inspector>.layout-inspector')
    )) {
      if (other !== panel) other.open = false;
    }
    panel.open = true;
    panel.replaceChildren();
    const title = documentObj.createElement('summary');
    title.textContent = 'Нативное окно · живое дерево · F9: выбор';
    panel.append(title);
    const current = Boolean(
      appState.nativeSource &&
      appState.files[appState.nativeSource.entry] === appState.nativeSource.source,
    );
    const model = nativeInspectionModel(
      data,
      sceneControlNodes(appState.nativeSource?.nodes?.[0]?.children ?? []),
      current,
    );
    const status = documentObj.createElement('p');
    status.textContent = model.status;
    panel.append(status);
    for (const item of model.rows) {
      const row = documentObj.createElement('button');
      row.style.display = 'block';
      row.style.marginLeft = `${item.indent}px`;
      row.textContent = item.label;
      row.setAttribute('aria-pressed', String(item.pressed));
      row.disabled = item.disabled;
      row.onclick = () =>
        selectTemplateControl({ path: appState.nativeSource.entry, node: item.source });
      panel.append(row);
    }
    if (model.selected)
      selectTemplateControl({ path: appState.nativeSource.entry, node: model.selected });
  }

  function updateLayout() {
    layoutInspector?.update();
  }

  function destroy() {
    layoutInspector?.destroy();
  }

  return {
    propertyInspector,
    bulkEditor,
    layoutInspector,
    renderInspector,
    showNativeInspection,
    updateLayout,
    destroy,
  };
}
