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
  if (val === null || typeof val !== 'object') {
    return val;
  }
  if (typeof structuredClone === 'function') {
    return structuredClone(val);
  }
  return cloneFallback(val);
}

function cloneFallback<T>(val: T): T {
  if (val === null || typeof val !== 'object') return val;
  if (val instanceof Date) return new Date(val.getTime()) as unknown as T;
  if (val instanceof RegExp) return new RegExp(val.source, val.flags) as unknown as T;
  if (Array.isArray(val)) return val.map(cloneFallback) as unknown as T;
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(val)) {
    out[key] = cloneFallback((val as Record<string, unknown>)[key]);
  }
  return out as T;
}

function areFormValuesEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') {
    return false;
  }
  if (a instanceof Date && b instanceof Date) return Object.is(a.getTime(), b.getTime());
  if (a instanceof RegExp && b instanceof RegExp) {
    return a.source === b.source && a.flags === b.flags;
  }
  if (Array.isArray(a) || Array.isArray(b)) {
    if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
    return a.every((item, idx) => areFormValuesEqual(item, b[idx]));
  }
  const recA = a as Record<string, unknown>;
  const recB = b as Record<string, unknown>;
  const keysA = Object.keys(recA);
  const keysB = Object.keys(recB);
  if (keysA.length !== keysB.length) return false;
  return keysA.every(
    key =>
      Object.prototype.hasOwnProperty.call(recB, key) && areFormValuesEqual(recA[key], recB[key]),
  );
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
  options?: { stiffness?: number; damping?: number; precision?: number },
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
  (getter as SpringSignal & { id: number }).id = currentSig.id;
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
    setOpts?: { hard?: boolean; steps?: number; animate?: boolean },
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
  },
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

export interface FormFieldRule<V = unknown, AllValues = Record<string, unknown>> {
  required?: boolean;
  min?: number;
  max?: number;
  minLength?: number;
  maxLength?: number;
  range?: [number, number] | undefined;
  pattern?: RegExp;
  validate?: (value: V, allValues: AllValues) => string | null | undefined;
  validateAsync?: (
    value: V,
    options: { signal: AbortSignal; allValues: AllValues },
  ) => Promise<string | null | undefined>;
}

export interface ReactiveForm<T extends object> {
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

function validateSyncField<T extends object, K extends keyof T>(
  val: T[K],
  rule: FormFieldRule<T[K], T> | undefined,
  getAllValuesReactive: () => T,
): string | null {
  if (!rule) return null;
  if (rule.required && (val === null || val === undefined || val === '')) {
    return 'Обязательное поле';
  }
  if (typeof val === 'number') {
    const minBound = rule.range ? rule.range[0] : rule.min;
    const maxBound = rule.range ? rule.range[1] : rule.max;
    if (minBound !== undefined && maxBound !== undefined && (val < minBound || val > maxBound)) {
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
}

function startAsyncFieldValidation<T extends object, K extends keyof T>(
  key: K,
  val: T[K],
  asyncErrorSig: Signal<string | null>,
  asyncValidatingSig: Signal<boolean>,
  validateAsync: NonNullable<FormFieldRule<T[K], T>['validateAsync']>,
  activeControllers: Map<keyof T, AbortController>,
  getAllValuesReactive: () => T,
): Promise<void> {
  activeControllers.get(key)?.abort();
  const ctrl = new AbortController();
  activeControllers.set(key, ctrl);
  asyncValidatingSig.set(true);
  const allVals = untrack(() => getAllValuesReactive());
  return validateAsync(val, { signal: ctrl.signal, allValues: allVals })
    .then(msg => {
      if (!ctrl.signal.aborted) {
        batch(() => {
          asyncErrorSig.set(msg ?? null);
          asyncValidatingSig.set(false);
        });
      }
    })
    .catch(() => {
      if (!ctrl.signal.aborted) {
        asyncValidatingSig.set(false);
      }
    });
}

function attachAsyncFieldValidator<T extends object, K extends keyof T>(
  key: K,
  valueSig: Signal<T[K]>,
  syncErrorSig: ReadonlySignal<string | null>,
  asyncErrorSig: Signal<string | null>,
  asyncValidatingSig: Signal<boolean>,
  validateAsync: NonNullable<FormFieldRule<T[K], T>['validateAsync']>,
  activeControllers: Map<keyof T, AbortController>,
  getAllValuesReactive: () => T,
): void {
  effect(() => {
    const val = valueSig();
    const sErr = syncErrorSig();
    if (sErr !== null) {
      const prevCtrl = activeControllers.get(key);
      if (prevCtrl) {
        prevCtrl.abort();
        activeControllers.delete(key);
      }
      asyncErrorSig.set(null);
      asyncValidatingSig.set(false);
      return;
    }
    void startAsyncFieldValidation(
      key,
      val,
      asyncErrorSig,
      asyncValidatingSig,
      validateAsync,
      activeControllers,
      getAllValuesReactive,
    );
  });
}

/**
 * Creates a reactive, per-field and cross-field validated Form (`createForm` / `createContractForm`)
 * supporting Forma's `required`, `range: [min, max]`, `min`, `max`, `minLength`, `maxLength`,
 * `pattern: RegExp`, synchronous cross-field `validate(val, allValues)`, and AbortController-backed
 * `validateAsync(val, { signal, allValues })` with atomic `batch()` updates.
 */
export function createForm<T extends object>(config: {
  initialValues: T;
  rules?: { [K in keyof T]?: FormFieldRule<T[K], T> };
}): ReactiveForm<T> {
  const keys = Object.keys(config.initialValues) as Array<keyof T>;
  const baselineValues = {} as T;
  const values = {} as { [K in keyof T]: Signal<T[K]> };
  const touched = {} as { [K in keyof T]: Signal<boolean> };
  const dirty = {} as { [K in keyof T]: ReadonlySignal<boolean> };
  const syncErrors = {} as { [K in keyof T]: ReadonlySignal<string | null> };
  const asyncErrors = {} as { [K in keyof T]: Signal<string | null> };
  const asyncValidating = {} as { [K in keyof T]: Signal<boolean> };
  const errors = {} as { [K in keyof T]: ReadonlySignal<string | null> };
  const activeControllers = new Map<keyof T, AbortController>();

  const abortActiveControllers = () => {
    for (const ctrl of activeControllers.values()) ctrl.abort();
    activeControllers.clear();
  };
  onCleanup(abortActiveControllers);

  const getAllValuesReactive = (): T => {
    const snap = {} as T;
    for (const k of keys) snap[k] = values[k]();
    return snap;
  };

  for (const key of keys) {
    baselineValues[key] = structuredCloneIfObject(config.initialValues[key]);
    values[key] = signal<T[keyof T]>(structuredCloneIfObject(baselineValues[key]));
    touched[key] = signal<boolean>(false);
    asyncErrors[key] = signal<string | null>(null);
    asyncValidating[key] = signal<boolean>(false);
    dirty[key] = computed<boolean>(() => !areFormValuesEqual(values[key](), baselineValues[key]));
    const rule = config.rules?.[key];
    syncErrors[key] = computed<string | null>(() =>
      validateSyncField(values[key](), rule, getAllValuesReactive),
    );
    errors[key] = computed<string | null>(() => syncErrors[key]() ?? asyncErrors[key]());
    if (rule?.validateAsync) {
      attachAsyncFieldValidator(
        key,
        values[key],
        syncErrors[key],
        asyncErrors[key],
        asyncValidating[key],
        rule.validateAsync,
        activeControllers,
        getAllValuesReactive,
      );
    }
  }

  const isDirty = computed<boolean>(() => keys.some(k => dirty[k]()));
  const isValidating = computed<boolean>(() => keys.some(k => asyncValidating[k]()));
  const isValid = computed<boolean>(
    () => !keys.some(k => errors[k]() !== null || asyncValidating[k]()),
  );

  const touchAll = () =>
    batch(() => {
      for (const key of keys) touched[key].set(true);
    });

  return {
    values,
    errors,
    touched,
    dirty,
    isDirty,
    isValidating,
    isValid,
    setValue: <K extends keyof T>(key: K, value: T[K]) =>
      batch(() => {
        touched[key]?.set(true);
        values[key]?.set(value);
      }),
    setValues: (partial: Partial<T>) =>
      batch(() => {
        for (const k of Object.keys(partial) as Array<keyof T>) {
          if (k in values) {
            touched[k].set(true);
            values[k].set(partial[k] as T[keyof T]);
          }
        }
      }),
    async validateAll(): Promise<boolean> {
      touchAll();
      const tasks: Promise<void>[] = [];
      for (const key of keys) {
        const rule = config.rules?.[key];
        if (rule?.validateAsync && syncErrors[key]() === null) {
          tasks.push(
            startAsyncFieldValidation(
              key,
              values[key].peek(),
              asyncErrors[key],
              asyncValidating[key],
              rule.validateAsync,
              activeControllers,
              getAllValuesReactive,
            ),
          );
        }
      }
      if (tasks.length > 0) await Promise.all(tasks);
      return isValid();
    },
    reset() {
      abortActiveControllers();
      batch(() => {
        for (const key of keys) {
          values[key].set(structuredCloneIfObject(baselineValues[key]));
          touched[key].set(false);
          asyncErrors[key].set(null);
          asyncValidating[key].set(false);
        }
      });
    },
    submit(onSubmit: (cleanValues: T) => void): boolean {
      touchAll();
      if (!isValid()) return false;
      const snapshot = {} as T;
      for (const key of keys) snapshot[key] = structuredCloneIfObject(values[key].peek());
      onSubmit(snapshot);
      return true;
    },
  };
}

export const createContractForm = createForm;
