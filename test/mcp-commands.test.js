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
