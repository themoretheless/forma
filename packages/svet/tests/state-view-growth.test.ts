import { expect, it } from 'vitest';
import {
  computed,
  createGpuTensor,
  createWasmCoreInstance,
  effect,
  getGraphTelemetry,
  signal,
  withWasmCore,
} from '../src/engine';

it.each(['signal', 'computed', 'effect', 'pooled effect', 'tensor'] as const)(
  'refreshes old computed state views when %s allocation relocates Rust storage',
  kind => {
    const core = createWasmCoreInstance();
    core.memory.grow(8); // Keep the ArrayBuffer stable; test an internal Vec relocation.
    withWasmCore(core, () => {
      const source = signal(0),
        values = Array.from({ length: 32 }, () => computed(() => source()));
      const extras: Array<{ dispose(): void }> = [];
      try {
        if (kind === 'pooled effect') {
          const pool = Array.from({ length: 64 }, () => effect(() => {}));
          pool.forEach(value => value.dispose());
          // Consume freed native slots while retaining the reusable JS shells.
          extras.push(...Array.from({ length: 64 }, () => signal(0)));
        }
        values.forEach(value => expect(value.peek()).toBe(0));
        source.set(1);
        values.forEach(value => expect(value.peek()).toBe(1));
        const pointer = core.svet_get_states_ptr(),
          buffer = core.memory.buffer;
        while (core.svet_get_states_ptr() === pointer && extras.length < 256) {
          extras.push(
            kind === 'signal'
              ? signal(0)
              : kind === 'computed'
                ? computed(() => 0)
                : kind === 'effect' || kind === 'pooled effect'
                  ? effect(() => {})
                  : createGpuTensor(0),
          );
        }
        expect(core.svet_get_states_ptr()).not.toBe(pointer);
        source.set(2);
        expect(values.map(value => value.peek())).toEqual(Array(32).fill(2));
        expect(core.memory.buffer).toBe(buffer);
      } finally {
        extras.forEach(value => value.dispose());
        values.forEach(value => value.dispose());
        source.dispose();
      }
      expect(getGraphTelemetry().nodes).toBe(0);
    });
  },
);
