import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { ALL_MCP_TOOL_NAMES } from '../protocol/mcp-tools.js';

const config = JSON.parse(await readFile(new URL('../.mcp.json', import.meta.url), 'utf8'))
  .mcpServers.forma;
const client = new Client({ name: 'forma-smoke', version: '1.0.0' });
await client.connect(
  new StdioClientTransport({
    command: config.command,
    args: config.args,
    env: { ...process.env, ...config.env },
  }),
);
async function call(name, args = {}) {
  const r = await client.callTool({ name, arguments: args });
  assert.ok(!r.isError, r.content[0]?.text);
  return JSON.parse(r.content[0].text);
}
try {
  const tools = await client.listTools();
  // Compare with the shared protocol registry instead of a number that goes stale with every new tool.
  assert.deepEqual(tools.tools.map(tool => tool.name).sort(), [...ALL_MCP_TOOL_NAMES].sort());
  const uiStatus = await call('ui_status');
  assert.ok(uiStatus.controlCount >= 1);
  const clicked = await call('ui_click', { key: uiStatus.controls[0].key });
  assert.equal(clicked.clicksAfter, clicked.clicksBefore + 1);
  const uiStatusResource = await client.readResource({ uri: 'forma://ui/status' });
  assert.equal(JSON.parse(uiStatusResource.contents[0].text).totalClicks, 1);
  const status = await call('ide_status');
  const file = await call('file_read', { path: status.entry });
  const write = await call('file_write', {
    path: file.path,
    content: file.content,
    expectedContent: file.content,
  });
  assert.equal(write.error, '');
  const stale = await client.callTool({
    name: 'file_write',
    arguments: { path: file.path, content: 'invalid', expectedContent: 'stale' },
  });
  assert.equal(stale.isError, true);
  assert.equal((await call('file_read', { path: file.path })).content, file.content);
  const tree = await call('component_tree');
  assert.ok(tree.nodes.length);
  await call('editor_fold', { collapsed: true });
  const selected = await call('component_select', { start: tree.nodes[0].start });
  assert.equal(selected.selected, tree.nodes[0].start);
  await call('debug_break', { enabled: true });
  assert.equal((await call('ide_status')).debug.breakOn, true);
  await call('debug_break', { enabled: status.debug.breakOn });
  const resources = await client.readResource({ uri: 'forma://language' });
  assert.ok(resources.contents[0].text.includes('Forma'));
  const spacing = await call('component_spacing');
  assert.equal(spacing.type, tree.nodes[0].type);
  console.log(
    `PASS: ${ALL_MCP_TOOL_NAMES.length} MCP tools, runtime UI + live IDE reads/writes, stale-write guard, component selection and spacing, debugger configuration, resources.`,
  );
} finally {
  await client.close();
}
