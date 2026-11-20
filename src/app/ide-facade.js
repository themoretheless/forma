// @ts-check
import { DEFAULT_BACKEND, DEFAULT_PLATFORM, isBackend, isPlatform } from '../../protocol/enums.js';
import { designReferences, setDesignData } from '../compiler/design-data.js';
import { mountDevBridge } from '../shell/dev-bridge.js';
import { createIdeCommands } from './mcp-commands.js';

/** Connects the Studio IDE MCP command facade and dev-server HMR bridge. */
export function attachStudioBridges({
  $,
  hot,
  appState,
  commands,
  coordinator,
  services,
  liveSource,
  documentObj = document,
}) {
  const { workspace, output, editor, tree, inspector, canvas, preview } = services;
  const ideCommand = createIdeCommands({
    $,
    hot,
    compileComponents: preview.compileComponents,
    get files() {
      return appState.files;
    },
    set files(v) {
      appState.files = v;
    },
    get active() {
      return appState.active;
    },
    set active(v) {
      appState.active = v;
    },
    get entry() {
      return appState.entry;
    },
    set entry(v) {
      appState.entry = v;
    },
    get scenario() {
      return appState.scenario;
    },
    set scenario(v) {
      appState.scenario = v;
    },
    get selected() {
      return appState.selected;
    },
    set selected(v) {
      appState.selected = v;
    },
    get logs() {
      return appState.logs;
    },
    set logs(v) {
      appState.logs = v;
    },
    get nativeSource() {
      return appState.nativeSource;
    },
    set nativeSource(v) {
      appState.nativeSource = v;
    },
    get compiled() {
      return appState.compiled;
    },
    get state() {
      return appState.state;
    },
    get error() {
      return appState.error;
    },
    get mode() {
      return appState.mode;
    },
    get renderer() {
      return appState.renderer;
    },
    get backend() {
      return appState.backend ?? DEFAULT_BACKEND;
    },
    set backend(v) {
      if (isBackend(v)) preview.setBackend(v);
    },
    get platform() {
      return appState.platform ?? DEFAULT_PLATFORM;
    },
    set platform(v) {
      if (isPlatform(v)) preview.setPlatform(v);
    },
    get breakOn() {
      return preview.breakOn;
    },
    get pending() {
      return preview.pending;
    },
    get vectorPreview() {
      return preview.vectorPreview;
    },
    get controlTree() {
      return tree.controlTree;
    },
    get codeEditor() {
      return editor.codeEditor;
    },
    get spacingOverlay() {
      return canvas.spacingOverlay;
    },
    get rendererPicker() {
      return preview.rendererPicker;
    },
    open: coordinator.open,
    persist: workspace.persist,
    compile: preview.compile,
    render: preview.render,
    output: output.output,
    select: coordinator.select,
    commitSource: coordinator.commitSource,
    designStateFile: tree.designStateFile,
    designStateEdit: tree.designStateEdit,
    resetProjectEditing: coordinator.resetProjectEditing,
    dispatch: preview.dispatch,
    liveSource,
    propertyInput: name =>
      Array.from(
        /** @type {NodeListOf<HTMLElement>} */ (documentObj.querySelectorAll('[data-prop]')),
      ).find(el => el.dataset.prop === name),
  });

  const devBridge = hot
    ? mountDevBridge(hot, {
        anchor: $('run'),
        log: output.log,
        setDesignData,
        designReferences,
        ideCommand,
        registerCommand: (id, spec) => commands.register(id, spec),
        showNativeInspection: inspector.showNativeInspection,
        get compiled() {
          return appState.compiled;
        },
        get files() {
          return appState.files;
        },
        showEvents() {
          appState.tab = 'events';
          output.syncTabs();
          output.output();
        },
        setError(message) {
          appState.error = message;
          output.output();
        },
        compile: () => preview.compile(),
        persist: workspace.persist,
      })
    : null;

  return { ideCommand, devBridge };
}
