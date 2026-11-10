import { registerMcpBridge } from './mcp-bridge.js';
import { registerRunnerWebSockets } from './runner-ws.js';
import { registerWasmMiddlewares } from './wasm-middleware.js';

export function formaMcpBridgePlugin() {
  return {
    name: 'forma-mcp-bridge',
    configureServer(server) {
      registerWasmMiddlewares(server.middlewares);
      const getClient = registerMcpBridge(server);
      registerRunnerWebSockets(server, getClient);
    },
  };
}
