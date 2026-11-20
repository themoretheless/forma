import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createStudioState } from '../src/app/state.js';

function memoryHost(seed = {}) {
  const local = new Map(Object.entries(seed.local ?? {}));
  const session = new Map(Object.entries(seed.session ?? {}));
  return {
    localStorage: {
      getItem: k => (local.has(k) ? local.get(k) : null),
      setItem: (k, v) => local.set(k, String(v)),
    },
    sessionStorage: {
      getItem: k => (session.has(k) ? session.get(k) : null),
      setItem: (k, v) => session.set(k, String(v)),
    },
  };
}

test('createStudioState restores valid saved views, isolates studio-controls keys, and seeds Rust files', () => {
  const host = memoryHost({
    local: {
      'forma-backend': 'svet',
      'forma-platform': 'ios',
      'forma-renderer': 'vector',
      'forma-view': JSON.stringify({ active: 'Cargo.toml', entry: 'ui/SearchWindow.ui' }),
    },
    session: {
      'forma-canvas-mode': 'interact',
    },
  });
  const state = createStudioState({ search: '', host });
  assert.equal(state.projectId, 'knowledge-workspace');
  assert.equal(state.backend, 'svet');
  assert.equal(state.platform, 'ios');
  assert.equal(state.renderer, 'vector');
  assert.equal(state.mode, 'interact');
  assert.equal(state.active, 'Cargo.toml');
  assert.equal(state.entry, 'ui/SearchWindow.ui');
  assert.ok(state.files['src/main.rs'].includes('fn main()'));
  assert.equal(
    state.liveSource('Cargo.toml', () => 'LIVE'),
    'LIVE',
  );
  assert.equal(
    state.liveSource('src/main.rs', () => 'LIVE'),
    state.files['src/main.rs'],
  );

  const guide = createStudioState({
    search: '?project=studio-controls',
    host: memoryHost(),
    getInitialProject: isGuide =>
      isGuide
        ? { 'ui/StudioControls.ui': 'component StudioControls { Frame { Text { text: "ok"; } } }' }
        : {},
  });
  assert.equal(guide.isStudioControls, true);
  assert.equal(guide.storageKey('forma-project'), 'forma-project:studio-controls-guide');
  assert.equal(guide.renderer, 'vector');
  assert.ok(guide.files['ui/StudioControls.ui']);
});

test('createStudioState flags corrupted stored project JSON while preserving recovery payload', () => {
  const host = memoryHost({ local: { 'forma-project': '{not-valid-json' } });
  const state = createStudioState({ search: '', host });
  assert.equal(state.damagedProject, '{not-valid-json');
  assert.equal(state.recoveryPayload, '{not-valid-json');
  assert.ok(state.files['ui/SearchWindow.ui']);
});
