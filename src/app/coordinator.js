// @ts-check
import { setDesignData } from '../compiler/design-data.js';
import { findNodeByStart } from '../compiler/element-tree.js';
import { parse } from '../compiler/language.js';
import { countLines } from '../shared/text.js';

/** Coordinates cross-panel file opening, control selection, and source commits. */
export function createStudioCoordinator({ $, appState, ports, documentObj = document }) {
  function resetProjectEditing() {
    clearTimeout(appState.saveTimer);
    clearTimeout(appState.compileTimer);
    appState.saveTimer = appState.compileTimer = undefined;
    setDesignData({});
    ports.editor()?.codeEditor?.resetProject();
    ports.devBridge()?.revokeTrust?.();
  }

  function select(n) {
    const preview = ports.preview();
    const inspector = ports.inspector();
    const editor = ports.editor();
    const tree = ports.tree();
    appState.selected = n;
    appState.selectedPath = appState.entry;
    preview.vectorPreview?.select(n.start);
    inspector.updateLayout();
    if (appState.active !== appState.entry) open(appState.entry);
    $('code').setSelectionRange(n.start, n.start);
    const line = countLines(appState.files[appState.entry], n.start);
    $('code').scrollTop = Math.max(0, (line - 4) * 23);
    $('lines').scrollTop = $('code').scrollTop;
    /** @type {NodeListOf<HTMLElement>} */ (documentObj.querySelectorAll('.ui-node')).forEach(el =>
      el.classList.toggle('selected', Number(el.dataset.start) === n.start),
    );
    appState.inspectorNote = '';
    inspector.renderInspector();
    editor.lines();
    tree.controlTree?.select(n.start, appState.entry);
  }

  function clearSelection() {
    const preview = ports.preview();
    const canvas = ports.canvas();
    const inspector = ports.inspector();
    const editor = ports.editor();
    const tree = ports.tree();
    appState.selected = null;
    appState.selectedPath = null;
    preview.vectorPreview?.select(null);
    canvas.elementTools?.setSelection([]);
    inspector.updateLayout();
    documentObj
      .querySelectorAll('.ui-node.selected')
      .forEach(el => el.classList.remove('selected'));
    appState.inspectorNote = '';
    inspector.renderInspector();
    editor.lines();
    tree.controlTree?.select(null, null);
  }

  function selectTemplateControl(item) {
    const preview = ports.preview();
    const inspector = ports.inspector();
    const editor = ports.editor();
    const tree = ports.tree();
    if (
      item.path === appState.entry ||
      appState.files[item.path] === appState.files[appState.entry]
    ) {
      if (appState.mode !== 'design') preview.toggleMode();
      select(item.node);
      return;
    }
    appState.selected = item.node;
    appState.selectedPath = item.path;
    preview.vectorPreview?.select(null);
    documentObj
      .querySelectorAll('.ui-node.selected')
      .forEach(el => el.classList.remove('selected'));
    if (appState.active !== item.path) open(item.path);
    $('code').setSelectionRange(item.node.start, item.node.start);
    appState.inspectorNote = 'Узел шаблона. Геометрию конкретного экземпляра здесь выбрать нельзя.';
    inspector.renderInspector();
    editor.lines();
    tree.controlTree.select(item.node.start, item.path);
  }

  function open(path) {
    appState.active = path;
    appState.selectedTreeItem = path;
    appState.persistView();
    ports.editor().syncOpenDocument(path);
    const tree = ports.tree();
    tree.tree();
    tree.refreshControlTree();
    tree.refreshStatesPanel();
  }

  function commitSource(spec) {
    ports.editor().commitSource(spec, {
      onReselect: start => {
        const found = findNodeByStart(appState.compiled?.nodes, start);
        if (found) select(found);
      },
    });
  }

  function resetSelectionOnInput() {
    const tree = ports.tree();
    appState.selected = null;
    appState.selectedPath = null;
    tree.controlTree?.select(null, null);
    tree.refreshControlTree();
    ports.editor().highlightSource();
    documentObj
      .querySelectorAll('.ui-node.selected')
      .forEach(el => el.classList.remove('selected'));
  }

  function openCodeRange(source) {
    open(source.file);
    $('code').setSelectionRange(source.from, source.to);
    $('code').focus();
  }

  function commitLayoutEdit(source, insert) {
    const previous = appState.files[source.file];
    if (previous === undefined) throw Error('Файл не найден');
    parse(previous.slice(0, source.from) + insert + previous.slice(source.to));
    if (appState.active !== source.file) open(source.file);
    ports.editor().codeEditor.edit({ from: source.from, to: source.to, insert });
    ports.preview().compile();
  }

  return {
    resetProjectEditing,
    select,
    clearSelection,
    selectTemplateControl,
    open,
    commitSource,
    resetSelectionOnInput,
    openCodeRange,
    commitLayoutEdit,
  };
}
