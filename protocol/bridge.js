// Where the MCP bridge of the dev server lives, shared by the server that announces it and the
// MCP process that connects to it.

export const BRIDGE_PATH = '/__forma_mcp';

const LOOPBACK_HOSTS = ['127.0.0.1', '[::1]', 'localhost'];

/**
 * The bridge of a dev server listening at `address` (the value of `httpServer.address()`):
 * the URL to announce and the `Host` values a request may carry. Null when the server is not
 * reachable through loopback, where the bridge stays closed.
 */
export function bridgeEndpoint(address) {
  if (!address || typeof address !== 'object' || !Number.isInteger(address.port)) return null;
  const hosts = {
    '127.0.0.1': ['127.0.0.1', 'localhost'],
    '::1': ['[::1]', 'localhost'],
    '0.0.0.0': ['127.0.0.1', 'localhost'],
    '::': LOOPBACK_HOSTS,
  }[address.address];
  if (!hosts) return null;
  return {
    url: `http://${hosts[0]}:${address.port}${BRIDGE_PATH}`,
    hosts: hosts.map(host => `${host}:${address.port}`),
  };
}

/** Whether `url` is a bridge address on this machine, the only kind a descriptor may name. */
export function isBridgeUrl(url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  return (
    LOOPBACK_HOSTS.includes(parsed.hostname) &&
    parsed.href === `http://${parsed.host}${BRIDGE_PATH}`
  );
}
