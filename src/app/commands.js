// @ts-check
/** Creates an observable registry of Studio UI and palette commands. */
export function createCommandRegistry() {
  const entries = new Map();
  const listeners = new Set();
  const notify = () => {
    for (const listener of listeners) listener();
  };

  return {
    register(id, spec) {
      if (!id || typeof spec?.run !== 'function') {
        throw Error('Command registration requires id and run()');
      }
      entries.set(id, { id, ...spec });
      notify();
      return () => {
        entries.delete(id);
        notify();
      };
    },
    get(id) {
      return entries.get(id) ?? null;
    },
    execute(id, args) {
      const cmd = entries.get(id);
      if (!cmd) throw Error(`Unknown Studio command: ${id}`);
      if (cmd.enabled && !cmd.enabled()) return false;
      return cmd.run(args);
    },
    list() {
      return [...entries.values()].filter(cmd => !cmd.enabled || cmd.enabled());
    },
    menuItems() {
      return this.list().filter(cmd => cmd.menu);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

/** Registers core Studio workspace, preview, platform, and target actions. */
export function registerStudioCommands(
  registry,
  {
    state,
    workspace,
    tree,
    editor,
    preview,
    onNavigate = href => {
      globalThis.location.href = href;
    },
  },
) {
  const switchTarget = state.isStudioControls
    ? { href: '/?project=knowledge-workspace', label: 'Библиотека знаний' }
    : { href: '/?project=studio-controls', label: 'Контролы Studio' };

  registry.register('switch-project', {
    label: switchTarget.label,
    menu: true,
    run: () => onNavigate(switchTarget.href),
  });
  registry.register('open-controls-gallery', {
    label: 'Контролы · Reveal',
    menu: true,
    run: () => onNavigate('/crates/forma/examples/controls.html'),
  });
  registry.register('open-folder', {
    label: 'Папка',
    menu: true,
    run: () => workspace.openFolder(),
  });
  registry.register('import-project', {
    label: 'Открыть',
    menu: true,
    run: () => workspace.promptImport(),
  });
  registry.register('export-project', {
    label: 'Экспорт',
    menu: true,
    run: () => workspace.exportProject(),
  });
  registry.register('export-native', {
    label: 'Экспорт Rust',
    menu: true,
    run: () => workspace.exportNative(),
  });
  if (state.recoveryPayload !== null) {
    registry.register('download-recovery', {
      label: 'Скачать данные восстановления',
      menu: true,
      run: () => workspace.downloadRecovery(),
    });
  }
  registry.register('format-code', {
    label: 'Формат',
    key: '⇧⌥F',
    enabled: () => Boolean(state.active?.endsWith('.ui')),
    run: () => editor.formatActiveFile(),
  });
  registry.register('show-entry', {
    label: 'Показать UI',
    enabled: () => Boolean(state.active?.endsWith('.ui') && !state.active?.endsWith('.design.ui')),
    run: () => preview.activateCurrentAsEntry(),
  });
  registry.register('new-file', {
    label: 'Новый файл',
    run: () => tree.createInTree('file'),
  });
  registry.register('new-folder', {
    label: 'Новая папка',
    run: () => tree.createInTree('folder'),
  });
  for (const [value, label] of [
    ['desktop', 'Платформа: Desktop'],
    ['ios', 'Платформа: iOS'],
    ['android', 'Платформа: Android'],
  ]) {
    registry.register(`set-platform:${value}`, {
      label: `Целевая платформа: ${label}`,
      run: () => preview.setPlatform(value),
    });
  }
  for (const [value, label] of [
    ['ask', 'Бэкенд: выбор (egui / svet)'],
    ['egui', 'Бэкенд: egui'],
    ['svet', 'Бэкенд: svet'],
  ]) {
    registry.register(`set-backend:${value}`, {
      label: `Бэкенд запуска: ${label}`,
      run: () => preview.setBackend(value),
    });
  }
  for (const [value, label] of [
    ['html', 'HTML · прежний'],
    ['vector', 'Вектор · Rust/WASM'],
  ]) {
    registry.register(`set-renderer:${value}`, {
      label: `Рендерер предпросмотра: ${label}`,
      run: () => preview.setRenderer(value),
    });
  }
}
