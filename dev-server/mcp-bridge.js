import { randomUUID } from 'node:crypto';
import { chmodSync, mkdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { BRIDGE_PATH, bridgeEndpoint } from '../protocol/bridge.js';

const MAX_MCP_BODY_BYTES = 2_000_000;
const MCP_COMMAND_TIMEOUT_MS = 8_000;

const DESCRIPTOR_DIRECTORY = fileURLToPath(new URL('../.forma/', import.meta.url));

/** Registers the bridge on `server`; its address and token go to `bridge.json` in `directory`. */
export function registerMcpBridge(server, directory = DESCRIPTOR_DIRECTORY) {
  const token = randomUUID();
  const pending = new Map();
  let client = null;
  // Known once the server listens; until then, and off loopback, every request is refused.
  let endpoint = null;

  const descriptor = join(directory, 'bridge.json');
  mkdirSync(directory, { recursive: true });

  server.httpServer?.once('listening', () => {
    endpoint = bridgeEndpoint(server.httpServer.address());
    if (!endpoint) return;
    writeFileSync(descriptor, JSON.stringify({ url: endpoint.url, token }), { mode: 0o600 });
    chmodSync(descriptor, 0o600);
  });
  server.httpServer?.once('close', () => {
    try {
      unlinkSync(descriptor);
    } catch {}
    for (const p of pending.values()) p.reject(Error('IDE server stopped'));
  });

  server.ws.on('forma:hello', (_, sender) => {
    if (!client || client.socket.readyState !== 1) client = sender;
  });
  server.ws.on('forma:result', (data, sender) => {
    if (sender !== client) return;
    const p = pending.get(data.id);
    if (p) {
      pending.delete(data.id);
      p.resolve(data);
    }
  });

  server.middlewares.use(BRIDGE_PATH, async (req, res) => {
    res.setHeader('Content-Type', 'application/json');
    const send = (status, value) => {
      res.statusCode = status;
      res.end(JSON.stringify(value));
    };
    if (
      req.headers.authorization !== `Bearer ${token}` ||
      req.headers.origin ||
      !endpoint?.hosts.includes(req.headers.host)
    )
      return send(403, { error: 'Forbidden' });
    if (req.method !== 'POST') return send(405, { error: 'POST required' });
    if (!client || client.socket.readyState !== 1)
      return send(503, { error: 'Open the IDE browser tab first' });
    try {
      let body = '';
      for await (const chunk of req) {
        body += chunk;
        if (body.length > MAX_MCP_BODY_BYTES) return send(413, { error: 'Request too large' });
      }
      const command = JSON.parse(body);
      const id = randomUUID();
      const result = await new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(id);
          reject(
            Error(
              'IDE did not reply. Outcome may be unknown; inspect state before retrying a write.',
            ),
          );
        }, MCP_COMMAND_TIMEOUT_MS);
        pending.set(id, {
          resolve: r => {
            clearTimeout(timer);
            resolve(r);
          },
          reject: e => {
            clearTimeout(timer);
            reject(e);
          },
        });
        client.send('forma:command', { id, ...command });
      });
      send(200, result);
    } catch (e) {
      send(500, { error: e.message });
    }
  });

  return () => client;
}
