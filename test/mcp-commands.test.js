import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { MCP_TOOLS, MCP_TOOL_NAMES } from '../protocol/mcp-tools.js';
import { createIdeCommands } from '../src/mcp-commands.js';
import { squash } from './support/source-text.js';

const root = new URL('..', import.meta.url);
const read = path => readFileSync(new URL(path, root), 'utf8');

test('the shared protocol table, MCP server, and IDE bridge name the exact same commands', () => {
  const ide = createIdeCommands({});
  const commands = ide.commandNames();
  assert.ok(MCP_TOOL_NAMES.length >= 45, 'expected the full MCP tool catalog');
  assert.equal(new Set(MCP_TOOL_NAMES).size, MCP_TOOL_NAMES.length, 'a tool is registered twice');
  assert.deepEqual(
    MCP_TOOL_NAMES.filter(name => !commands.includes(name)),
    [],
    'registered with no handler',
  );
  assert.deepEqual(
    commands.filter(name => !MCP_TOOL_NAMES.includes(name)),
    [],
    'handled with no tool',
  );
  for (const tool of MCP_TOOLS) {
    assert.ok(tool.description.length > 0, `missing description on ${tool.name}`);
    assert.equal(typeof tool.readOnly, 'boolean', `missing readOnly flag on ${tool.name}`);
    assert.ok(
      ['project', 'design', 'runtime'].includes(tool.domain),
      `unexpected domain ${tool.domain} on ${tool.name}`,
    );
  }
  const serverSource = read('mcp/server.js');
  assert.match(
    serverSource,
    /import\s*\{\s*MCP_TOOLS\s*\}\s*from\s*'\.\.\/protocol\/mcp-tools\.js'/,
  );
  assert.match(serverSource, /for\s*\(\s*const\s+tool\s+of\s+MCP_TOOLS\s*\)/);
});

test('every design state an agent can reach has a tool of its own', () => {
  assert.deepEqual(
    MCP_TOOL_NAMES.filter(name => name.startsWith('design_state')),
    [
      'design_states_read',
      'design_state_create',
      'design_state_rename',
      'design_state_delete',
      'design_state_property',
    ],
  );
});

test('typed app_run starts the Rust ViewModel without designer preview state', () => {
  const runtimeBridge = squash(read('src/mcp-commands-runtime.js'));
  assert.match(
    runtimeBridge,
    /if\(parse\(files\[entry\]\)\.defaults\.contextType\)\{hot\.send\('forma:form-run',\{files\}\)/,
  );
  assert.doesNotMatch(runtimeBridge, /hot\.send\('forma:form-run',\{files,state\}\)/);
});

/** A Studio with just enough state for the helpers every command shares. */
function facadeStudio(overrides = {}) {
  const calls = [];
  return {
    calls,
    files: { 'ui/Demo.ui': 'component Demo { }' },
    active: 'ui/Demo.ui',
    entry: 'ui/Demo.ui',
    scenario: -1,
    mode: 'design',
    error: null,
    selected: null,
    pending: null,
    breakOn: false,
    renderer: 'dom',
    compiled: { nodes: [{ start: 17, type: 'Frame', children: [] }], states: [{ name: 'empty' }] },
    open: path => calls.push(['open', path]),
    persist: () => calls.push(['persist']),
    compile: force => calls.push(['compile', force]),
    select: node => calls.push(['select', node.start]),
    ...overrides,
  };
}

test('the snapshot every command returns describes the Studio without leaking its objects', () => {
  const idle = createIdeCommands(facadeStudio())('ide_status');
  assert.deepEqual(idle, {
    active: 'ui/Demo.ui',
    entry: 'ui/Demo.ui',
    scenario: null,
    mode: 'design',
    error: null,
    selected: null,
    debug: { breakOn: false, paused: false, action: null },
    renderer: 'dom',
    backend: 'ask',
    platform: 'desktop',
    capabilities: { vectorRenderer: false, rustExecution: false, rustDap: false },
  });

  const busy = createIdeCommands(
    facadeStudio({
      scenario: 0,
      selected: { start: 17 },
      pending: { action: 'actions.go' },
      breakOn: true,
      backend: 'metal',
      platform: 'ios',
      vectorPreview: {},
      hot: { send() {} },
    }),
  )('ide_status');
  assert.equal(busy.scenario, 'empty');
  assert.equal(busy.selected, 17);
  assert.deepEqual(busy.debug, { breakOn: true, paused: true, action: 'actions.go' });
  assert.deepEqual([busy.backend, busy.platform], ['metal', 'ios']);
  assert.deepEqual(busy.capabilities, {
    vectorRenderer: true,
    rustExecution: true,
    rustDap: false,
  });
  // A scenario index past the compiled states reads as no scenario.
  assert.equal(createIdeCommands(facadeStudio({ scenario: 5 }))('ide_status').scenario, null);
});

test('shared lookups name what is missing and unknown commands are refused', () => {
  const studio = facadeStudio();
  const ide = createIdeCommands(studio);
  assert.throws(() => ide('no_such_command'), /Unknown IDE command: no_such_command/);
  assert.throws(() => ide('toString'), /Unknown IDE command/);
  assert.throws(() => ide('file_read', { path: 'ui/None.ui' }), /File not found: ui\/None.ui/);
  assert.throws(() => ide('file_read', { path: 'toString' }), /File not found/);
  assert.throws(
    () => ide('component_select', { start: 999 }),
    /Component not found; refresh component_tree/,
  );
  assert.equal(ide('component_select', { start: 17 }).selected, null);
  assert.deepEqual(studio.calls, [['select', 17]]);
});

test('a written file drops the selection, reopens the active file, saves and recompiles', () => {
  const studio = facadeStudio({ selected: { start: 17 } });
  const ide = createIdeCommands(studio);
  const result = ide('file_write', { path: 'ui/New.ui', content: 'component New { }' });
  assert.equal(studio.selected, null);
  assert.equal(result.selected, null);
  assert.deepEqual(studio.calls, [['open', 'ui/Demo.ui'], ['persist'], ['compile', true]]);
});
