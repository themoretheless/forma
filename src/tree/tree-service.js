// @ts-check
import { fileTreeHtml, visibleTreePaths } from './file-tree-model.js';
import { createControlTree } from './control-tree.js';
import { createStatesPanel } from './states-panel.js';
import {
  componentPath,
  createFile,
  duplicateFile,
  moveDestination,
  newFileContent,
  renameFile,
  suggestComponentName,
} from '../workspace/project-files.js';
import { stateCreateEdit, stateDeleteEdit, stateRenameEdit } from '../compiler/design-states.js';
import { extractElement, moveAmongSiblings, moveIntoContainer } from '../compiler/element-edit.js';
import { parse } from '../compiler/language.js';
import { createOperationHistory } from '../shared/operation-history.js';
import { parentDirOf } from '../shared/text.js';

/**
 * Coordinates project file tree, control tree, and design states panel interactions.
 * @param {{ $: (id: string) => any, root?: Document | Element, appState: any, studioCache: any, liveSource: (path: string) => string, open: (path: string) => void, persist: () => void, compile: (reset?: boolean) => void, selectTemplateControl: (item: any) => void, commitSource: (spec: any) => void, openFolder: () => void, reportError: (err: any) => void, setStatus: (text: string) => void, setCaption: (text: string) => void, clearCompileTimer: () => void, documentObj?: Document, windowObj?: Window, navigatorObj?: Navigator }} options
 */
export function createTreeService({
  $,
  root = document,
  appState,
  studioCache,
  liveSource,
  open,
  persist,
  compile,
  selectTemplateControl,
  commitSource,
  openFolder,
  reportError,
  setStatus,
  setCaption,
  clearCompileTimer,
  documentObj = document,
  windowObj = window,
  navigatorObj = navigator,
}) {
  const treeHistory = createOperationHistory({ announce: setStatus });
  let treeContextMenu = null;

  function treeOutcome(result) {
    if (!result.ok) setStatus(result.reason);
    return result.ok;
  }

  function selectTreeRow(targetPath) {
    appState.selectedTreeItem = targetPath;
    documentObj.body.classList.toggle('is-ui-file', !!targetPath?.endsWith('.ui'));
    for (const b of $('tree').querySelectorAll('.file')) {
      b.classList.toggle('active', b.dataset.path === appState.selectedTreeItem);
    }
  }

  function selectedTargetDir() {
    return appState.selectedTreeItem ? parentDirOf(appState.selectedTreeItem) : '';
  }

  async function activateTreeFile(path) {
    selectTreeRow(path);
    if (path.endsWith('.ui')) {
      if (!path.endsWith('.design.ui')) {
        appState.entry = path;
        appState.scenario = 0;
      }
      controlTree?.setView({ visible: true });
      open(path);
      if (!path.endsWith('.design.ui')) compile(true);
    } else {
      open(path);
      documentObj.body.classList.add('show-code');
    }
  }

  async function applyTreeRename(fromPath, toPath) {
    if (treeOutcome(renameFile(appState.files, fromPath, toPath))) {
      if (appState.active === fromPath) open(toPath);
      else {
        appState.selectedTreeItem = toPath;
        tree(true);
      }
      persist();
      return true;
    }
    return false;
  }

  async function moveTreeItem(fromPath, targetDir) {
    if (!fromPath) return;
    const name = fromPath.split('/').at(-1);
    const toPath = moveDestination(fromPath, targetDir);
    if (!toPath || !Object.hasOwn(appState.files, fromPath)) return;
    if (await applyTreeRename(fromPath, toPath)) {
      treeHistory.push({
        label: `перемещение «${name}»`,
        undo: () => applyTreeRename(toPath, fromPath),
        redo: () => applyTreeRename(fromPath, toPath),
      });
    }
  }

  function bindTreeEvents(scope = $('tree')) {
    scope.querySelectorAll('[data-path]').forEach(b => {
      b.onclick = () => activateTreeFile(b.dataset.path);
    });
    scope.querySelectorAll('.file').forEach(b => {
      const itemPath = b.dataset.path;
      b.ondragstart = e => {
        appState.draggedTreePath = itemPath;
        b.classList.add('dragging');
        e.dataTransfer.effectAllowed = 'move';
        e.dataTransfer.setData('text/plain', itemPath);
      };
      b.ondragend = () => {
        appState.draggedTreePath = null;
        b.classList.remove('dragging');
        $('tree').classList.remove('drop-root');
        $('tree')
          .querySelectorAll('.drop-target')
          .forEach(el => el.classList.remove('drop-target'));
      };
      b.ondragover = e => {
        if (!appState.draggedTreePath) return;
        const targetFolder = parentDirOf(b.dataset.path);
        if (
          targetFolder === appState.draggedTreePath ||
          targetFolder.startsWith(`${appState.draggedTreePath}/`)
        )
          return;
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer.dropEffect = 'move';
        $('tree').classList.remove('drop-root');
        $('tree')
          .querySelectorAll('.drop-target')
          .forEach(el => el.classList.remove('drop-target'));
        $('tree').classList.add('drop-root');
      };
      b.ondrop = async e => {
        e.preventDefault();
        e.stopPropagation();
        const from = appState.draggedTreePath || e.dataTransfer.getData('text/plain');
        const targetFolder = parentDirOf(b.dataset.path);
        $('tree').classList.remove('drop-root');
        $('tree')
          .querySelectorAll('.drop-target')
          .forEach(el => el.classList.remove('drop-target'));
        await moveTreeItem(from, targetFolder);
      };
    });
  }

  function tree(force = false) {
    const paths = visibleTreePaths(appState.files, appState.treeFilter);
    const buttons = [...$('tree').querySelectorAll('[data-path]')];
    const existing = new Set(buttons.map(b => b.dataset.path));
    if (!force && buttons.length === paths.length && paths.every(path => existing.has(path))) {
      selectTreeRow(appState.selectedTreeItem);
      return;
    }
    $('tree').innerHTML = fileTreeHtml(paths, appState.selectedTreeItem);
    bindTreeEvents();
  }

  function closeTreeContextMenu() {
    if (treeContextMenu) treeContextMenu.hidden = true;
  }

  function startTreeInlineInput({ mode: inputMode, targetPath = '', dir = '' }) {
    closeTreeContextMenu();
    const existing = $('tree').querySelector('.tree-inline-row');
    if (existing) existing.remove();
    const row = documentObj.createElement('div');
    row.className = 'tree-inline-row';
    const input = documentObj.createElement('input');
    input.type = 'text';
    input.className = 'tree-inline-input';
    input.spellcheck = false;
    const initialName =
      inputMode === 'rename'
        ? targetPath.split('/').at(-1)
        : inputMode === 'new-folder'
          ? 'new-folder'
          : 'NewComponent.ui';
    input.value = initialName;
    row.append(input);
    let anchorBtn = null;
    if (inputMode === 'rename') {
      anchorBtn = $('tree').querySelector(`.file[data-path="${CSS.escape(targetPath)}"]`);
      if (anchorBtn) {
        const lvl = Number(anchorBtn.getAttribute('aria-level') ?? 1);
        row.style.paddingLeft = `${8 + (lvl - 1) * 12}px`;
        anchorBtn.after(row);
        anchorBtn.hidden = true;
      }
    }
    if (!row.isConnected) {
      row.style.paddingLeft = '8px';
      $('tree').prepend(row);
    }
    input.focus();
    const dotIdx = initialName.lastIndexOf('.');
    if (dotIdx > 0 && inputMode !== 'new-folder') input.setSelectionRange(0, dotIdx);
    else input.select();
    let done = false;
    const cleanup = () => {
      if (anchorBtn) anchorBtn.hidden = false;
      row.remove();
    };
    const commit = async () => {
      if (done) return;
      done = true;
      const val = input.value.trim();
      cleanup();
      if (!val || (inputMode === 'rename' && val === initialName)) return;
      if (inputMode === 'rename') {
        const parent = parentDirOf(targetPath);
        const toPath = parent ? `${parent}/${val}` : val;
        if (await applyTreeRename(targetPath, toPath)) {
          treeHistory.push({
            label: `переименование «${initialName}» → «${val}»`,
            undo: () => applyTreeRename(toPath, targetPath),
            redo: () => applyTreeRename(targetPath, toPath),
          });
        }
      } else if (inputMode === 'new-file') {
        const rel = dir ? `${dir}/${val}` : val;
        if (!treeOutcome(createFile(appState.files, rel, newFileContent(rel)))) return;
        open(rel);
        persist();
      }
    };
    input.onkeydown = e => {
      if (e.key === 'Enter') {
        e.preventDefault();
        commit();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        done = true;
        cleanup();
        $('tree').focus();
      }
    };
    input.onblur = () => commit();
  }

  async function createInTree(kind = 'file', baseDir = selectedTargetDir()) {
    startTreeInlineInput({ mode: kind === 'folder' ? 'new-folder' : 'new-file', dir: baseDir });
  }

  async function duplicateTreeItem(targetPath = appState.selectedTreeItem) {
    if (!targetPath) return;
    const copy = duplicateFile(appState.files, targetPath);
    if (!copy.ok) return;
    open(copy.path);
    persist();
  }

  async function deleteTreeItem(targetPath = appState.selectedTreeItem) {
    if (!targetPath) return;
    const name = targetPath.split('/').at(-1);
    if (!windowObj.confirm(`Удалить «${name}»?`)) return;
    if (Object.hasOwn(appState.files, targetPath)) {
      delete appState.files[targetPath];
      const nextFile = Object.keys(appState.files)[0];
      if (nextFile) open(nextFile);
      else tree(true);
      persist();
    }
  }

  function locateActiveInTree() {
    if (!appState.active) return;
    selectTreeRow(appState.active);
    const btn = $('tree').querySelector(`.file[data-path="${CSS.escape(appState.active)}"]`);
    btn?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }

  const treeSearchBar = $('tree-search-bar');
  const treeSearchInput = $('tree-search-input');
  const treeSearchToggle = $('tree-search-toggle');
  const treeSearchClear = $('tree-search-clear');

  function toggleTreeSearch(forceOpen) {
    if (!treeSearchBar) return;
    const isOpen = forceOpen !== undefined ? forceOpen : treeSearchBar.hidden;
    treeSearchBar.hidden = !isOpen;
    treeSearchToggle?.classList.toggle('active', isOpen);
    if (isOpen) {
      treeSearchInput?.focus();
      treeSearchInput?.select();
    } else if (appState.treeFilter) {
      appState.treeFilter = '';
      if (treeSearchInput) treeSearchInput.value = '';
      tree(true);
    }
  }

  function refreshControlTree() {
    const isUi = !!appState.active?.endsWith('.ui');
    documentObj.body.classList.toggle('is-ui-file', isUi);
    if (!controlTree) return;
    const path = appState.active?.endsWith('.ui')
      ? appState.active
      : controlTree.scope === 'designer'
        ? appState.entry
        : appState.active;
    let doc = null;
    let message = '';
    let stale = false;
    if (!path?.endsWith('.ui')) {
      message = 'Откройте файл .ui';
    } else {
      try {
        const source = appState.files[path];
        const previous =
          studioCache.get(`tree:${path}`) ??
          (appState.activeTreeDocument?.path === path ? appState.activeTreeDocument : null);
        doc = previous?.source === source ? previous.document : parse(source);
        if (previous?.document !== doc) studioCache.set(`tree:${path}`, { source, document: doc });
        appState.activeTreeDocument = { path, source, document: doc };
        if (controlTree.scope === 'designer' && appState.error) message = appState.error;
      } catch (e) {
        doc =
          (appState.activeTreeDocument?.path === path
            ? appState.activeTreeDocument.document
            : studioCache.get(`tree:${path}`)?.document) ?? null;
        message = e.message;
        stale = !!doc;
      }
    }
    controlTree.update({
      document: doc,
      path,
      error: message,
      stale,
      selectedStart: appState.selected?.start,
      selectedPath:
        appState.selectedPath === appState.entry && path === appState.active
          ? appState.active
          : appState.selectedPath,
      files: appState.files,
    });
  }

  function designFilePath() {
    if (appState.active.endsWith('.design.ui')) return appState.active;
    return appState.designFiles[0] ?? null;
  }

  function designStateFile() {
    const path = designFilePath();
    if (!path) throw Error('Дизайн-файл не выбран');
    return { path, source: liveSource(path) };
  }

  function designStateEdit(build, expectedContent) {
    const { path, source } = designStateFile();
    if (expectedContent !== undefined && expectedContent !== source)
      throw Error('Дизайн-файл изменился: обновите его через design_states_read');
    const edit = build(source);
    commitSource({ file: path, source, ...edit });
    queueMicrotask(() => {
      clearCompileTimer();
      compile();
    });
    return {
      file: path,
      content: source.slice(0, edit.from) + edit.insert + source.slice(edit.to),
    };
  }

  function refreshStatesPanel() {
    if (!statesPanel) return;
    const path = designFilePath();
    let states = [];
    let baseCount = 0;
    let message = '';
    let editable = appState.designPresetName === 'original';
    if (path) {
      const source = liveSource(path);
      try {
        const doc = parse(source);
        if (!doc.designBody) throw Error('Файл не является дизайн-файлом');
        states = doc.states.map(state => ({ name: state.name, count: state.nodes.length }));
        baseCount = doc.entries.length;
      } catch (e) {
        message = e.message;
        editable = false;
      }
    }
    statesPanel.update({
      path,
      states,
      baseCount,
      editable,
      error: message,
      active: appState.scenario < 0 ? null : appState.compiled?.states[appState.scenario]?.name,
    });
  }

  function reorderTreeControl({ path, start, targetStart, side }) {
    const source =
      appState.activeTreeDocument?.path === path
        ? appState.activeTreeDocument.source
        : appState.files[path];
    if (source === undefined) {
      setCaption('Файл не найден');
      return;
    }
    try {
      const change =
        side === 'inside'
          ? moveIntoContainer(source, start, targetStart)
          : moveAmongSiblings(source, start, targetStart, side);
      if (change)
        commitSource({
          file: path,
          source,
          from: change.from,
          to: change.to,
          insert: change.insert,
          reselect: path === appState.entry ? change.start : undefined,
        });
    } catch (e) {
      setCaption(e.message);
    }
  }

  async function extractTreeControl({ path, start, node }) {
    const source =
      liveSource(path) ??
      (appState.activeTreeDocument?.path === path
        ? appState.activeTreeDocument.source
        : appState.files[path]);
    if (source === undefined) {
      setCaption('Файл не найден');
      return;
    }
    const input = windowObj.prompt(
      'Имя нового компонента (.ui в папке components/):',
      suggestComponentName(appState.files, node),
    );
    if (!input) return;
    const name = input.trim().replace(/\.ui$/i, '');
    try {
      const compPath = componentPath(name);
      if (
        Object.hasOwn(appState.files, compPath) &&
        !windowObj.confirm(`Файл ${compPath} уже существует. Перезаписать?`)
      )
        return;
      const change = extractElement(source, start, name);
      appState.files[change.componentPath] = change.componentSource;
      commitSource({
        file: path,
        source,
        from: change.from,
        to: change.to,
        insert: change.insert,
        reselect: path === appState.entry ? change.start : undefined,
      });
      persist();
      tree(true);
      setStatus(`Вынесен в ${change.componentPath}`);
      setCaption(`Создан компонент ${change.componentPath}`);
    } catch (e) {
      reportError(e);
      setCaption(e.message);
    }
  }

  const controlTree = createControlTree({
    explorer: root.querySelector('.explorer'),
    toolbar: root.querySelector('.preview-tools'),
    onSelect: selectTemplateControl,
    onScopeChange: refreshControlTree,
    onOpenTemplate: path => {
      open(path);
      controlTree.setView({ scope: 'file', visible: true });
    },
    onExtract: extractTreeControl,
    canReorder: () => appState.designPresetName === 'original',
    onReorder: reorderTreeControl,
  });

  const statesPanel = createStatesPanel({
    explorer: root.querySelector('.explorer'),
    onActivate: name => {
      if (name === null) {
        appState.scenario = -1;
        compile(true);
        return;
      }
      const index = (appState.compiled?.states ?? []).findIndex(state => state.name === name);
      if (index < 0) throw Error('Состояние не участвует в текущем предпросмотре');
      appState.scenario = index;
      compile(true);
    },
    onCreate: name => designStateEdit(source => stateCreateEdit(source, name)),
    onRename: (name, newName) => designStateEdit(source => stateRenameEdit(source, name, newName)),
    onDelete: name => designStateEdit(source => stateDeleteEdit(source, name)),
  });

  const projectSwitcher = $('project-switcher');
  const projectSwitcherMenu = $('project-switcher-menu');
  const projectSwitcherWrap = projectSwitcher?.parentElement;
  function closeProjectMenu() {
    if (!projectSwitcherMenu || projectSwitcherMenu.hidden) return;
    projectSwitcherMenu.hidden = true;
    projectSwitcher?.setAttribute('aria-expanded', 'false');
  }

  function bindEvents(codeEvents) {
    $('new').onclick = () => createInTree('file');
    if ($('new-folder')) $('new-folder').onclick = () => createInTree('folder');
    if ($('tree-locate')) $('tree-locate').onclick = () => locateActiveInTree();
    if ($('tree-collapse-all')) $('tree-collapse-all').onclick = () => tree(true);
    if (treeSearchToggle) treeSearchToggle.onclick = () => toggleTreeSearch();
    if (treeSearchInput) {
      treeSearchInput.oninput = () => {
        appState.treeFilter = treeSearchInput.value.trim();
        tree(true);
      };
      treeSearchInput.onkeydown = e => {
        if (e.key === 'Escape') {
          e.stopPropagation();
          toggleTreeSearch(false);
          $('tree').focus();
        }
      };
    }
    if (treeSearchClear) {
      treeSearchClear.onclick = () => {
        if (treeSearchInput) treeSearchInput.value = '';
        appState.treeFilter = '';
        tree(true);
        treeSearchInput?.focus();
      };
    }
    $('tree').ondragover = e => {
      if (!appState.draggedTreePath || e.target.closest('.file')) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      $('tree')
        .querySelectorAll('.drop-target')
        .forEach(el => el.classList.remove('drop-target'));
      $('tree').classList.add('drop-root');
    };
    $('tree').ondragleave = e => {
      if (e.target === $('tree')) $('tree').classList.remove('drop-root');
    };
    $('tree').ondrop = async e => {
      if (!appState.draggedTreePath || e.target.closest('.file')) return;
      e.preventDefault();
      $('tree').classList.remove('drop-root');
      await moveTreeItem(appState.draggedTreePath, '');
    };
    $('tree').oncontextmenu = e => {
      e.preventDefault();
      const btn = e.target.closest('.file');
      const itemPath = btn ? btn.dataset.path : null;
      if (itemPath) selectTreeRow(itemPath);
      const targetDir = itemPath ? parentDirOf(itemPath) : '';
      if (!treeContextMenu) {
        treeContextMenu = documentObj.createElement('div');
        treeContextMenu.className = 'tree-context-menu';
        treeContextMenu.hidden = true;
        documentObj.body.append(treeContextMenu);
      }
      treeContextMenu.innerHTML = `
  <button type="button" class="tree-context-item" data-cmd="new-file"><span>Новый файл…</span><span class="tree-context-kbd">N</span></button>
  <button type="button" class="tree-context-item" data-cmd="new-folder"><span>Новая папка…</span><span class="tree-context-kbd">⇧N</span></button>
  <div class="tree-context-sep"></div>
  <button type="button" class="tree-context-item" data-cmd="rename" ${itemPath ? '' : 'disabled'}><span>Переименовать</span><span class="tree-context-kbd">F2</span></button>
  <button type="button" class="tree-context-item" data-cmd="duplicate" ${itemPath ? '' : 'disabled'}><span>Дублировать</span><span class="tree-context-kbd">⌘D</span></button>
  <button type="button" class="tree-context-item" data-cmd="copy-path" ${itemPath ? '' : 'disabled'}><span>Скопировать путь</span><span class="tree-context-kbd">⌥⌘C</span></button>
  <div class="tree-context-sep"></div>
  <button type="button" class="tree-context-item" data-cmd="collapse-all"><span>Свернуть все папки</span></button>
  <div class="tree-context-sep"></div>
  <button type="button" class="tree-context-item danger" data-cmd="delete" ${itemPath ? '' : 'disabled'}><span>Удалить</span><span class="tree-context-kbd">⌫</span></button>
 `;
      treeContextMenu.hidden = false;
      const x = Math.min(e.clientX, windowObj.innerWidth - 220);
      const y = Math.min(e.clientY, windowObj.innerHeight - 260);
      treeContextMenu.style.left = `${x}px`;
      treeContextMenu.style.top = `${y}px`;
      treeContextMenu.querySelectorAll('[data-cmd]').forEach(el => {
        el.onclick = async () => {
          const cmd = el.dataset.cmd;
          closeTreeContextMenu();
          if (cmd === 'new-file') await createInTree('file', targetDir);
          else if (cmd === 'new-folder') await createInTree('folder', targetDir);
          else if (cmd === 'rename' && itemPath)
            startTreeInlineInput({ mode: 'rename', targetPath: itemPath });
          else if (cmd === 'duplicate' && itemPath) await duplicateTreeItem(itemPath);
          else if (cmd === 'copy-path' && itemPath) {
            try {
              await navigatorObj.clipboard.writeText(itemPath);
              setStatus(`Скопировано: ${itemPath}`);
            } catch {}
          } else if (cmd === 'collapse-all') {
            tree(true);
          } else if (cmd === 'delete' && itemPath) await deleteTreeItem(itemPath);
        };
      });
    };
    $('tree').onkeydown = async e => {
      if (e.target.matches('input,textarea')) return;
      const visibleRows = [...$('tree').querySelectorAll('.file')].filter(
        b => !b.hidden && b.offsetParent !== null,
      );
      if (!visibleRows.length) return;
      const idx = visibleRows.findIndex(b => b.dataset.path === appState.selectedTreeItem);
      const curBtn = idx >= 0 ? visibleRows[idx] : visibleRows[0];
      const curPath = curBtn ? curBtn.dataset.path : null;
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        const next = visibleRows[Math.min(visibleRows.length - 1, idx + 1)] ?? visibleRows[0];
        selectTreeRow(next.dataset.path);
        next.scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        const prev = visibleRows[Math.max(0, idx - 1)] ?? visibleRows[0];
        selectTreeRow(prev.dataset.path);
        prev.scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'ArrowLeft' && curPath) {
        e.preventDefault();
        const parent = parentDirOf(curPath);
        if (parent) selectTreeRow(parent);
      } else if (e.key === 'Enter' && curBtn?.dataset.path) {
        e.preventDefault();
        await activateTreeFile(curBtn.dataset.path);
      } else if (e.key === 'F2' && curPath) {
        e.preventDefault();
        startTreeInlineInput({ mode: 'rename', targetPath: curPath });
      } else if ((e.key === 'Delete' || e.key === 'Backspace') && curPath) {
        e.preventDefault();
        await deleteTreeItem(curPath);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        toggleTreeSearch(true);
      }
    };

    if (projectSwitcher && projectSwitcherMenu) {
      projectSwitcher.onclick = e => {
        e.stopPropagation();
        const openState = projectSwitcherMenu.hidden;
        projectSwitcherMenu.hidden = !openState;
        projectSwitcher.setAttribute('aria-expanded', String(openState));
      };
      $('project-switcher-open-folder').onclick = () => {
        closeProjectMenu();
        openFolder();
      };
    }

    codeEvents.listen(documentObj, 'pointerdown', e => {
      if (projectSwitcherWrap && !projectSwitcherWrap.contains(e.target)) closeProjectMenu();
      if (treeContextMenu && !treeContextMenu.contains(e.target)) closeTreeContextMenu();
    });
    codeEvents.listen(documentObj, 'keydown', e => {
      if (e.key === 'Escape') {
        closeProjectMenu();
        closeTreeContextMenu();
        return;
      }
      if (
        (e.metaKey || e.ctrlKey) &&
        !e.altKey &&
        !e.target.closest('.cm-editor,input,textarea,#canvas,#preview')
      ) {
        const isZ = e.code === 'KeyZ' || e.key.toLowerCase() === 'z' || e.key.toLowerCase() === 'я';
        const isY = e.code === 'KeyY' || e.key.toLowerCase() === 'y' || e.key.toLowerCase() === 'н';
        if (isZ && !e.shiftKey && treeHistory.canUndo) {
          e.preventDefault();
          treeHistory.undo();
        } else if (((isZ && e.shiftKey) || isY) && treeHistory.canRedo) {
          e.preventDefault();
          treeHistory.redo();
        }
      }
    });
  }

  function destroy() {
    treeContextMenu?.remove();
    controlTree?.destroy();
  }

  return {
    controlTree,
    statesPanel,
    tree,
    selectTreeRow,
    createInTree,
    extractTreeControl,
    refreshControlTree,
    refreshStatesPanel,
    designStateFile,
    designStateEdit,
    bindEvents,
    destroy,
  };
}
