// Behaviour of the MCP runtime commands against a stand-in for the Studio.
//
// The commands only read and call the `studio` object they are given, so a recording stand-in is
// enough to pin what each one sends to the dev server, changes and refuses.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRuntimeCommands } from '../src/app/mcp-commands-runtime.js';

const PLAIN = 'component Demo { Frame { Text { text: "a"; } } }';
const FORM = 'component Demo { contextType: DemoContext; Frame { Text { text: "a"; } } }';

/** A Studio with one file, recording everything the commands do to it. */
function studioWith(overrides = {}) {
  const sent = [];
  const calls = [];
  const elements = {
    break: { checked: false, onchange: event => calls.push(['break', event.target.checked]) },
    continue: { click: () => calls.push(['continue']) },
    reset: { click: () => calls.push(['reset']) },
    preview: { id: 'preview' },
  };
  const studio = {
    $: id => elements[id],
    hot: { send: (name, payload) => sent.push([name, payload]) },
    files: { 'ui/Demo.ui': PLAIN },
    entry: 'ui/Demo.ui',
    compiled: { nodes: [{ type: 'Frame' }], states: [{ name: 'empty' }] },
    state: { count: 1, title: 'a' },
    error: null,
    renderer: 'dom',
    mode: 'interact',
    scenario: -1,
    logs: ['first'],
    pending: null,
    render: (...args) => calls.push(['render', ...args]),
    output: () => calls.push(['output']),
    dispatch: (action, node) => calls.push(['dispatch', action, node.id]),
    compileComponents: (files, entry, state, options) => ({
      source: `compiled ${entry}`,
      designState: options.designState,
    }),
    vectorPreview: { measureText: () => [1, 1], activate: () => calls.push(['activate']) },
    ...overrides,
  };
  const nodes = {
    button: { id: 'button', events: { clicked: 'actions.go' }, props: { disabled: false } },
    label: { id: 'label', events: {}, props: {} },
    off: { id: 'off', events: { clicked: 'actions.go' }, props: { disabled: true } },
  };
  const commands = createRuntimeCommands(studio, {
    snapshot: () => 'snapshot',
    findNode: start => nodes[start],
  });
  return { studio, commands, sent, calls };
}

test('app_run asks the dev server for the run that matches the project and renderer', () => {
  const dom = studioWith();
  assert.deepEqual(dom.commands.app_run({}), {
    status: 'requested',
    renderer: 'vector',
    backend: 'ask',
    platform: 'desktop',
  });
  assert.deepEqual(dom.sent, [
    ['forma:vector-run', { source: 'compiled ui/Demo.ui', designState: null }],
  ]);
  assert.equal(dom.studio.nativeSource.entry, 'ui/Demo.ui');
  assert.deepEqual(dom.calls, []);

  const form = studioWith({ files: { 'ui/Demo.ui': FORM }, backend: 'metal' });
  assert.deepEqual(form.commands.app_run({ platform: 'ios' }), {
    status: 'requested',
    renderer: 'generated-rust',
    backend: 'metal',
    platform: 'ios',
  });
  assert.deepEqual(form.sent, [['forma:form-run', { files: form.studio.files }]]);

  const vector = studioWith({ renderer: 'vector', scenario: 0 });
  assert.deepEqual(vector.commands.app_run({ backend: 'cpu' }), {
    status: 'requested',
    renderer: 'vector',
    backend: 'cpu',
    platform: 'desktop',
  });
  assert.deepEqual(vector.sent, [
    ['forma:vector-run', { source: 'compiled ui/Demo.ui', designState: 'empty', backend: 'cpu' }],
  ]);
  assert.equal(vector.studio.nativeSource.entry, 'ui/Demo.ui');
});

test('app_run refuses a project that does not compile', () => {
  assert.throws(() => studioWith({ error: 'bad' }).commands.app_run({}), /Исправьте разметку/);
  assert.throws(() => studioWith({ compiled: null }).commands.app_run({}), /Исправьте разметку/);
  assert.throws(() => studioWith({ renderer: 'vector', error: 'bad' }).commands.app_run({}), /bad/);
  assert.throws(
    () => studioWith({ renderer: 'vector', vectorPreview: null }).commands.app_run({}),
    /Векторный renderer не готов/,
  );
});

test('stop and Rust run commands only send requests', () => {
  const { commands, sent, studio } = studioWith();
  assert.deepEqual(commands.app_stop(), { status: 'requested' });
  assert.deepEqual(
    sent.map(([name]) => name),
    ['forma:rust-stop', 'forma:vector-stop'],
  );
  sent.length = 0;
  assert.equal(commands.rust_run().status, 'requested');
  assert.deepEqual(sent, [['forma:rust-run', { files: studio.files }]]);
  assert.deepEqual(commands.rust_stop(), { status: 'stop requested' });

  const offline = studioWith({ hot: null });
  assert.throws(() => offline.commands.rust_run(), /Requires local dev server/);
  assert.deepEqual(offline.commands.rust_stop(), { status: 'stop requested' });
});

test('state_set changes only existing fields of the same type', () => {
  const { commands, studio, calls } = studioWith();
  const copy = commands.state_read();
  copy.count = 99;
  assert.equal(studio.state.count, 1, 'state_read returns a copy');

  assert.equal(commands.state_set({ values: { count: 2 } }), 'snapshot');
  assert.deepEqual(studio.state, { count: 2, title: 'a' });
  assert.deepEqual(calls, [['render'], ['output']]);

  assert.throws(() => commands.state_set({ values: { missing: 1 } }), /Unknown field/);
  assert.throws(() => commands.state_set({ values: { count: 'two' } }), /incompatible type/);
  // One bad value refuses the whole update.
  assert.throws(() => commands.state_set({ values: { title: 'b', count: 'two' } }));
  assert.deepEqual(studio.state, { count: 2, title: 'a' });
});

test('event_dispatch clicks an enabled control and explains every refusal', () => {
  const dom = studioWith();
  assert.equal(dom.commands.event_dispatch({ start: 'button' }), 'snapshot');
  assert.deepEqual(dom.calls, [['dispatch', 'actions.go', 'button']]);
  assert.throws(() => dom.commands.event_dispatch({ start: 'label' }), /No clicked handler/);
  assert.throws(() => dom.commands.event_dispatch({ start: 'off' }), /Component is disabled/);
  assert.throws(
    () => studioWith({ error: 'bad' }).commands.event_dispatch({ start: 'button' }),
    /Fix diagnostics first/,
  );

  const vector = studioWith({ renderer: 'vector' });
  assert.equal(vector.commands.event_dispatch({ start: 'button' }), 'snapshot');
  assert.deepEqual(vector.calls, [['activate']]);
  assert.throws(
    () => vector.commands.event_dispatch({ start: 'label' }),
    /Выберите кнопку с clicked/,
  );
  assert.throws(
    () =>
      studioWith({ renderer: 'vector', mode: 'design' }).commands.event_dispatch({
        start: 'button',
      }),
    /Включите взаимодействие/,
  );
  assert.throws(
    () =>
      studioWith({ renderer: 'vector', error: 'bad' }).commands.event_dispatch({ start: 'button' }),
    /bad/,
  );
});

test('debugger and log commands drive the Studio controls', () => {
  const { commands, calls, studio } = studioWith();
  assert.equal(commands.debug_break({ enabled: true }), 'snapshot');
  assert.equal(studio.$('break').checked, true);
  assert.throws(() => commands.debug_continue(), /Debugger is not paused/);
  studio.pending = {};
  assert.equal(commands.debug_continue(), 'snapshot');
  assert.equal(commands.debug_reset(), 'snapshot');
  assert.deepEqual(calls, [['break', true], ['continue'], ['reset']]);

  const logs = commands.events_read();
  logs.push('mine');
  assert.deepEqual(studio.logs, ['first'], 'events_read returns a copy');
  assert.equal(commands.events_clear(), 'snapshot');
  assert.deepEqual(studio.logs, []);
  assert.deepEqual(commands.diagnostics_read(), { error: null, entry: 'ui/Demo.ui' });
});
