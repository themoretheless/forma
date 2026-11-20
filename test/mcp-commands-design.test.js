// Behaviour of the MCP design commands against a stand-in for the Studio.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createDesignCommands } from '../src/app/mcp-commands-design.js';

const SOURCE = "component Demo { Frame { Text { text: 'a'; } Text { text: 'b'; } } }";
const FIRST = SOURCE.indexOf("Text { text: 'a'");
const SECOND = SOURCE.indexOf("Text { text: 'b'");

function studioWith(overrides = {}) {
  const calls = [];
  const element = id => ({
    id,
    value: '',
    click: () => calls.push(['click', id]),
    onchange: event => calls.push(['change', id, event.target.value]),
    classList: { toggle: (name, on) => calls.push(['class', id, name, on]) },
  });
  const elements = {};
  const tree = { rows: 2 };
  const studio = {
    $: id => (elements[id] ??= element(id)),
    files: { 'ui/Demo.ui': SOURCE },
    entry: 'ui/Demo.ui',
    error: null,
    mode: 'design',
    scenario: 0,
    compiled: { nodes: [{ type: 'Frame' }], states: [{ name: 'empty' }, { name: 'full' }] },
    liveSource: () => SOURCE,
    commitSource: change => calls.push(['commit', change]),
    compile: force => calls.push(['compile', force]),
    select: node => calls.push(['select', node.id]),
    propertyInput: property =>
      property === 'hidden'
        ? null
        : {
            set value(text) {
              calls.push(['input', property, text]);
            },
            onchange: () => calls.push(['input-change', property]),
          },
    controlTree: {
      snapshot: () => tree,
      setView: args => calls.push(['view', args]),
      selectId: id => calls.push(['tree-select', id]),
      fold: (id, collapsed) => calls.push(['tree-fold', id, collapsed]),
    },
    rendererPicker: { value: 'dom', onchange: () => calls.push(['renderer']) },
    vectorPreview: { snapshot: () => ({ ready: true }) },
    spacingOverlay: { read: () => ({ gaps: [] }) },
    designStateFile: () => ({ source: "design Demo { state 'empty' { } }" }),
    designStateEdit: (edit, expectedContent) => {
      calls.push(['state-edit', expectedContent]);
      return 'edited';
    },
    ...overrides,
  };
  const nodes = {
    [FIRST]: { id: 'first', props: { text: 'a', width: 10, hidden: 1, brush: { color: 'x' } } },
    [SECOND]: { id: 'second', props: { text: 'b' } },
  };
  const commands = createDesignCommands(studio, {
    snapshot: () => 'snapshot',
    requireFile: path => {
      if (!Object.hasOwn(studio.files, path)) throw Error(`No file ${path}`);
    },
    findNode: start => {
      if (!nodes[start]) throw Error(`No node at ${start}`);
      return nodes[start];
    },
  });
  return { studio, commands, calls };
}

test('tree and preview commands forward to the Studio and report its state', () => {
  const { commands, calls, studio } = studioWith();
  assert.deepEqual(commands.designer_tree(), { rows: 2 });
  commands.designer_tree_view({ search: 'x' });
  commands.designer_tree_select({ id: 7 });
  commands.designer_tree_fold({ id: 7, collapsed: true });
  assert.equal(commands.preview_renderer({ renderer: 'vector' }), 'snapshot');
  assert.equal(studio.rendererPicker.value, 'vector');
  assert.deepEqual(commands.vector_status(), { ready: true });
  assert.deepEqual(studioWith({ vectorPreview: null }).commands.vector_status(), { ready: false });
  assert.deepEqual(commands.component_spacing(), { gaps: [] });
  assert.deepEqual(commands.component_tree(), {
    entry: 'ui/Demo.ui',
    nodes: [{ type: 'Frame' }],
    error: null,
  });
  assert.deepEqual(calls, [
    ['view', { search: 'x' }],
    ['tree-select', 7],
    ['tree-fold', 7, true],
    ['renderer'],
  ]);
});

test('preview_open and preview_scenario switch what is shown', () => {
  const { commands, calls, studio } = studioWith({ scenario: 1 });
  studio.files['ui/Other.ui'] = SOURCE;
  assert.equal(commands.preview_open({ path: 'ui/Other.ui' }), 'snapshot');
  assert.equal(studio.entry, 'ui/Other.ui');
  assert.equal(studio.scenario, 0);
  assert.throws(() => commands.preview_open({ path: 'ui/None.ui' }), /No file/);

  assert.equal(commands.preview_scenario({ name: 'full' }), 'snapshot');
  assert.equal(studio.scenario, 1);
  assert.equal(commands.preview_scenario({ name: null }), 'snapshot');
  assert.throws(() => commands.preview_scenario({ name: 'missing' }), /Design state not found/);
  assert.throws(
    () =>
      studioWith({ compiled: { nodes: [], states: [] } }).commands.preview_scenario({ name: null }),
    /has no design states/,
  );
  assert.deepEqual(calls, [
    ['compile', true],
    ['change', 'scenario', 1],
    ['change', 'scenario', -1],
  ]);
});

test('preview_mode and preview_viewport press the Studio controls', () => {
  const { commands, calls } = studioWith();
  commands.preview_mode({ mode: 'design' });
  commands.preview_mode({ mode: 'interact' });
  commands.preview_viewport({ device: 'phone', theme: 'light' });
  assert.deepEqual(calls, [
    ['click', 'run'],
    ['click', 'phone'],
    ['class', 'preview', 'light', true],
  ]);
});

test('design state commands edit the file the states panel authors', () => {
  const { commands, calls } = studioWith();
  assert.equal(typeof commands.design_states_read(), 'object');
  assert.equal(commands.design_state_create({ name: 'n', expectedContent: 'c1' }), 'edited');
  assert.equal(
    commands.design_state_rename({ name: 'n', newName: 'm', expectedContent: 'c2' }),
    'edited',
  );
  assert.equal(commands.design_state_delete({ name: 'm', expectedContent: 'c3' }), 'edited');
  assert.equal(
    commands.design_state_property({
      type: 'Text',
      key: 't',
      property: 'text',
      value: 'x',
      expectedContent: 'c4',
    }),
    'edited',
  );
  assert.deepEqual(calls, [
    ['state-edit', 'c1'],
    ['state-edit', 'c2'],
    ['state-edit', 'c3'],
    ['state-edit', 'c4'],
  ]);
});

test('component_select and component_property act on the compiled node', () => {
  const { commands, calls } = studioWith();
  assert.equal(commands.component_select({ start: FIRST }), 'snapshot');
  assert.equal(
    commands.component_property({
      start: FIRST,
      property: 'text',
      value: 'z',
      expectedContent: SOURCE,
    }),
    'snapshot',
  );
  assert.deepEqual(calls, [
    ['select', 'first'],
    ['select', 'first'],
    ['input', 'text', 'z'],
    ['input-change', 'text'],
  ]);

  const property = (name, value, expectedContent = SOURCE) =>
    commands.component_property({ start: FIRST, property: name, value, expectedContent });
  assert.throws(() => property('text', 'z', 'stale'), /Stale or invalid source/);
  assert.throws(() => property('missing', 'z'), /Only existing literal properties/);
  assert.throws(() => property('brush', 'z'), /Only existing literal properties/);
  assert.throws(() => property('width', 'wide'), /Property type mismatch/);
  assert.throws(() => property('hidden', 2), /Свойство недоступно для правки/);
  assert.throws(
    () => studioWith({ error: 'bad' }).commands.component_select({ start: FIRST }),
    /Fix diagnostics/,
  );
});

test('splice commands change the entry source and report the new node', () => {
  const { commands, calls } = studioWith();
  const stale = { start: FIRST, expectedContent: 'stale', markup: 'Text { }' };
  for (const command of [
    'component_insert',
    'component_move',
    'component_delete',
    'component_duplicate',
  ])
    assert.throws(() => commands[command](stale), /Stale or invalid source/);
  assert.throws(
    () => commands.component_delete({ start: 3, expectedContent: SOURCE }),
    /No node at 3/,
  );
  assert.deepEqual(calls, []);

  const inserted = commands.component_insert({
    start: FIRST,
    markup: "Text { text: 'new'; }",
    side: 'after',
    expectedContent: SOURCE,
  });
  assert.equal(inserted.changed, true);
  assert.match(inserted.content, /text: 'a'; \}\s*Text \{ text: 'new'; \}\s*Text \{ text: 'b'/);
  assert.equal(inserted.content.slice(inserted.start).startsWith("Text { text: 'new'"), true);

  const removed = commands.component_delete({ start: FIRST, expectedContent: SOURCE });
  assert.equal(removed.changed, true);
  assert.doesNotMatch(removed.content, /text: 'a'/);
  assert.match(removed.content, /text: 'b'/);

  const duplicated = commands.component_duplicate({ start: SECOND, expectedContent: SOURCE });
  assert.equal(duplicated.content.match(/text: 'b'/g).length, 2);

  const moved = commands.component_move({
    start: FIRST,
    target: SECOND,
    side: 'after',
    expectedContent: SOURCE,
  });
  assert.equal(moved.changed, true);
  assert.ok(moved.content.indexOf("text: 'b'") < moved.content.indexOf("text: 'a'"));

  // A move onto the slot the node already holds changes nothing and commits nothing.
  const before = calls.length;
  const still = commands.component_move({
    start: FIRST,
    target: SECOND,
    side: 'before',
    expectedContent: SOURCE,
  });
  assert.deepEqual(still, { start: FIRST, content: SOURCE, changed: false });
  assert.equal(calls.length, before);

  assert.deepEqual(
    calls.map(([name, change]) => [name, change.file, change.source === SOURCE]),
    Array(4).fill(['commit', 'ui/Demo.ui', true]),
  );
  assert.ok(Array.isArray(commands.component_controls()));
});

test('component_select and component_property resolve nodes by stableId across shifted offsets', async () => {
  const { createIdeCommands } = await import('../src/app/mcp-commands.js');
  const { parse } = await import('../src/compiler/language.js');
  const shifted =
    "component Demo { Frame { Button { text: 'Inserted'; } Text { text: 'a'; } Text { text: 'b'; } } }";
  const doc = parse(shifted, { file: 'ui/Demo.ui' });
  const textA = doc.nodes[0].children[1];
  const selected = [];
  const ide = createIdeCommands({
    $: () => ({}),
    files: { 'ui/Demo.ui': shifted },
    active: 'ui/Demo.ui',
    entry: 'ui/Demo.ui',
    scenario: -1,
    mode: 'design',
    error: null,
    selected: null,
    pending: null,
    breakOn: false,
    renderer: 'dom',
    compiled: doc,
    select: node => selected.push(node),
    open() {},
    persist() {},
    compile() {},
    propertyInput: () => ({ value: '', onchange() {} }),
  });
  ide('component_select', { stableId: textA.stableId });
  assert.equal(selected[0], textA);
  ide('component_property', {
    stableId: textA.stableId,
    property: 'text',
    value: 'updated',
    expectedContent: shifted,
  });
  assert.equal(selected.length, 2);
});
