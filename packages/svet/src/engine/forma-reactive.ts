/**
 * # Reactive Springs, Persisted Signals & Contract Forms (`forma-reactive.ts`)
 *
 * - `createSpring` / `flushSpringFrame` — physics-backed harmonic oscillator Spring signals
 *   driven by a shared, batched frame scheduler.
 * - `createPersistedSignal` — reactive signal persisted to `localStorage` and synchronized across tabs.
 * - `createForm` / `createContractForm` — per-field, cross-field, and async-validated reactive forms.
 */

import {
  batch,
  computed,
  effect,
  onCleanup,
  type ReadonlySignal,
  signal,
  type Signal,
  untrack,
} from './reactivity.ts';

export function structuredCloneIfObject<T>(val: T): T {
  if (val !== null && typeof val === 'object') {
    return JSON.parse(JSON.stringify(val));
  }
  return val;
}

export interface SpringSignal extends ReadonlySignal<number> {
  readonly target: ReadonlySignal<number>;
  /** Sets the target value and steps the spring physics toward it */
  set(nextTarget: number, options?: { hard?: boolean; steps?: number; animate?: boolean }): void;
  /** Instantly snaps both target and current value to `nextTarget` with zero velocity */
  snap(nextTarget: number): void;
  /** Advances the spring simulation by `dtSeconds` (default `1/60`) */
  step(dtSeconds?: number): number;
  dispose(): void;
}

const activeAnimatedSprings = new Set<(dt: number) => boolean>();
let sharedSpringRafId: number | null = null;

function scheduleSharedSpringTick(): void {
  if (sharedSpringRafId !== null) return;
  if (typeof requestAnimationFrame !== 'function') return;
  sharedSpringRafId = requestAnimationFrame(() => {
    sharedSpringRafId = null;
    if (activeAnimatedSprings.size === 0) return;
    // Update all active springs in a SINGLE atomic WASM batch per frame!
    batch(() => {
      for (const tickSpring of Array.from(activeAnimatedSprings)) {
        const settled = tickSpring(1 / 60);
        if (settled) {
          activeAnimatedSprings.delete(tickSpring);
        }
      }
    });
    if (activeAnimatedSprings.size > 0) {
      scheduleSharedSpringTick();
    }
  });
}

/**
 * Steps all currently animating springs by `steps` frames inside a single atomic `batch()`.
 * Useful for deterministic testing or manual frame-rate synchronization.
 */
export function flushSpringFrame(steps = 1, dt = 1 / 60): void {
  if (activeAnimatedSprings.size === 0) return;
  for (let s = 0; s < steps; s++) {
    batch(() => {
      for (const tickSpring of Array.from(activeAnimatedSprings)) {
        const settled = tickSpring(dt);
        if (settled) {
          activeAnimatedSprings.delete(tickSpring);
        }
      }
    });
    if (activeAnimatedSprings.size === 0) break;
  }
}

/**
 * Creates a physics-backed harmonic oscillator Spring signal (`createSpring`)
 * with configurable `stiffness` and `damping` integrated into the WASM reactive graph
 * and driven by a shared, batched frame scheduler.
 */
export function createSpring(
  initialValue: number,
  options?: { stiffness?: number; damping?: number; precision?: number }
): SpringSignal {
  const stiffness = options?.stiffness ?? 170;
  const damping = options?.damping ?? 26;
  const precision = options?.precision ?? 0.001;

  const currentSig = signal(initialValue);
  const targetSig = signal(initialValue);
  let velocity = 0;

  const stepAndCheckSettled = (dt = 1 / 60): { value: number; settled: boolean } => {
    const cur = currentSig.peek();
    const tgt = targetSig.peek();
    const displacement = cur - tgt;
    const springForce = -stiffness * displacement;
    const dampingForce = -damping * velocity;
    const acceleration = springForce + dampingForce;

    velocity += acceleration * dt;
    const nextVal = cur + velocity * dt;

    if (Math.abs(velocity) < precision && Math.abs(tgt - nextVal) < precision) {
      velocity = 0;
      currentSig.set(tgt);
      return { value: tgt, settled: true };
    }
    currentSig.set(nextVal);
    return { value: nextVal, settled: false };
  };

  const tickCallback = (dt: number): boolean => stepAndCheckSettled(dt).settled;

  const step = (dt = 1 / 60): number => stepAndCheckSettled(dt).value;

  const getter = (() => currentSig()) as unknown as SpringSignal;
  (getter as any).id = currentSig.id;
  getter.peek = () => currentSig.peek();
  Object.defineProperty(getter, 'target', { value: targetSig, enumerable: true });

  getter.snap = (nextTarget: number) => {
    activeAnimatedSprings.delete(tickCallback);
    velocity = 0;
    batch(() => {
      targetSig.set(nextTarget);
      currentSig.set(nextTarget);
    });
  };

  getter.set = (
    nextTarget: number,
    setOpts?: { hard?: boolean; steps?: number; animate?: boolean }
  ) => {
    if (setOpts?.hard) {
      getter.snap(nextTarget);
      return;
    }
    targetSig.set(nextTarget);
    const simSteps = setOpts?.steps ?? 1;
    let isSettled = false;
    batch(() => {
      for (let i = 0; i < simSteps; i++) {
        const res = stepAndCheckSettled(1 / 60);
        if (res.settled) {
          isSettled = true;
          break;
        }
      }
    });
    if (!isSettled && setOpts?.animate !== false) {
      activeAnimatedSprings.add(tickCallback);
      scheduleSharedSpringTick();
    } else if (isSettled) {
      activeAnimatedSprings.delete(tickCallback);
    }
  };

  getter.step = step;
  getter.dispose = () => {
    activeAnimatedSprings.delete(tickCallback);
    currentSig.dispose();
    targetSig.dispose();
  };
  onCleanup(() => getter.dispose());

  return getter;
}

/**
 * Creates a reactive Signal persisted automatically to `localStorage` (or custom storage)
 * and synchronized across browser tabs via `storage` and `BroadcastChannel` events!
 * Properly closes its `BroadcastChannel` and removes its `storage` listener on `dispose()` or scope cleanup.
 */
export function createPersistedSignal<T>(
  key: string,
  initialValue: T,
  options?: {
    storage?: Pick<Storage, 'getItem' | 'setItem'>;
    channelName?: string;
  }
): Signal<T> {
  const storage =
    options?.storage ?? (typeof localStorage !== 'undefined' ? localStorage : undefined);

  let startVal = initialValue;
  if (storage) {
    try {
      const raw = storage.getItem(key);
      if (raw !== null) {
        startVal = JSON.parse(raw) as T;
      }
    } catch {
      // Fallback to initialValue
    }
  }

  const sig = signal<T>(startVal);
  let suppressBroadcast = false;
  let channel: BroadcastChannel | null = null;

  if (typeof BroadcastChannel !== 'undefined') {
    try {
      channel = new BroadcastChannel(options?.channelName ?? `svet_persisted_${key}`);
      channel.onmessage = (ev: MessageEvent) => {
        if (ev.data && ev.data.key === key) {
          suppressBroadcast = true;
          try {
            sig.set(ev.data.value as T);
          } finally {
            suppressBroadcast = false;
          }
        }
      };
    } catch {
      channel = null;
    }
  }

  const syncEffect = effect(() => {
    const val = sig();
    if (storage) {
      try {
        storage.setItem(key, JSON.stringify(val));
      } catch {
        // Ignore quota errors
      }
    }
    if (channel && !suppressBroadcast) {
      try {
        channel.postMessage({ key, value: val });
      } catch {
        // Ignore non-cloneable or closed channel errors
      }
    }
  });

  let onStorageListener: ((e: StorageEvent) => void) | null = null;
  if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
    onStorageListener = (e: StorageEvent) => {
      if (e.key === key && e.newValue !== null) {
        suppressBroadcast = true;
        try {
          sig.set(JSON.parse(e.newValue) as T);
        } catch {
          // Ignore malformed JSON
        } finally {
          suppressBroadcast = false;
        }
      }
    };
    window.addEventListener('storage', onStorageListener);
  }

  let cleanedUp = false;
  const cleanupPersisted = () => {
    if (cleanedUp) return;
    cleanedUp = true;
    syncEffect.dispose();
    if (
      onStorageListener &&
      typeof window !== 'undefined' &&
      typeof window.removeEventListener === 'function'
    ) {
      window.removeEventListener('storage', onStorageListener);
      onStorageListener = null;
    }
    if (channel) {
      try {
        channel.close();
      } catch {
        // Ignore
      }
      channel = null;
    }
  };

  const origDispose = sig.dispose.bind(sig);
  sig.dispose = () => {
    cleanupPersisted();
    origDispose();
  };
  onCleanup(cleanupPersisted);

  return sig;
}

export interface FormFieldRule<V = any, AllValues = Record<string, any>> {
  required?: boolean;
  min?: number;
  max?: number;
  minLength?: number;
  maxLength?: number;
  range?: [number, number];
  pattern?: RegExp;
  validate?: (value: V, allValues: AllValues) => string | null | undefined;
  validateAsync?: (
    value: V,
    options: { signal: AbortSignal; allValues: AllValues }
  ) => Promise<string | null | undefined>;
}

export interface ReactiveForm<T extends Record<string, any>> {
  readonly values: { [K in keyof T]: Signal<T[K]> };
  readonly errors: { [K in keyof T]: ReadonlySignal<string | null> };
  readonly touched: { [K in keyof T]: Signal<boolean> };
  readonly dirty: { [K in keyof T]: ReadonlySignal<boolean> };
  readonly isDirty: ReadonlySignal<boolean>;
  readonly isValidating: ReadonlySignal<boolean>;
  readonly isValid: ReadonlySignal<boolean>;
  setValue<K extends keyof T>(key: K, value: T[K]): void;
  setValues(partial: Partial<T>): void;
  validateAll(): Promise<boolean>;
  reset(): void;
  submit(onSubmit: (cleanValues: T) => void): boolean;
}

/**
 * Creates a reactive, per-field and cross-field validated Form (`createForm` / `createContractForm`)
 * supporting Forma's `required`, `range: [min, max]`, `min`, `max`, `minLength`, `maxLength`,
 * `pattern: RegExp`, synchronous cross-field `validate(val, allValues)`, and AbortController-backed
 * `validateAsync(val, { signal, allValues })` with atomic `batch()` updates.
 */
export function createForm<T extends Record<string, any>>(config: {
  initialValues: T;
  rules?: { [K in keyof T]?: FormFieldRule<T[K], T> };
}): ReactiveForm<T> {
  const keys = Object.keys(config.initialValues) as Array<keyof T>;
  const values = {} as { [K in keyof T]: Signal<T[K]> };
  const touched = {} as { [K in keyof T]: Signal<boolean> };
  const dirty = {} as { [K in keyof T]: ReadonlySignal<boolean> };
  const syncErrors = {} as { [K in keyof T]: ReadonlySignal<string | null> };
  const asyncErrors = {} as { [K in keyof T]: Signal<string | null> };
  const asyncValidating = {} as { [K in keyof T]: Signal<boolean> };
  const errors = {} as { [K in keyof T]: ReadonlySignal<string | null> };
  const activeControllers = new Map<keyof T, AbortController>();

  for (const key of keys) {
    const initVal = structuredCloneIfObject(config.initialValues[key]);
    values[key] = signal<T[keyof T]>(initVal);
    touched[key] = signal<boolean>(false);
    asyncErrors[key] = signal<string | null>(null);
    asyncValidating[key] = signal<boolean>(false);
    dirty[key] = computed<boolean>(() => !Object.is(values[key](), config.initialValues[key]));
  }

  const getAllValuesReactive = (): T => {
    const snap = {} as T;
    for (const k of keys) {
      snap[k] = values[k]();
    }
    return snap;
  };

  for (const key of keys) {
    const rule = config.rules?.[key];
    syncErrors[key] = computed<string | null>(() => {
      const val = values[key]();
      if (!rule) return null;
      if (rule.required && (val === null || val === undefined || val === '')) {
        return 'Обязательное поле';
      }
      if (typeof val === 'number') {
        const minBound = rule.range ? rule.range[0] : rule.min;
        const maxBound = rule.range ? rule.range[1] : rule.max;
        if (
          minBound !== undefined &&
          maxBound !== undefined &&
          (val < minBound || val > maxBound)
        ) {
          return `Значение должно быть от ${minBound} до ${maxBound}`;
        }
        if (minBound !== undefined && val < minBound) {
          return `Минимум ${minBound}`;
        }
        if (maxBound !== undefined && val > maxBound) {
          return `Максимум ${maxBound}`;
        }
      }
      if (typeof val === 'string') {
        if (rule.minLength !== undefined && val.length < rule.minLength) {
          return `Минимальная длина: ${rule.minLength}`;
        }
        if (rule.maxLength !== undefined && val.length > rule.maxLength) {
          return `Максимальная длина: ${rule.maxLength}`;
        }
        if (rule.pattern && !rule.pattern.test(val)) {
          return 'Некорректный формат';
        }
      }
      if (rule.validate) {
        // Cross-field validation: only subscribes to other fields if `rule.validate` has >= 2 parameters!
        const allVals = rule.validate.length >= 2 ? getAllValuesReactive() : ({} as T);
        return rule.validate(val, allVals) ?? null;
      }
      return null;
    });

    errors[key] = computed<string | null>(() => syncErrors[key]() ?? asyncErrors[key]());

    if (rule?.validateAsync) {
      effect(() => {
        const val = values[key]();
        const sErr = syncErrors[key]();
        const prevCtrl = activeControllers.get(key);
        if (prevCtrl) {
          prevCtrl.abort();
          activeControllers.delete(key);
        }
        if (sErr !== null) {
          asyncErrors[key].set(null);
          asyncValidating[key].set(false);
          return;
        }
        const ctrl = new AbortController();
        activeControllers.set(key, ctrl);
        asyncValidating[key].set(true);
        const allVals = untrack(() => getAllValuesReactive());
        rule
          .validateAsync!(val, { signal: ctrl.signal, allValues: allVals })
          .then((msg) => {
            if (!ctrl.signal.aborted) {
              batch(() => {
                asyncErrors[key].set(msg ?? null);
                asyncValidating[key].set(false);
              });
            }
          })
          .catch(() => {
            if (!ctrl.signal.aborted) {
              asyncValidating[key].set(false);
            }
          });
      });
    }
  }

  const isDirty = computed<boolean>(() => {
    for (const key of keys) {
      if (dirty[key]()) return true;
    }
    return false;
  });

  const isValidating = computed<boolean>(() => {
    for (const key of keys) {
      if (asyncValidating[key]()) return true;
    }
    return false;
  });

  const isValid = computed<boolean>(() => {
    for (const key of keys) {
      if (errors[key]() !== null || asyncValidating[key]()) return false;
    }
    return true;
  });

  const setValue = <K extends keyof T>(key: K, value: T[K]): void => {
    batch(() => {
      touched[key]?.set(true);
      values[key]?.set(value);
    });
  };

  const setValues = (partial: Partial<T>): void => {
    batch(() => {
      for (const k of Object.keys(partial) as Array<keyof T>) {
        if (k in values) {
          touched[k].set(true);
          values[k].set(partial[k] as T[keyof T]);
        }
      }
    });
  };

  const validateAll = async (): Promise<boolean> => {
    batch(() => {
      for (const key of keys) {
        touched[key].set(true);
      }
    });
    const asyncTasks: Promise<void>[] = [];
    for (const key of keys) {
      const rule = config.rules?.[key];
      if (rule?.validateAsync && syncErrors[key]() === null) {
        const prevCtrl = activeControllers.get(key);
        if (prevCtrl) prevCtrl.abort();
        const ctrl = new AbortController();
        activeControllers.set(key, ctrl);
        asyncValidating[key].set(true);
        const val = values[key].peek();
        const allVals = untrack(() => getAllValuesReactive());
        asyncTasks.push(
          rule
            .validateAsync(val, { signal: ctrl.signal, allValues: allVals })
            .then((msg) => {
              if (!ctrl.signal.aborted) {
                batch(() => {
                  asyncErrors[key].set(msg ?? null);
                  asyncValidating[key].set(false);
                });
              }
            })
            .catch(() => {
              if (!ctrl.signal.aborted) {
                asyncValidating[key].set(false);
              }
            })
        );
      }
    }
    if (asyncTasks.length > 0) {
      await Promise.all(asyncTasks);
    }
    return isValid();
  };

  const reset = () => {
    for (const ctrl of activeControllers.values()) {
      ctrl.abort();
    }
    activeControllers.clear();
    batch(() => {
      for (const key of keys) {
        values[key].set(structuredCloneIfObject(config.initialValues[key]));
        touched[key].set(false);
        asyncErrors[key].set(null);
        asyncValidating[key].set(false);
      }
    });
  };

  const submit = (onSubmit: (cleanValues: T) => void): boolean => {
    batch(() => {
      for (const key of keys) {
        touched[key].set(true);
      }
    });
    if (!isValid()) return false;
    const snapshot = {} as T;
    for (const key of keys) {
      snapshot[key] = structuredCloneIfObject(values[key].peek());
    }
    onSubmit(snapshot);
    return true;
  };

  return {
    values,
    errors,
    touched,
    dirty,
    isDirty,
    isValidating,
    isValid,
    setValue,
    setValues,
    validateAll,
    reset,
    submit,
  };
}

export const createContractForm = createForm;
