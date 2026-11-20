// @ts-check
import { loadVectorRuntime, createVectorPreview } from './vector-preview.js';
import { renderHtmlPreview } from './html-preview.js';
import { createPreviewDebugger } from './preview-debugger.js';
import {
  writePreviewBinding,
  executePreviewStateAction,
  matchPreviewShortcut,
} from './preview-state.js';
import { designPreset, collectionScenario } from './design-presets.js';
import { createComponentCompiler } from '../compiler/components.js';
import { evaluate } from '../compiler/component-semantics.js';
import {
  designStatePatch,
  lintDocument,
  parse,
  resolve,
  validateDesign,
} from '../compiler/language.js';
import { stampStableIds } from '../compiler/element-tree.js';
import { createLatestOnly } from '../shared/latest-only.js';
import { escapeHtml as esc } from '../shared/text.js';
import { studioEventTypes } from '../shared/studio-events.js';

/**
 * Coordinates UI compilation, vector/HTML preview rendering, design presets, and debugger runtime.
 * @param {{ $: (id: string) => any, root?: Document | Element, appState: any, studioCache: any, studioEvents: any, hot?: any, writeStorage: any, onSelectNode: (node: any) => void, onRenderInspector: () => void, onRefreshControlTree: () => void, onRefreshStatesPanel: () => void, onOutput: () => void, onLog: (text: string) => void, onSyncTabs: () => void, onUpdateLayoutInspector: () => void, onUpdateCanvasTools: () => void, onUpdateOverlays: () => void, captureSelection: any, restoreSelectionById: any, documentObj?: Document }} options
 */
export function createPreviewService({
  $,
  root = document,
  appState,
  studioCache,
  studioEvents,
  hot,
  writeStorage,
  onSelectNode,
  onRenderInspector,
  onRefreshControlTree,
  onRefreshStatesPanel,
  onOutput,
  onLog,
  onSyncTabs,
  onUpdateLayoutInspector,
  onUpdateCanvasTools,
  onUpdateOverlays,
  captureSelection,
  restoreSelectionById,
  documentObj = document,
}) {
  const compileComponents = createComponentCompiler({ cache: studioCache });
  const vectorLoads = createLatestOnly();
  let vectorPreview;
  let previewRefreshPending = false;

  const previewDebugger = createPreviewDebugger({
    argumentsOf: (n, event) =>
      (n.eventArgExpressions?.[event] ?? n.eventArgs?.[event] ?? []).map(value =>
        evaluate(value, appState.compiled.defaults, appState.state, [], false, n.environment),
      ),
    execute: (action, args) => execute(action, args),
    log: onLog,
    paused(action) {
      $('continue').disabled = false;
      $('debug-status').textContent = `Пауза перед ${action}`;
      appState.tab = 'state';
      onSyncTabs();
      onOutput();
    },
    resumed() {
      $('continue').disabled = true;
      $('debug-status').textContent = 'Готов';
    },
  });
  const dispatch = previewDebugger.dispatch;

  function schedulePreviewRefresh() {
    if (previewRefreshPending) return;
    previewRefreshPending = true;
    queueMicrotask(() => {
      previewRefreshPending = false;
      try {
        render();
        appState.error = '';
      } catch (e) {
        appState.error = e.message;
        appState.sourceDiagnostic = e.diagnostic ?? null;
      }
      onOutput();
    });
  }

  function syncHotRunningApp() {
    if (!hot || appState.error || !appState.compiled) return;
    try {
      const linked = compileComponents(appState.files, appState.entry, appState.state, {
        ...(vectorPreview ? { measureText: vectorPreview.measureText } : {}),
        designState:
          appState.scenario < 0
            ? null
            : (appState.compiled?.states[appState.scenario]?.name ?? null),
      });
      appState.nativeSource = {
        entry: appState.entry,
        source: appState.files[appState.entry],
        nodes: structuredClone(appState.compiled?.nodes ?? []),
      };
      hot.send('forma:vector-update', linked);
    } catch {}
  }

  function render(designMode = appState.mode === 'design') {
    if (!appState.compiled) return;
    if (appState.renderer === 'vector') {
      if (!vectorPreview) throw Error('Загрузка векторного Rust/WASM…');
      if (!appState.files['components/Button.ui'])
        throw Error('Добавьте components/Button.ui с примитивами кнопки');
      const activeStateName =
        appState.scenario < 0 ? null : (appState.compiled.states[appState.scenario]?.name ?? null);
      const linked = compileComponents(appState.files, appState.entry, appState.state, {
        measureText: vectorPreview.measureText,
        designMode,
        designState: activeStateName,
        instanceProps: designMode ? n => designPreset(appState.designPresetName, n) : undefined,
        transformScene: designMode
          ? scene => collectionScenario(appState.designPresetName, scene, appState.files)
          : undefined,
      });
      appState.lastVisuals = linked.visualNodes ?? [];
      appState.lastPreviewControls = linked.previewControls ?? [];
      const ok = vectorPreview.render({
        container: $('preview'),
        ...linked,
        designMode,
        nodes:
          designMode && appState.designPresetName !== 'original'
            ? linked.previewNodes
            : appState.compiled.nodes,
        selectedStart: appState.selectedPath === appState.entry ? appState.selected?.start : null,
      });
      if (!ok) {
        const err = Error(appState.error || 'Ошибка векторного компонента');
        if (appState.sourceDiagnostic) err.diagnostic = appState.sourceDiagnostic;
        throw err;
      }
      $('preview').style.width = `${vectorPreview.layoutSnapshot().width}px`;
      $('preview').style.minHeight = '0';
      $('preview').style.overflow = 'visible';
      $('caption').textContent = designMode
        ? 'Rust/WASM · компонент из примитивов · щёлкните для выбора'
        : 'Rust/WASM · hover / pressed + transitions · события в журнале';
      if (designMode && appState.designPresetName !== 'original')
        $('caption').textContent =
          'Дизайнерский сценарий · исходник не изменён · редактирование частей отключено';
      $('canvas').classList.toggle('interacting', !designMode);
      onUpdateCanvasTools();
      onUpdateLayoutInspector();
      return;
    }
    renderHtmlPreview({
      preview: $('preview'),
      compiled: appState.compiled,
      state: appState.state,
      files: appState.files,
      interactive: appState.mode !== 'design',
      design: designMode
        ? designStatePatch(appState.compiled, appState.compiled.states[appState.scenario]?.name)
        : null,
      onSelect: onSelectNode,
      onEvent: dispatch,
      onLog,
      onChange: schedulePreviewRefresh,
    });
    $('canvas').classList.toggle('interacting', appState.mode !== 'design');
    onUpdateOverlays();
  }

  function compile(reset = false) {
    appState.persistView();
    try {
      if (!appState.entry || !appState.files[appState.entry])
        throw Error('Откройте .ui компонент и нажмите «Показать UI»');
      const next = parse(appState.files[appState.entry]);
      try {
        appState.lintWarnings = lintDocument(appState.files[appState.entry]) ?? [];
      } catch {
        appState.lintWarnings = [];
      }
      const states = [];
      appState.designFiles = [];
      for (const ref of next.designs) {
        const base = appState.entry.includes('/')
          ? appState.entry.slice(0, appState.entry.lastIndexOf('/') + 1)
          : '';
        const path = base + ref.replace(/^\.\//, '');
        if (!(path in appState.files)) throw Error(`Дизайн-файл не найден: ${path}`);
        appState.designFiles.push(path);
        const design = parse(appState.files[path]);
        if (Object.keys(design.overrides).length || design.states.length) {
          validateDesign(next, design);
          for (const key of Object.keys(design.overrides)) {
            if (Object.hasOwn(next.overrides, key)) throw Error(`Повторный дизайн-key ${key}`);
            Object.defineProperty(next.overrides, key, {
              value: design.overrides[key],
              enumerable: true,
            });
          }
          for (const state of design.states) {
            if (states.some(other => other.name === state.name))
              throw Error(`Повторное состояние ${state.name}`);
            states.push({ ...state, file: path });
          }
        }
      }
      const prevIdentities =
        !reset &&
        appState.selected &&
        appState.selectedPath === appState.entry &&
        appState.compiled?.nodes?.[0]
          ? captureSelection(appState.compiled.nodes[0], appState.selected.start)
          : null;
      stampStableIds(next.nodes, appState.entry);
      appState.compiled = next;
      appState.compiled.states = states;
      if (prevIdentities?.length && appState.compiled.nodes?.[0]) {
        appState.selected = restoreSelectionById(appState.compiled.nodes[0], prevIdentities);
        if (!appState.selected) appState.selectedPath = null;
      }
      appState.scenario = Math.min(appState.scenario, Math.max(0, states.length - 1));
      if (reset) {
        const defState = Object.fromEntries(
          Object.entries(next.stateDefinitions ?? {})
            .filter(([, d]) => d.default !== undefined)
            .map(([k, d]) => [k, resolve(d.default, {})]),
        );
        appState.state = {
          ...defState,
          ...structuredClone(states[appState.scenario]?.state ?? {}),
        };
      }
      $('scenario').innerHTML =
        (states.length ? '<option value="-1">Базовые значения</option>' : '') +
          states.map((s, i) => `<option value="${i}">${esc(s.name)}</option>`).join('') ||
        '<option>Без состояний</option>';
      $('scenario').value = String(appState.scenario);
      $('preview-name').textContent = next.name;
      appState.error = '';
      render();
      if (prevIdentities?.length) onRenderInspector();
      syncHotRunningApp();
      $('status').textContent = 'Live preview обновлён';
    } catch (e) {
      appState.error = e.message;
      appState.sourceDiagnostic = e.diagnostic ?? null;
      $('status').textContent = 'Ошибка · сохранён последний предпросмотр';
    }
    onRefreshControlTree();
    onRefreshStatesPanel();
    onOutput();
  }

  function execute(action, args = []) {
    if (action === 'actions.search') {
      appState.state.status = `Дизайн-обработчик: запрос «${appState.state.query || 'пусто'}»`;
      onLog('Выполнен дизайн-обработчик search (без Rust/backend)');
    } else {
      const defState = Object.fromEntries(
        Object.entries(appState.compiled?.stateDefinitions ?? {})
          .filter(([, d]) => d.default !== undefined)
          .map(([k, d]) => [k, resolve(d.default, {})]),
      );
      const res = executePreviewStateAction(appState.state, action, args, defState);
      if (res.handled) onLog(res.log);
      else onLog(`Обработчик ${action} не подключён`);
    }
    render();
    onOutput();
  }

  function initVector() {
    const current = vectorLoads.begin();
    loadVectorRuntime()
      .then(runtime => {
        if (appState.shellDisposed || !current() || vectorPreview) return;
        vectorPreview = createVectorPreview({
          runtime,
          onSelect: n => {
            if (appState.designPresetName === 'original') onSelectNode(n);
          },
          onError: err => {
            appState.error = String(err?.message ?? err);
            appState.sourceDiagnostic = err?.diagnostic ?? null;
            onOutput();
          },
          onAction: (action, n, event = 'clicked') => {
            if (n) dispatch(action, n, event);
          },
          onControlKey: e => {
            if (!appState.compiled?.shortcuts?.length) return false;
            for (const s of appState.compiled.shortcuts)
              if (matchPreviewShortcut(e, s.key)) {
                const stateDefaults = Object.fromEntries(
                  Object.entries(appState.compiled.stateDefinitions ?? {})
                    .filter(([, d]) => d.default !== undefined)
                    .map(([k, d]) => [k, d.default]),
                );
                const previewEnv = {
                  enums: { ...appState.compiled.enums, ...appState.compiled.structs },
                  tokens: appState.compiled.tokens,
                  i18n: appState.compiled.i18n,
                  styles: appState.compiled.styles,
                  stateDefaults,
                  allowMissingState: true,
                };
                const syntheticNode = {
                  type: 'Shortcut',
                  start: s.from ?? 0,
                  props: {},
                  events: { shortcut: s.action },
                  eventArgs: { shortcut: (s.args ?? []).map(a => resolve(a, appState.state)) },
                  eventArgExpressions: { shortcut: s.args ?? [] },
                  environment: previewEnv,
                };
                dispatch(s.action, syntheticNode, 'shortcut');
                return true;
              }
            return false;
          },
          onBindingChange: changes => {
            for (const { node, path, value } of changes)
              if (writePreviewBinding(appState.state, node, path, value)) {
                onLog(`${path} = ${JSON.stringify(value)}`);
                if (node.events.changed) dispatch(node.events.changed, node, 'changed');
              }
            schedulePreviewRefresh();
          },
        });
        if (appState.renderer === 'vector') compile();
      })
      .catch(e => {
        if (appState.renderer === 'vector') {
          appState.error = `Векторный WASM не собран: ${e.message}`;
          onOutput();
        }
      });
  }

  const rendererPicker = documentObj.createElement('select');
  rendererPicker.id = 'renderer';
  rendererPicker.setAttribute('aria-label', 'Рендерер предпросмотра');
  rendererPicker.innerHTML =
    '<option value="html">HTML · прежний</option><option value="vector">Вектор · Rust/WASM</option>';
  $('scenario').before(rendererPicker);
  rendererPicker.value = appState.renderer;

  const presetPicker = documentObj.createElement('select');
  presetPicker.className = 'design-preset';
  presetPicker.setAttribute('aria-label', 'Проверка дизайна');
  for (const [value, label] of [
    ['original', 'Исходный вид'],
    ['long', 'Длинный текст'],
    ['empty', 'Пустой текст'],
    ['disabled', 'Недоступные контролы'],
    ['list-empty', 'Список: пусто'],
    ['list-12', 'Список: 12 строк'],
    ['list-100', 'Список: 100 строк'],
    ['loading', 'Список: загрузка'],
    ['error', 'Список: ошибка'],
  ])
    presetPicker.add(new Option(label, value));
  root.querySelector('.canvas-tools')?.append(presetPicker);
  presetPicker.title = 'Только предпросмотр: исходники и нативное приложение не меняются';

  function setRenderer(nextRenderer) {
    appState.renderer = nextRenderer;
    rendererPicker.value = nextRenderer;
    writeStorage('localStorage', appState.storageKey('forma-renderer'), nextRenderer);
    appState.selected = null;
    vectorPreview?.destroy();
    vectorPreview = undefined;
    initVector();
    compile();
  }

  function setBackend(nextBackend) {
    appState.backend = nextBackend;
    writeStorage('localStorage', appState.storageKey('forma-backend'), nextBackend);
    hot?.send('forma:backend', { backend: nextBackend });
  }

  function setPlatform(nextPlatform) {
    appState.platform = nextPlatform;
    writeStorage('localStorage', appState.storageKey('forma-platform'), nextPlatform);
    hot?.send('forma:platform', { platform: nextPlatform });
  }

  function toggleMode() {
    appState.mode = appState.mode === 'design' ? 'interact' : 'design';
    writeStorage('sessionStorage', appState.storageKey('forma-canvas-mode'), appState.mode);
    studioEvents.emit(studioEventTypes.mode, { mode: appState.mode });
    $('run').textContent = appState.mode === 'design' ? 'Взаимодействие' : 'Дизайн';
    $('caption').textContent =
      appState.mode === 'design'
        ? 'Выберите элемент, чтобы найти его в разметке'
        : 'События выполняются в дизайн-runtime · Rust не подключён';
    render();
  }

  function activateCurrentAsEntry() {
    if (appState.renderer === 'vector' && appState.active === 'components/Button.ui') {
      compile();
      return;
    }
    appState.entry = appState.active;
    appState.scenario = 0;
    compile(true);
  }

  function bindEvents() {
    rendererPicker.onchange = () => setRenderer(rendererPicker.value);
    presetPicker.onchange = () => {
      appState.designPresetName = presetPicker.value;
      try {
        if (appState.mode !== 'design') toggleMode();
        else render();
        appState.error = '';
      } catch (e) {
        appState.error = e.message;
        appState.sourceDiagnostic = e.diagnostic ?? null;
      }
      onRenderInspector();
      onRefreshControlTree();
      onRefreshStatesPanel();
      onOutput();
    };
    $('scenario').onchange = e => {
      appState.scenario = Number(e.target.value);
      compile(true);
    };
    $('entry').onclick = activateCurrentAsEntry;
    $('run').onclick = toggleMode;
    $('desktop').onclick = () => {
      $('preview').style.width = '520px';
      $('desktop').classList.add('chosen');
      $('mobile').classList.remove('chosen');
    };
    $('mobile').onclick = () => {
      $('preview').style.width = '320px';
      $('mobile').classList.add('chosen');
      $('desktop').classList.remove('chosen');
    };
    $('theme').onclick = () => $('preview').classList.toggle('light');
    $('break').onchange = e => previewDebugger.setBreak(e.target.checked);
    $('continue').onclick = () => previewDebugger.resume();
    $('reset').onclick = () => {
      previewDebugger.reset();
      compile(true);
      onLog('Состояние предпросмотра восстановлено');
    };
  }

  function destroy() {
    vectorLoads.cancel();
    vectorPreview?.destroy();
    vectorPreview = undefined;
  }

  function mountCanvasToolbar() {
    root.querySelector('.canvas-tools')?.append(presetPicker);
  }

  return {
    compileComponents,
    compile,
    render,
    execute,
    dispatch,
    initVector,
    bindEvents,
    mountCanvasToolbar,
    destroy,
    toggleMode,
    activateCurrentAsEntry,
    setRenderer,
    setBackend,
    setPlatform,
    rendererPicker,
    get vectorPreview() {
      return vectorPreview;
    },
    get breakOn() {
      return previewDebugger.breakOn;
    },
    get pending() {
      return previewDebugger.pending;
    },
  };
}
