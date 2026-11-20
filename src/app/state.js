// @ts-check
import { DEFAULT_BACKEND, DEFAULT_PLATFORM, isBackend, isPlatform } from '../../protocol/enums.js';
import {
  readStorage,
  restoreProject,
  validateProject,
  writeStorage,
} from '../workspace/browser-storage.js';
import { sampleProject } from '../workspace/sample-project.js';

export const OPEN_PROJECTS = [
  {
    id: 'studio-controls',
    name: 'Studio Controls',
    subtitle: 'Контролы Studio · дизайн-система',
    href: '/?project=studio-controls',
  },
  {
    id: 'knowledge-workspace',
    name: 'knowledge-workspace',
    subtitle: 'Библиотека знаний · пример проекта',
    href: '/?project=knowledge-workspace',
  },
];

const DEFAULT_CARGO_TOML = `[package]\nname = "forma-preview-app"\nversion = "0.1.0"\nedition = "2021"\n`;
const DEFAULT_MAIN_RS = `mod actions;\n\nfn main() {\n    let mut state = actions::SearchState {\n        query: String::from("Архитектура"),\n        loading: false,\n        status: String::from("Готов"),\n    };\n    println!("Rust-приложение запущено");\n    println!("Запрос: {}", state.query);\n    actions::search(&mut state);\n    println!("После actions::search: loading={}, status={}", state.loading, state.status);\n}\n`;

/**
 * Creates the centralized Studio workspace & preview session state store.
 * @param {{ search?: string, host?: any, getInitialProject?: (isStudioControls: boolean) => Record<string, string> }} [options]
 */
export function createStudioState({
  search = globalThis.location?.search ?? '',
  host = globalThis,
  getInitialProject = () => sampleProject,
} = {}) {
  const requestedProject = new URLSearchParams(search).get('project');
  const isStudioControls = requestedProject === 'studio-controls';
  const storageKey = name => (isStudioControls ? `${name}:studio-controls-guide` : name);
  const initialProject = getInitialProject(isStudioControls);

  const projectStorageKey = storageKey('forma-project');
  const rawStoredProject = readStorage('localStorage', projectStorageKey, host);
  let damagedProject = null;
  if (rawStoredProject !== null) {
    try {
      validateProject(JSON.parse(rawStoredProject));
    } catch {
      damagedProject = rawStoredProject;
    }
  }
  const recoveryPayload =
    damagedProject ?? readStorage('localStorage', `${projectStorageKey}:recovery`, host);

  const files = restoreProject(rawStoredProject, initialProject);
  for (const [k, v] of Object.entries(initialProject)) files[k] ??= v;
  files['Cargo.toml'] ??= DEFAULT_CARGO_TOML;
  files['src/main.rs'] ??= DEFAULT_MAIN_RS;

  const storedBackend = readStorage('localStorage', storageKey('forma-backend'), host);
  const storedPlatform = readStorage('localStorage', storageKey('forma-platform'), host);
  const storedMode = readStorage('sessionStorage', storageKey('forma-canvas-mode'), host);
  const storedRenderer = readStorage('localStorage', storageKey('forma-renderer'), host);

  let active = Object.keys(files)[0];
  let entry = Object.keys(files).find(p => p.endsWith('.ui') && !p.endsWith('.design.ui'));
  try {
    const view = JSON.parse(readStorage('localStorage', storageKey('forma-view'), host));
    if (view?.entry in files) entry = view.entry;
    if (view?.active in files) active = view.active;
  } catch {}

  return {
    isStudioControls,
    projectId: isStudioControls ? 'studio-controls' : 'knowledge-workspace',
    projectName: isStudioControls ? 'Studio Controls' : 'knowledge-workspace',
    openProjects: OPEN_PROJECTS,
    storageKey,
    damagedProject,
    recoveryPayload,
    files,
    active,
    entry,
    scenario: 0,
    state: {},
    compiled: null,
    designFiles: [],
    activeTreeDocument: null,
    selected: null,
    selectedPath: null,
    selectedTreeItem: active,
    treeFilter: '',
    draggedTreePath: null,
    mode: storedMode === 'interact' ? 'interact' : 'design',
    renderer: isStudioControls || storedRenderer === 'vector' ? 'vector' : 'html',
    designPresetName: 'original',
    backend: isBackend(storedBackend) ? storedBackend : DEFAULT_BACKEND,
    platform: isPlatform(storedPlatform) ? storedPlatform : DEFAULT_PLATFORM,
    logs: [],
    lintWarnings: [],
    tab: 'problems',
    error: '',
    sourceDiagnostic: null,
    nativeSource: null,
    lastVisuals: [],
    lastPreviewControls: [],
    inspectorNote: '',
    saveTimer: undefined,
    compileTimer: undefined,
    shellDisposed: false,
    persistView() {
      writeStorage(
        'localStorage',
        storageKey('forma-view'),
        JSON.stringify({ active: this.active, entry: this.entry }),
      );
    },
    liveSource(path, readOpenCode) {
      return path === this.active ? readOpenCode() : this.files[path];
    },
  };
}
