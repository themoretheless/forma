// Shared MCP tool protocol between mcp/server.js and the Studio IDE bridge in src/mcp-commands*.js.
// Zero npm dependencies so browser/Studio modules can import it directly without pulling in Node SDKs.
import { BACKENDS, PLATFORMS } from './enums.js';

export const MCP_TOOLS = [
  {
    name: 'app_run',
    domain: 'runtime',
    description:
      'Launch current UI: typed forms open a native window with the live Rust ViewModel; Runtime projects use vector or legacy WebView preview',
    schema: {
      backend: { type: 'enum', values: BACKENDS, optional: true },
      platform: { type: 'enum', values: PLATFORMS, optional: true },
    },
    readOnly: false,
  },
  {
    name: 'app_stop',
    domain: 'runtime',
    description: 'Close the running native application',
    schema: {},
    readOnly: false,
  },
  {
    name: 'rust_run',
    domain: 'runtime',
    description:
      'Compile and execute current virtual Cargo project locally, offline. Runs user Rust code; inspect events_read for completion.',
    schema: {},
    readOnly: false,
  },
  {
    name: 'rust_stop',
    domain: 'runtime',
    description: 'Stop active Cargo run',
    schema: {},
    readOnly: false,
  },
  {
    name: 'ide_status',
    domain: 'project',
    description: 'Read active file, preview, diagnostics, selection and debugger status',
    schema: {},
    readOnly: true,
  },
  {
    name: 'project_read',
    domain: 'project',
    description: 'Read the full current virtual project (also serves as export)',
    schema: {},
    readOnly: true,
  },
  {
    name: 'project_export_native',
    domain: 'project',
    description:
      'Generate standalone native Rust crate scaffold (Cargo.toml, src/main.rs, and .ui files) for the current project',
    schema: {
      crateName: { type: 'string', optional: true },
      formaDependencyPath: { type: 'string', optional: true },
    },
    readOnly: true,
  },
  {
    name: 'file_read',
    domain: 'project',
    description: 'Read one project file',
    schema: { path: { type: 'path' } },
    readOnly: true,
  },
  {
    name: 'file_write',
    domain: 'project',
    description:
      'Create/update a virtual project file. expectedContent prevents overwriting concurrent edits; omit only to create.',
    schema: {
      path: { type: 'path' },
      content: { type: 'string' },
      expectedContent: { type: 'string', optional: true },
    },
    readOnly: false,
  },
  {
    name: 'file_open',
    domain: 'project',
    description: 'Open a file in the editor',
    schema: { path: { type: 'path' } },
    readOnly: false,
  },
  {
    name: 'editor_fold',
    domain: 'project',
    description:
      'Collapse/expand a block at one-based line; omit line for all blocks in active file',
    schema: {
      line: { type: 'positiveInt', optional: true },
      collapsed: { type: 'boolean' },
    },
    readOnly: false,
  },
  {
    name: 'project_replace',
    domain: 'project',
    description: 'Replace project using a previously read full project as concurrency guard',
    schema: {
      files: { type: 'stringRecord' },
      expectedFiles: { type: 'stringRecord' },
    },
    readOnly: false,
    destructive: true,
  },
  {
    name: 'preview_open',
    domain: 'design',
    description: 'Set a component as preview entry',
    schema: { path: { type: 'path' } },
    readOnly: false,
  },
  {
    name: 'preview_renderer',
    domain: 'design',
    description: 'Select HTML compatibility preview or native-shared Rust/WASM vector primitives',
    schema: { renderer: { type: 'enum', values: ['html', 'vector'] } },
    readOnly: false,
  },
  {
    name: 'vector_status',
    domain: 'design',
    description:
      'Read loaded vector source, template, actual CPU/WebGPU backend, geometry uploads, frame counters and fallback reason',
    schema: {},
    readOnly: true,
  },
  {
    name: 'preview_scenario',
    domain: 'design',
    description:
      'Choose an existing design state by name; null shows the base values of the design file',
    schema: { name: { type: 'string', nullable: true } },
    readOnly: false,
  },
  {
    name: 'design_states_read',
    domain: 'design',
    description:
      'Read the design file the states panel authors: the base block and every state, each entry with its type, key and the markup text of its values',
    schema: {},
    readOnly: true,
  },
  {
    name: 'design_state_create',
    domain: 'design',
    description:
      'Add an empty design state block. Returns the file text to use as expectedContent next.',
    schema: { name: { type: 'string' }, expectedContent: { type: 'string' } },
    readOnly: false,
  },
  {
    name: 'design_state_rename',
    domain: 'design',
    description: 'Rename a design state block',
    schema: {
      name: { type: 'string' },
      newName: { type: 'string' },
      expectedContent: { type: 'string' },
    },
    readOnly: false,
  },
  {
    name: 'design_state_delete',
    domain: 'design',
    description: 'Delete a design state block together with its overrides',
    schema: { name: { type: 'string' }, expectedContent: { type: 'string' } },
    readOnly: false,
  },
  {
    name: 'design_state_property',
    domain: 'design',
    description:
      'Override one property of one control in a state (state null means the base block), creating that entry when the block does not patch the key yet. Value is markup text as the file spells it: "Текст", 240, true, #e8edf7.',
    schema: {
      state: { type: 'string', nullable: true, optional: true },
      type: { type: 'string' },
      key: { type: 'string' },
      property: { type: 'string' },
      value: { type: 'string' },
      expectedContent: { type: 'string' },
    },
    readOnly: false,
  },
  {
    name: 'preview_mode',
    domain: 'design',
    description: 'Select design or interaction mode',
    schema: { mode: { type: 'enum', values: ['design', 'interact'] } },
    readOnly: false,
  },
  {
    name: 'preview_viewport',
    domain: 'design',
    description: 'Choose desktop/mobile and dark/light surface',
    schema: {
      device: { type: 'enum', values: ['desktop', 'mobile'] },
      theme: { type: 'enum', values: ['dark', 'light'] },
    },
    readOnly: false,
  },
  {
    name: 'component_tree',
    domain: 'design',
    description: 'Read compiled nodes with source offsets and properties',
    schema: {},
    readOnly: true,
  },
  {
    name: 'designer_tree',
    domain: 'design',
    description:
      'Read the visible designer control-tree panel, source paths, offsets and selection',
    schema: {},
    readOnly: true,
  },
  {
    name: 'designer_tree_view',
    domain: 'design',
    description:
      'Show/hide the control-tree panel or switch between preview hierarchy and active-file structure',
    schema: {
      visible: { type: 'boolean', optional: true },
      scope: { type: 'enum', values: ['designer', 'file'], optional: true },
    },
    readOnly: false,
  },
  {
    name: 'designer_tree_select',
    domain: 'design',
    description:
      'Select a source control from designer_tree by id; opens its file without selecting text',
    schema: { id: { type: 'string' } },
    readOnly: false,
  },
  {
    name: 'designer_tree_fold',
    domain: 'design',
    description: 'Collapse/expand a visible node from designer_tree',
    schema: { id: { type: 'string' }, collapsed: { type: 'boolean' } },
    readOnly: false,
  },
  {
    name: 'component_spacing',
    domain: 'design',
    description: 'Read declared and computed margin/padding of selected design component',
    schema: {},
    readOnly: true,
  },
  {
    name: 'component_select',
    domain: 'design',
    description: 'Select a component by its source start offset from component_tree',
    schema: { start: { type: 'nonnegativeInt' } },
    readOnly: false,
  },
  {
    name: 'component_property',
    domain: 'design',
    description:
      'Set a simple existing literal property; rejects stale source and expression replacement',
    schema: {
      start: { type: 'int' },
      property: { type: 'string' },
      value: { type: 'scalar' },
      expectedContent: { type: 'string' },
    },
    readOnly: false,
  },
  {
    name: 'component_controls',
    domain: 'design',
    description: 'List the controls the palette can insert, each with the markup it inserts',
    schema: {},
    readOnly: true,
  },
  {
    name: 'component_insert',
    domain: 'design',
    description:
      'Insert one control next to a node from component_tree; side is the slot: inside (a container), after (a sibling) or auto (follow the selection). Returns the node start the preview selected and the file text for the next expectedContent, with changed telling whether the file moved.',
    schema: {
      start: { type: 'int' },
      markup: { type: 'string' },
      side: { type: 'enum', values: ['inside', 'after', 'auto'], optional: true },
      expectedContent: { type: 'string' },
    },
    readOnly: false,
  },
  {
    name: 'component_move',
    domain: 'design',
    description:
      'Move one control to another slot of the same parent, as the tree drag does. Reports changed:false with the unchanged text when the node already holds that slot.',
    schema: {
      start: { type: 'int' },
      target: { type: 'int' },
      side: { type: 'enum', values: ['before', 'after'] },
      expectedContent: { type: 'string' },
    },
    readOnly: false,
  },
  {
    name: 'component_delete',
    domain: 'design',
    description:
      'Remove one control from its container, as the Delete key does in the designer; the container keeps the selection',
    schema: { start: { type: 'int' }, expectedContent: { type: 'string' } },
    readOnly: false,
  },
  {
    name: 'component_duplicate',
    domain: 'design',
    description:
      'Copy one control into the slot after it, as the designer duplicates it. A copied key is renamed; the copy keeps its coordinates, so set x/y on it to separate the two.',
    schema: { start: { type: 'int' }, expectedContent: { type: 'string' } },
    readOnly: false,
  },
  {
    name: 'state_read',
    domain: 'runtime',
    description: 'Read current preview state',
    schema: {},
    readOnly: true,
  },
  {
    name: 'state_set',
    domain: 'runtime',
    description: 'Update existing top-level preview state fields',
    schema: { values: { type: 'scalarRecord' } },
    readOnly: false,
  },
  {
    name: 'event_dispatch',
    domain: 'runtime',
    description: 'Dispatch a declared clicked event on a component; observes debugger pause',
    schema: { start: { type: 'int' } },
    readOnly: false,
  },
  {
    name: 'debug_break',
    domain: 'runtime',
    description: 'Enable/disable pause before UI events',
    schema: { enabled: { type: 'boolean' } },
    readOnly: false,
  },
  {
    name: 'debug_continue',
    domain: 'runtime',
    description: 'Continue the pending UI event (not a Rust debugger)',
    schema: {},
    readOnly: false,
  },
  {
    name: 'debug_reset',
    domain: 'runtime',
    description: 'Restore the preview state and clear pause',
    schema: {},
    readOnly: false,
  },
  {
    name: 'events_read',
    domain: 'runtime',
    description: 'Read event log',
    schema: {},
    readOnly: true,
  },
  {
    name: 'events_clear',
    domain: 'runtime',
    description: 'Clear UI event log',
    schema: {},
    readOnly: false,
  },
  {
    name: 'diagnostics_read',
    domain: 'runtime',
    description: 'Read current compilation diagnostics',
    schema: {},
    readOnly: true,
  },
];

export const MCP_TOOL_NAMES = MCP_TOOLS.map(tool => tool.name);
