// @ts-check
import {
  readDirectoryProject,
  validateProject,
  writeDirectoryProject,
  writeProject,
} from './browser-storage.js';
import { createProjectStore } from './project-store.js';
import { buildNativeRustExport } from './native-export.js';

function downloadJson(name, text, doc = document) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const a = doc.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Coordinates project persistence, import/export, local directory sync, and native Rust export. */
export function createWorkspaceService({
  $,
  appState,
  onResetEditing,
  onOpenFile,
  onRenderTree,
  onCompile,
  onProjectNameChange,
  reportError,
  saveStatus,
  setStatus,
  windowObj = window,
  documentObj = document,
}) {
  const projectStore = createProjectStore({
    key: appState.storageKey('forma-project'),
    writeProject,
    writeDirectoryProject,
    status: saveStatus,
  });

  if (appState.recoveryPayload !== null) {
    const button = documentObj.createElement('button');
    button.type = 'button';
    button.className = 'project-switcher-item project-switcher-action';
    button.id = 'download-recovery';
    button.title = 'Исходная запись проекта, которую не удалось прочитать';
    button.innerHTML =
      '<span class="project-switcher-item-main"><span class="project-switcher-item-name">Скачать данные восстановления</span><small class="project-switcher-item-sub">Непрочитанная запись проекта JSON</small></span>';
    button.onclick = () =>
      downloadJson('forma-project-recovery.json', appState.recoveryPayload, documentObj);
    $('project-switcher-menu')?.append(button);
  }
  if (appState.damagedProject !== null) {
    saveStatus('damaged', 'Сохранённый проект повреждён: открыта исходная версия');
  }

  function persist() {
    projectStore.save(appState.files);
  }

  function applyLoadedProject(data, name) {
    appState.files = data;
    if (name) {
      appState.projectName = name;
      onProjectNameChange?.(name);
    }
    appState.active = Object.keys(data)[0];
    appState.entry = Object.keys(data).find(p => p.endsWith('.ui') && !p.endsWith('.design.ui'));
    appState.scenario = 0;
    onOpenFile(appState.active);
    persist();
    onCompile(true);
  }

  async function openFolder() {
    if (typeof windowObj.showDirectoryPicker !== 'function') {
      windowObj.alert(
        'Браузер не поддерживает File System Access API (showDirectoryPicker). Используйте Открыть / Экспорт JSON.',
      );
      return;
    }
    try {
      const handle = await windowObj.showDirectoryPicker({ mode: 'readwrite' });
      const data = await readDirectoryProject(handle);
      onResetEditing();
      projectStore.attach(handle, data);
      applyLoadedProject(data, handle.name);
    } catch (e) {
      if (e?.name !== 'AbortError') windowObj.alert(e.message);
    }
  }

  async function exportNative() {
    try {
      const bundle = buildNativeRustExport(appState.files, appState.entry, {
        name: appState.isStudioControls ? 'forma-studio-controls' : 'forma-workspace-app',
      });
      const folder = projectStore.directory;
      if (folder) {
        await projectStore.writeToDirectory(bundle.files);
        appState.files = { ...bundle.files };
        onRenderTree(false);
        saveStatus('saved', `Rust-крейт записан (${folder.name})`);
      } else {
        downloadJson(
          `${bundle.name}-rust-crate.json`,
          JSON.stringify(bundle.files, null, 2),
          documentObj,
        );
        setStatus(`Экспортирован нативный Rust-проект (${bundle.entry})`);
      }
    } catch (e) {
      reportError(e);
    }
  }

  function bindUpload() {
    $('upload').onchange = async e => {
      try {
        const f = e.target.files[0];
        if (!f) return;
        const data = validateProject(JSON.parse(await f.text()));
        if (
          !windowObj.confirm(
            'Заменить текущий проект? При необходимости сначала экспортируйте его.',
          )
        )
          return;
        onResetEditing();
        projectStore.detach();
        applyLoadedProject(data, null);
      } catch (err) {
        windowObj.alert(err.message);
      } finally {
        e.target.value = '';
      }
    };
  }

  return {
    projectStore,
    persist,
    openFolder,
    promptImport: () => $('upload').click(),
    exportProject: () =>
      downloadJson('forma-project.json', JSON.stringify(appState.files, null, 2), documentObj),
    downloadRecovery: () => {
      if (appState.recoveryPayload !== null) {
        downloadJson('forma-project-recovery.json', appState.recoveryPayload, documentObj);
      }
    },
    exportNative,
    bindUpload,
  };
}
