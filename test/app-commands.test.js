import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createCommandRegistry, registerStudioCommands } from '../src/app/commands.js';

test('command registry filters by enabled(), separates menu items, and notifies listeners', () => {
  const reg = createCommandRegistry();
  let notifications = 0;
  let allowFormat = false;
  const unsub = reg.subscribe(() => notifications++);
  const disposeFormat = reg.register('format', {
    label: 'Формат',
    enabled: () => allowFormat,
    run: () => 'formatted',
  });
  reg.register('export', { label: 'Экспорт', menu: true, run: v => `exported:${v}` });
  assert.equal(notifications, 2);
  assert.equal(reg.list().length, 1);
  assert.equal(
    reg
      .menuItems()
      .map(c => c.id)
      .join(','),
    'export',
  );
  assert.equal(reg.execute('format'), false);
  allowFormat = true;
  assert.equal(reg.list().length, 2);
  assert.equal(reg.execute('format'), 'formatted');
  assert.equal(reg.execute('export', 'json'), 'exported:json');
  assert.throws(() => reg.execute('missing'), /Unknown Studio command/);
  disposeFormat();
  unsub();
  assert.equal(notifications, 3);
  assert.equal(reg.get('format'), null);
});

test('registerStudioCommands exposes menu and runtime commands without relying on a hidden header DOM', () => {
  const reg = createCommandRegistry();
  const called = [];
  registerStudioCommands(reg, {
    state: { isStudioControls: false, active: 'ui/App.ui', recoveryPayload: 'bad-json' },
    workspace: {
      openFolder: () => called.push('openFolder'),
      promptImport: () => called.push('import'),
      exportProject: () => called.push('export'),
      exportNative: () => called.push('exportNative'),
      downloadRecovery: () => called.push('recovery'),
    },
    tree: { createInTree: kind => called.push(`new:${kind}`) },
    editor: { formatActiveFile: () => called.push('format') },
    preview: {
      activateCurrentAsEntry: () => called.push('entry'),
      setPlatform: p => called.push(`platform:${p}`),
      setBackend: b => called.push(`backend:${b}`),
      setRenderer: r => called.push(`renderer:${r}`),
    },
    onNavigate: href => called.push(`nav:${href}`),
  });
  for (const id of [
    'switch-project',
    'open-folder',
    'import-project',
    'export-project',
    'export-native',
    'download-recovery',
    'format-code',
    'show-entry',
    'new-file',
    'set-platform:ios',
    'set-backend:svet',
    'set-renderer:vector',
  ]) {
    reg.execute(id);
  }
  assert.deepEqual(called, [
    'nav:/?project=studio-controls',
    'openFolder',
    'import',
    'export',
    'exportNative',
    'recovery',
    'format',
    'entry',
    'new:file',
    'platform:ios',
    'backend:svet',
    'renderer:vector',
  ]);
});
