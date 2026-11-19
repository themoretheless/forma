// Shared MCP tool protocol between the unified Rust MCP server (crates/forma-mcp), mcp/smoke.js,
// and the Studio IDE bridge in src/mcp-commands*.js.
import tools from './mcp-tools.json' with { type: 'json' };

export const MCP_TOOLS = tools;
export const MCP_TOOL_NAMES = MCP_TOOLS.map(tool => tool.name);

export const MCP_RUNTIME_TOOL_NAMES = [
  'ui_status',
  'ui_controls',
  'ui_accessibility_tree',
  'ui_click',
  'ui_pointer',
  'ui_set_backend',
  'ui_set_platform',
  'ui_source_read',
  'ui_source_write',
  'ui_render_snapshot',
  'ui_events',
  'app_launch',
];

export const ALL_MCP_TOOL_NAMES = [...MCP_TOOL_NAMES, ...MCP_RUNTIME_TOOL_NAMES];
