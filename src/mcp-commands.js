import { findNodeByStart } from './element-tree.js';
import { createProjectCommands } from './mcp-commands-project.js';
import { createDesignCommands } from './mcp-commands-design.js';
import { createRuntimeCommands } from './mcp-commands-runtime.js';

// The MCP transport invokes the same runtime and UI operations as the editor. The editor hands
// over one `studio` facade: live state through getters and setters (files, active, entry,
// scenario, selected, logs are reassigned by commands), the panels a command drives, and the
// operations the editor already has. Nothing here reaches into the editor's closure by name.
export function createIdeCommands(studio) {
  const { hot } = studio;
  const snapshot = () => ({
    active: studio.active,
    entry: studio.entry,
    scenario: studio.scenario < 0 ? null : (studio.compiled?.states[studio.scenario]?.name ?? null),
    mode: studio.mode,
    error: studio.error,
    selected: studio.selected?.start ?? null,
    debug: {
      breakOn: studio.breakOn,
      paused: !!studio.pending,
      action: studio.pending?.action ?? null,
    },
    renderer: studio.renderer,
    backend: studio.backend ?? 'ask',
    platform: studio.platform ?? 'desktop',
    capabilities: { vectorRenderer: !!studio.vectorPreview, rustExecution: !!hot, rustDap: false },
  });
  const requireFile = path => {
    if (!Object.hasOwn(studio.files, path)) throw Error('File not found: ' + path);
  };
  const findNode = start => {
    const found = findNodeByStart(studio.compiled?.nodes, start);
    if (!found) throw Error('Component not found; refresh component_tree');
    return found;
  };
  const applyFiles = () => {
    studio.selected = null;
    studio.open(studio.active);
    studio.persist();
    studio.compile(true);
  };
  const helpers = { snapshot, requireFile, findNode, applyFiles };
  const handlers = {
    ...createProjectCommands(studio, helpers),
    ...createDesignCommands(studio, helpers),
    ...createRuntimeCommands(studio, helpers),
  };
  const runCommand = function ideCommand(command, args = {}) {
    if (!Object.hasOwn(handlers, command)) throw Error('Unknown IDE command: ' + command);
    return handlers[command](args);
  };
  runCommand.commandNames = () => Object.keys(handlers);
  return runCommand;
}
