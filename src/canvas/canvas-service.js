// @ts-check
import { createCanvasTools } from './canvas-tools.js';
import { createElementTools } from './element-tools.js';
import { createSpacingOverlay } from './spacing-overlay.js';
import { createComponentCatalog, domLayoutSnapshot } from './dom-layout-snapshot.js';
import { restoreSelection, selectionIdentities } from './selection-restore.js';
import { insertableControls } from '../compiler/control-catalog.js';
import { sceneControlNodes } from '../compiler/components.js';
import { findNodeByStart } from '../compiler/element-tree.js';
import { parse } from '../compiler/language.js';

/**
 * Coordinates interactive canvas selection, guides, zoom/pan tools, and element batch edits.
 * @param {{ $: (id: string) => any, root?: Document | Element, appState: any, compileComponents: any, getVectorPreview: () => any, onToggleMode: () => void, onSelect: (node: any) => void, onClearSelection: () => void, onExtractControl: (item: any) => void, onRenderInspector: () => void, onUpdateLayoutInspector: () => void, onOpen: (path: string) => void, onCompile: () => void, getCodeEditor: () => any, setCaption: (text: string) => void, navigatorObj?: Navigator }} options
 */
export function createCanvasService({
  $,
  root = document,
  appState,
  compileComponents,
  getVectorPreview,
  onToggleMode,
  onSelect,
  onClearSelection,
  onExtractControl,
  onRenderInspector,
  onUpdateLayoutInspector,
  onOpen,
  onCompile,
  getCodeEditor,
  setCaption,
  navigatorObj = navigator,
}) {
  const projectInserts = createComponentCatalog(insertableControls);
  const spacingOverlay = createSpacingOverlay($('canvas'), () =>
    appState.mode === 'design' && appState.selectedPath === appState.entry
      ? appState.selected
      : null,
  );

  const canvasTools = createCanvasTools({
    viewport: $('canvas'),
    artboard: $('preview'),
    toolbar: root.querySelector('.preview-tools'),
    getScene: () => (appState.renderer === 'vector' ? getVectorPreview()?.layoutSnapshot() : null),
    getSelection: () => {
      if (!appState.selected || appState.selectedPath !== appState.entry) return null;
      const children = appState.compiled?.nodes?.[0]?.children ?? [];
      const nodes = sceneControlNodes(children);
      return {
        index: nodes.findIndex(n => n.start === appState.selected.start),
        root: appState.selected.start === appState.compiled?.nodes?.[0]?.start,
      };
    },
    getMode: () => appState.mode,
    setMode: next => {
      if (appState.mode !== next) onToggleMode();
    },
    onChange: () => {
      getVectorPreview()?.refreshScale?.();
      spacingOverlay?.update();
      onUpdateLayoutInspector?.();
      elementTools?.update();
    },
  });

  const elementTools = createElementTools({
    viewport: $('canvas'),
    artboard: $('preview'),
    toolbar: root.querySelector('.canvas-tools'),
    context: () => {
      const vectorPreview = getVectorPreview();
      if (
        appState.mode !== 'design' ||
        appState.designPresetName !== 'original' ||
        appState.error ||
        (appState.renderer === 'vector' && !vectorPreview)
      )
        return null;
      const rootNode = appState.compiled?.nodes[0];
      const children = rootNode?.children ?? [];
      const scene =
        appState.renderer === 'vector'
          ? vectorPreview.layoutSnapshot()
          : domLayoutSnapshot($('preview'));
      if (!scene) return null;
      return {
        source: appState.files[appState.entry],
        path: appState.entry,
        root: rootNode,
        start: appState.selectedPath === appState.entry ? appState.selected?.start : null,
        top: sceneControlNodes(children),
        pick: start => findNodeByStart(rootNode ? [rootNode] : [], start),
        scene,
        grid: appState.lastVisuals.find(v => v.control === -1)?.grid,
        inserts: projectInserts(appState.files),
        visuals: appState.lastVisuals,
        state: appState.state,
      };
    },
    select: onSelect,
    extract: onExtractControl,
    report: setCaption,
    copy: async text => {
      try {
        await navigatorObj.clipboard.writeText(text);
        setCaption('CSS скопирован в буфер');
      } catch {
        setCaption('Буфер обмена недоступен');
      }
    },
    history: action => {
      const identities = selectionIdentities(appState.compiled?.nodes[0], elementTools.selection());
      if (appState.active !== appState.entry) onOpen(appState.entry);
      const codeEditor = getCodeEditor();
      if (codeEditor[action]()) {
        queueMicrotask(() => {
          clearTimeout(appState.compileTimer);
          onCompile();
          const restored = restoreSelection(appState.compiled?.nodes[0], identities);
          if (restored.nodes.length) onSelect(restored.nodes[0]);
          else {
            onClearSelection();
            if (identities.length) setCaption('Ни один из выбранных контролов не вернулся');
          }
          elementTools.setSelection(restored.starts);
          onRenderInspector();
        });
      }
    },
    commit: (change, context) => {
      if (appState.entry !== context.path || appState.files[appState.entry] !== context.source)
        throw Error('Исходник изменился — повторите операцию');
      const next =
        context.source.slice(0, change.from) + change.insert + context.source.slice(change.to);
      if (next === context.source) return;
      parse(next);
      const vectorPreview = getVectorPreview();
      if (appState.renderer === 'vector' && vectorPreview) {
        compileComponents(
          { ...appState.files, [appState.entry]: next },
          appState.entry,
          appState.state,
          {
            measureText: vectorPreview.measureText,
            designMode: appState.mode === 'design',
            designState:
              appState.scenario < 0
                ? null
                : (appState.compiled?.states[appState.scenario]?.name ?? null),
          },
        );
      }
      if (appState.active !== appState.entry) onOpen(appState.entry);
      getCodeEditor().edit({ from: change.from, to: change.to, insert: change.insert });
      queueMicrotask(() => {
        clearTimeout(appState.compileTimer);
        onCompile();
        const found = findNodeByStart(appState.compiled?.nodes, change.start);
        if (found) onSelect(found);
        if (change.starts) elementTools.setSelection(change.starts);
        elementTools.update();
      });
    },
  });

  function updateTools() {
    canvasTools?.update();
    elementTools?.update();
  }

  function updateSpacingAndTools() {
    spacingOverlay?.update();
    canvasTools?.update();
    elementTools?.update();
  }

  function capturePrimarySelection(rootNode, start) {
    return selectionIdentities(rootNode, [start]);
  }

  function restorePrimarySelection(rootNode, identities) {
    return restoreSelection(rootNode, identities).nodes[0] ?? null;
  }

  function destroy() {
    elementTools?.destroy();
    canvasTools?.destroy();
    spacingOverlay?.destroy();
  }

  return {
    canvasTools,
    elementTools,
    spacingOverlay,
    updateTools,
    updateSpacingAndTools,
    capturePrimarySelection,
    restorePrimarySelection,
    destroy,
  };
}
