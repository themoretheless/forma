import { createRustRunner } from '../rust-runner.js';
import { createNativeRunner } from '../native-runner.js';
import { evaluateDesignData } from '../design-data-runner.js';
import { createVectorRunner } from '../vector-runner.js';
import { DEFAULT_BACKEND, DEFAULT_PLATFORM, isBackend, isPlatform } from '../protocol/enums.js';

export function registerRunnerWebSockets(server, getClient) {
  let selectedBackend = DEFAULT_BACKEND;
  let selectedPlatform = DEFAULT_PLATFORM;
  let designBusy = false;

  const forwardOutput = data => getClient()?.send('forma:rust-output', data);
  const runner = createRustRunner(forwardOutput);
  const native = createNativeRunner(forwardOutput);
  const vector = createVectorRunner(forwardOutput);

  server.ws.on('forma:backend', (data, sender) => {
    if (sender === getClient() && isBackend(data?.backend)) selectedBackend = data.backend;
  });
  server.ws.on('forma:platform', (data, sender) => {
    if (sender === getClient() && isPlatform(data?.platform)) selectedPlatform = data.platform;
  });

  server.ws.on('forma:vector-run', (data, sender) => {
    if (sender === getClient())
      vector.run({
        ...data,
        backend: data?.backend ?? (selectedBackend !== 'ask' ? selectedBackend : undefined),
        platform:
          data?.platform ?? (selectedPlatform !== DEFAULT_PLATFORM ? selectedPlatform : undefined),
      });
  });
  server.ws.on('forma:vector-update', (data, sender) => {
    if (sender === getClient()) vector.update(data);
  });
  server.ws.on('forma:vector-stop', (_, sender) => {
    if (sender === getClient()) vector.stop();
  });

  server.ws.on('forma:design-data', async (data, sender) => {
    if (sender !== getClient()) return;
    if (designBusy) {
      sender.send('forma:design-data-result', {
        id: data?.id,
        error: 'Вычисление уже выполняется',
      });
      return;
    }
    designBusy = true;
    try {
      sender.send('forma:design-data-result', {
        id: data?.id,
        values: await evaluateDesignData(data?.files, data?.refs),
      });
    } catch (e) {
      sender.send('forma:design-data-result', { id: data?.id, error: e.message });
    } finally {
      designBusy = false;
    }
  });

  server.ws.on('forma:native-run', (data, sender) => {
    if (sender === getClient()) native.run(data);
  });
  server.ws.on('forma:native-update', (data, sender) => {
    if (sender === getClient()) native.update(data);
  });
  server.ws.on('forma:native-stop', (_, sender) => {
    if (sender === getClient()) native.stop();
  });

  server.ws.on('forma:form-run', (data, sender) => {
    if (sender === getClient())
      runner.run(data?.files, {
        generated: true,
        native: true,
        backend: data?.backend ?? selectedBackend,
        platform: data?.platform ?? selectedPlatform,
      });
  });
  server.ws.on('forma:rust-run', (data, sender) => {
    if (sender === getClient()) runner.run(data?.files);
  });
  server.ws.on('forma:rust-stop', (_, sender) => {
    if (sender === getClient()) runner.stop();
  });

  server.httpServer?.once('close', () => {
    vector.stop?.();
    native.stop?.();
    runner.stop?.();
  });
}
