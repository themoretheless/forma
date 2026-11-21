/** Destructurable shallow and deep reactive stores built on `signal` and `batch`. */

import type { Signal } from './reactivity-context.ts';
import { batch, signal } from './reactivity-core.ts';

type StoredFieldSignal = (() => unknown) & { set(value: unknown): void };
type DeepStoreNode =
  StoredFieldSignal | { snapshot(): unknown; patch(partial: Record<string, unknown>): void };

export type SignalStore<T extends object> = {
  [K in keyof T]: Signal<T[K]>;
} & {
  snapshot(): T;
  patch(partial: Partial<T>): void;
};

export function createStore<T extends object>(initial: T): SignalStore<T> {
  const store = {} as Record<string, StoredFieldSignal>;
  const keys = (Object.keys(initial) as Array<keyof T>).filter(
    k => k !== '__proto__' && k !== 'constructor' && k !== 'prototype',
  );

  for (const key of keys) {
    store[key as string] = signal(initial[key]) as unknown as StoredFieldSignal;
  }

  Object.defineProperty(store, 'snapshot', {
    value: (): T => {
      const snap = {} as T;
      for (const key of keys) {
        snap[key] = store[key as string]!() as T[keyof T];
      }
      return snap;
    },
    enumerable: false,
  });

  Object.defineProperty(store, 'patch', {
    value: (partial: Partial<T>) => {
      batch(() => {
        for (const key of Object.keys(partial) as Array<keyof T>) {
          if (
            key !== '__proto__' &&
            key !== 'constructor' &&
            key !== 'prototype' &&
            Object.prototype.hasOwnProperty.call(store, key)
          ) {
            store[key as string]!.set(partial[key]);
          }
        }
      });
    },
    enumerable: false,
  });

  return store as SignalStore<T>;
}

export type DeepSignalStore<T> = T extends readonly unknown[]
  ? Signal<T>
  : T extends object
    ? {
        [K in keyof T]: DeepSignalStore<T[K]>;
      } & {
        snapshot(): T;
        patch(partial: Record<string, unknown>): void;
      }
    : Signal<T>;

/** Creates a recursively destructurable Deep Signal Store (`createDeepStore`). */
export function createDeepStore<T extends object>(initial: T): DeepSignalStore<T> {
  const store = {} as Record<string, DeepStoreNode>;
  const keys = (Object.keys(initial) as Array<keyof T>).filter(
    k => k !== '__proto__' && k !== 'constructor' && k !== 'prototype',
  );

  for (const key of keys) {
    const val = initial[key];
    if (val !== null && typeof val === 'object' && !Array.isArray(val)) {
      store[key as string] = createDeepStore(val) as unknown as DeepStoreNode;
    } else {
      store[key as string] = signal(val) as unknown as DeepStoreNode;
    }
  }

  Object.defineProperty(store, 'snapshot', {
    value: (): T => {
      const snap = {} as Record<string, unknown>;
      for (const key of keys) {
        const prop = store[key as string];
        if (prop && 'snapshot' in prop && typeof prop.snapshot === 'function') {
          snap[key as string] = prop.snapshot();
        } else if (typeof prop === 'function') {
          snap[key as string] = prop();
        }
      }
      return snap as T;
    },
    enumerable: false,
  });

  Object.defineProperty(store, 'patch', {
    value: (partial: Record<string, unknown>) => {
      batch(() => {
        for (const [k, v] of Object.entries(partial)) {
          if (
            k === '__proto__' ||
            k === 'constructor' ||
            k === 'prototype' ||
            !Object.prototype.hasOwnProperty.call(store, k)
          ) {
            continue;
          }
          const target = store[k];
          if (!target) continue;
          if (
            v !== null &&
            typeof v === 'object' &&
            !Array.isArray(v) &&
            'patch' in target &&
            typeof target.patch === 'function'
          ) {
            target.patch(v as Record<string, unknown>);
          } else if ('set' in target && typeof target.set === 'function') {
            target.set(v);
          }
        }
      });
    },
    enumerable: false,
  });

  return store as unknown as DeepSignalStore<T>;
}
