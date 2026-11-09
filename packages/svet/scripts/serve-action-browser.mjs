import { resolve } from 'node:path';
import { createServer } from 'vite';
import { createActionFixture } from './browser-fixture-endpoints.mjs';
const fixture = createActionFixture();
const server = await createServer({
  root: resolve(import.meta.dirname, '..'),
  server: { host: '127.0.0.1', port: 5189, strictPort: true },
  plugins: [
    {
      name: 'svet-browser-fixture',
      configureServer(vite) {
        vite.middlewares.use(fixture.handle);
      },
    },
  ],
});
await server.listen();
console.log('Svet browser fixture: http://127.0.0.1:5189/tests/browser/server-action-http.html');
let closing = false;
const close = async () => {
  if (closing) return;
  closing = true;
  fixture.close();
  server.httpServer?.closeAllConnections();
  await server.close();
};
process.once('SIGINT', close);
process.once('SIGTERM', close);
