import {resolve} from 'node:path';
import {createServer} from 'vite';

const namespace = '/__svet_action_http/';
const requests = new Map();
const json = (response, status, value) => {
  response.writeHead(status, {'content-type': 'application/json', 'cache-control': 'no-store'});
  response.end(JSON.stringify(value));
};
const server = await createServer({
  root: resolve(import.meta.dirname, '..'),
  server: {host: '127.0.0.1', port: 5189, strictPort: true},
  plugins: [{
    name: 'svet-action-http-fixture',
    configureServer(vite) {
      vite.middlewares.use((request, response, next) => {
        const url = new URL(request.url, 'http://127.0.0.1');
        if (!url.pathname.startsWith(namespace)) return next();
        if (request.method !== 'GET') return json(response, 405, {error: 'GET only'});
        const id = url.searchParams.get('id');
        if (!id || id.length > 100) return json(response, 400, {error: 'Test request id required'});
        const operation = url.pathname.slice(namespace.length);
        if (operation === 'wait') {
          if (requests.has(id) || requests.size >= 256) return json(response, 409, {error: 'Duplicate id or fixture capacity'});
          const streaming = url.searchParams.get('stream') === '1';
          const state = {response, streaming, closed: false, complete: false};
          requests.set(id, state);
          response.once('close', () => { state.closed = true; });
          if (streaming) {
            response.writeHead(200, {'content-type': 'application/json', 'cache-control': 'no-store'});
            response.write('{"value":');
          }
          return;
        }
        const state = requests.get(id);
        if (operation === 'stats') return json(response, 200, state ? {
          started: true, streaming: state.streaming, closed: state.closed,
          complete: state.complete, writableEnded: state.response.writableEnded,
        } : {started: false});
        if (operation === 'finish') {
          if (!state || state.closed) return json(response, 404, {error: 'No active test response'});
          state.complete = true;
          const value = Number(url.searchParams.get('value'));
          if (state.streaming) state.response.end(`${JSON.stringify(value)}}`);
          else json(state.response, 200, {value});
          return json(response, 200, {finished: true});
        }
        if (operation === 'forget') {
          if (state && !state.response.writableEnded) state.response.destroy();
          requests.delete(id);
          return json(response, 200, {forgotten: true});
        }
        json(response, 404, {error: 'Unknown fixture operation'});
      });
    },
  }],
});
await server.listen();
console.log('Svet action browser fixture: http://127.0.0.1:5189/tests/browser/server-action-http.html');
let closing = false;
const close = async () => {
  if (closing) return;
  closing = true;
  for (const state of requests.values()) state.response.destroy();
  requests.clear();
  server.httpServer?.closeAllConnections();
  await server.close();
};
process.once('SIGINT', close);
process.once('SIGTERM', close);
