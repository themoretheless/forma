// @ts-check
import './canvas/canvas-tools.css';
import './shell/style.css';
import './canvas/selection.css';
import './preview/vector-artboard.css';
import './tree/control-tree.css';
import './inspector/property-inspector.css';
import './tree/states-panel.css';

import { createCacheBudget } from './shared/cache-budget.js';
import { createEventScope } from './shared/event-scope.js';
import { createStudioEvents, studioEventTypes } from './shared/studio-events.js';
import { writeStorage } from './workspace/browser-storage.js';
import { sampleProject } from './workspace/sample-project.js';
import { studioControlsProject } from './workspace/studio-controls-builtin.js';
import { createWorkspaceService } from './workspace/workspace-service.js';
import { createOutputService } from './output/output-service.js';
import { createEditorService } from './editor/editor-service.js';
import { createTreeService } from './tree/tree-service.js';
import { createInspectorService } from './inspector/inspector-service.js';
import { createCanvasService } from './canvas/canvas-service.js';
import { createPreviewService } from './preview/preview-service.js';
import { mountStudioShell } from './shell/studio-shell.js';
import { mountMinimalShell } from './shell/minimal-shell.js';
import { createStudioState } from './app/state.js';
import { createCommandRegistry, registerStudioCommands } from './app/commands.js';
import { renderStudioDom } from './app/dom-template.js';
import { createStudioCoordinator } from './app/coordinator.js';
import { attachStudioBridges } from './app/ide-facade.js';

const studioEvents = createStudioEvents();
const studioCache = createCacheBudget();
const codeEvents = createEventScope();
const commands = createCommandRegistry();
const appState = createStudioState({
  getInitialProject: isGuide => (isGuide ? studioControlsProject : sampleProject),
});
const appRoot = document.querySelector('#app');
const $ = renderStudioDom(appRoot, appState);
const liveSource = path => appState.liveSource(path, () => $('code').value);

const saveStatus = (status, text) => {
  $('saved').textContent = text;
  studioEvents.emit(studioEventTypes.save, { status, text });
};
const setStatus = text => ($('status').textContent = text);
const setCaption = text => ($('caption').textContent = text);

const coordinator = createStudioCoordinator({
  $,
  appState,
  ports: {
    editor: () => editor,
    tree: () => tree,
    inspector: () => inspector,
    canvas: () => canvas,
    preview: () => preview,
    devBridge: () => bridges?.devBridge,
  },
});

const output = createOutputService({
  $,
  root: appRoot,
  appState,
  studioEvents,
  onOpenLocation: target => {
    coordinator.open(target.file);
    $('code').setSelectionRange(target.from, target.to);
    $('code').focus();
  },
});

const workspace = createWorkspaceService({
  $,
  appState,
  onResetEditing: coordinator.resetProjectEditing,
  onOpenFile: coordinator.open,
  onRenderTree: force => tree.tree(force),
  onCompile: reset => preview.compile(reset),
  onProjectNameChange: name => {
    if ($('project-switcher-label')) $('project-switcher-label').textContent = name;
  },
  reportError: output.reportError,
  saveStatus,
  setStatus,
});

const editor = createEditorService({
  $,
  appState,
  onSaveStatus: saveStatus,
  onPersist: workspace.persist,
  onCompile: reset => preview.compile(reset),
  onOpen: coordinator.open,
  onHighlightUpdate: () => canvas?.updateSpacingAndTools(),
  onSourceInputResetSelection: coordinator.resetSelectionOnInput,
  onFormatSuccess: setStatus,
  reportError: output.reportError,
});

const tree = createTreeService({
  $,
  root: appRoot,
  appState,
  studioCache,
  liveSource,
  open: coordinator.open,
  persist: workspace.persist,
  compile: reset => preview.compile(reset),
  selectTemplateControl: coordinator.selectTemplateControl,
  commitSource: coordinator.commitSource,
  openFolder: workspace.openFolder,
  reportError: output.reportError,
  setStatus,
  setCaption,
  clearCompileTimer: () => clearTimeout(appState.compileTimer),
});

const inspector = createInspectorService({
  $,
  root: appRoot,
  appState,
  studioEvents,
  liveSource,
  commitSource: coordinator.commitSource,
  reportError: output.reportError,
  getBatchSelection: {
    get: () => canvas.elementTools?.selection() ?? [],
    set: starts => canvas.elementTools?.setSelection(starts),
  },
  getVectorPreview: () => preview.vectorPreview,
  openCodeRange: coordinator.openCodeRange,
  commitLayoutEdit: coordinator.commitLayoutEdit,
  selectTemplateControl: coordinator.selectTemplateControl,
});

const canvas = createCanvasService({
  $,
  root: appRoot,
  appState,
  compileComponents: (...args) => preview.compileComponents(...args),
  getVectorPreview: () => preview.vectorPreview,
  onToggleMode: () => preview.toggleMode(),
  onSelect: coordinator.select,
  onClearSelection: coordinator.clearSelection,
  onExtractControl: item => tree.extractTreeControl(item),
  onRenderInspector: inspector.renderInspector,
  onUpdateLayoutInspector: inspector.updateLayout,
  onOpen: coordinator.open,
  onCompile: () => preview.compile(),
  getCodeEditor: () => editor.codeEditor,
  setCaption,
});

const preview = createPreviewService({
  $,
  root: appRoot,
  appState,
  studioCache,
  studioEvents,
  hot: import.meta.hot,
  writeStorage,
  onSelectNode: coordinator.select,
  onRenderInspector: inspector.renderInspector,
  onRefreshControlTree: tree.refreshControlTree,
  onRefreshStatesPanel: tree.refreshStatesPanel,
  onOutput: output.output,
  onLog: output.log,
  onSyncTabs: output.syncTabs,
  onUpdateLayoutInspector: inspector.updateLayout,
  onUpdateCanvasTools: canvas.updateTools,
  onUpdateOverlays: canvas.updateSpacingAndTools,
  captureSelection: canvas.capturePrimarySelection,
  restoreSelectionById: canvas.restorePrimarySelection,
});

preview.mountCanvasToolbar();
output.bindTabs();
workspace.bindUpload();
editor.bindEvents(codeEvents);
tree.bindEvents(codeEvents);
preview.bindEvents();
registerStudioCommands(commands, { state: appState, workspace, tree, editor, preview });
const services = { workspace, output, editor, tree, inspector, canvas, preview };
const bridges = attachStudioBridges({
  $,
  hot: import.meta.hot,
  appState,
  commands,
  coordinator,
  services,
  liveSource,
});

coordinator.open(appState.active);
preview.compile(true);
studioEvents.emit(studioEventTypes.mode, { mode: appState.mode });

let studioShell;
const minimalShell = mountMinimalShell(appRoot, {
  events: studioEvents,
  commands,
  getTitle: () => appState.projectName,
  newFile: () => $('new').click(),
  clearSelection: () => {
    coordinator.clearSelection();
    tree.controlTree?.select(null, null);
  },
  inspector: inspector.propertyInspector,
  layers: tree.controlTree,
});
mountStudioShell(appRoot)
  .then(shell => (appState.shellDisposed ? shell.destroy() : (studioShell = shell)))
  .catch(e => output.log(`Контролы Studio: ${e.message}`));
preview.initVector();

import.meta.hot?.dispose(() => {
  appState.shellDisposed = true;
  codeEvents.dispose();
  tree.destroy();
  minimalShell.destroy();
  clearTimeout(appState.saveTimer);
  clearTimeout(appState.compileTimer);
  studioShell?.destroy();
  canvas.destroy();
  inspector.destroy();
  preview.destroy();
});
