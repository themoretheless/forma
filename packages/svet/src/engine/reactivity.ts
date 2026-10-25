/**
 * # Svet Reactive Primitives (`reactivity.ts`)
 *
 * Includes:
 * 1. **WASM Free-List Slot Recycling**: `dispose()` returns the node ID to the Rust WASM `free_list`
 *    so long-running SPAs reuse memory slots with 0 unbounded growth.
 * 2. **Hierarchical Owner/Scope Tree (`createRoot`, `onCleanup`)**: Any `effect` or `computed`
 *    created inside a `<Show>`, `<For>`, or `createRoot` scope automatically registers with its
 *    parent `Owner` and is disposed when the branch/item unmounts!
 * 3. **Zero-Allocation `untrack()`**: Uses `svet_pause_tracking()` / `svet_resume_tracking()` in WASM
 *    without allocating temporary nodes.
 * 4. **Multi-Instance SSR Isolation (`runWithIsolatedContext`)**: Each SSR request can run on its own
 *    isolated `WebAssembly.Instance` with its own registries.
 */

import {
  createWasmCoreInstance,
  getWasmCore,
  KIND_COMPUTED,
  KIND_EFFECT,
  KIND_SIGNAL,
  readFlushedEffects,
  STATE_CLEAN,
  SvetWasmExports,
  withWasmCore,
} from './wasm-bridge';

export interface ReadonlySignal<T> {
  (): T;
  readonly id: number;
  readonly __svet_sig?: boolean;
  peek(): T;
  dispose(): void;
}

export interface Signal<T> extends ReadonlySignal<T> {
  set(value: T | ((prev: T) => T)): void;
  update(updater: (prev: T) => T): void;
}

export interface EffectHandle {
  readonly id: number;
  dispose(): void;
}

export interface OwnerScope {
  disposables: Array<() => void>;
  children: Set<OwnerScope> | null;
  parent: OwnerScope | null;
  disposed: boolean;
  context?: Map<symbol, any>;
}

type EffectEntry = {
  kind: 2;
  id: number;
  core: SvetWasmExports;
  ctx: JsReactiveContext;
  userFn: (onCleanupFn: (cleanupFn: () => void) => void) => void | (() => void);
  initialized: boolean;
  wasmLinked: boolean;
  inUnlinkedList: boolean;
  run: () => void;
  rawFn: () => void;
  registerCleanup: (cleanupFn: () => void) => void;
  handle: EffectHandle;
  cleanup?: () => void;
  disposed: boolean;
  owner: OwnerScope | null;
  deps: number[];
  dep0: number;
  dep1: number;
  depsCount: number;
  pureSources: boolean;
};

type ComputedEntry<T = unknown> = {
  kind: 1;
  id: number;
  compute: () => T;
  value: T;
  initialized: boolean;
  equals: (a: T, b: T) => boolean;
  deps: number[];
  dep0: number;
  dep1: number;
  depsCount: number;
  lastCleanClock: number;
  pureSources: boolean;
};

interface JsReactiveContext {
  signalRegistry: Array<any>;
  effectRegistry: Array<EffectEntry | undefined>;
  computedRegistry: Array<ComputedEntry<any> | undefined>;
  currentOwner: OwnerScope | null;
  activeObserverId: number;
  activeEntry: EffectEntry | ComputedEntry<any> | null;
  activeCursor: number;
  activeTopologyChanged: boolean;
  globalClock: number;
  topologyVersion: number;
  singleSignalRootId: number; // >= 0 when exactly 1 active signal exists in this context, -2 when multiple
  batchDepth: number;
  isRunningEffect: boolean;
  cascadedDuringEffect: boolean;
  statesView: Uint8Array | null;
  statesPtr: number;
  statesCap: number;
  effectPool: EffectEntry[];
  handlePool: EffectHandle[];
  unlinkedEffects: EffectEntry[];
}

const contextByCore = new WeakMap<SvetWasmExports, JsReactiveContext>();
let cachedCore: SvetWasmExports | null = null;
let cachedCtx: JsReactiveContext | null = null;

function getJsContext(core: SvetWasmExports = getWasmCore()): JsReactiveContext {
  if (core === cachedCore && cachedCtx) {
    return cachedCtx;
  }
  let ctx = contextByCore.get(core);
  if (!ctx) {
    ctx = {
      signalRegistry: [],
      effectRegistry: [],
      computedRegistry: [],
      currentOwner: null,
      activeObserverId: -1,
      activeEntry: null,
      activeCursor: 0,
      activeTopologyChanged: false,
      globalClock: 1,
      topologyVersion: 1,
      singleSignalRootId: -1,
      batchDepth: 0,
      isRunningEffect: false,
      cascadedDuringEffect: false,
      statesView: null,
      statesPtr: 0,
      statesCap: 0,
      effectPool: [],
      handlePool: [],
      unlinkedEffects: [],
    };
    contextByCore.set(core, ctx);
  }
  cachedCore = core;
  cachedCtx = ctx;
  return ctx;
}

function flushUnlinkedEffectsToWasm(core: SvetWasmExports, ctx: JsReactiveContext): void {
  const list = ctx.unlinkedEffects;
  const len = list.length;
  if (len === 0) return;
  for (let i = 0; i < len; i++) {
    const eff = list[i];
    eff.inUnlinkedList = false;
    if (!eff.disposed && !eff.wasmLinked && eff.depsCount === 1) {
      eff.wasmLinked = true;
      core.svet_link_1(eff.id, eff.dep0);
    }
  }
  list.length = 0;
}

function flushPooledDisposalsToWasm(core: SvetWasmExports, ctx: JsReactiveContext): void {
  const pool = ctx.effectPool;
  for (let i = 0; i < pool.length; i++) {
    const eff = pool[i];
    if (eff.id >= 0) {
      core.svet_dispose_node(eff.id);
      eff.id = -1;
    }
  }
}

function trackReadFast(ctx: JsReactiveContext, sourceId: number): void {
  const entry = ctx.activeEntry;
  if (!entry) return;
  const cur = ctx.activeCursor;
  if (cur === 0) {
    if (entry.depsCount >= 1 && entry.dep0 === sourceId) {
      ctx.activeCursor = 1;
      return;
    }
    ctx.activeTopologyChanged = true;
    entry.dep0 = sourceId;
    ctx.activeCursor = 1;
    return;
  }
  if (cur === 1) {
    if (entry.dep0 === sourceId) return;
    if (entry.depsCount >= 2 && entry.dep1 === sourceId) {
      ctx.activeCursor = 2;
      return;
    }
    ctx.activeTopologyChanged = true;
    entry.dep1 = sourceId;
    ctx.activeCursor = 2;
    return;
  }
  // 3 or more dependencies
  if (entry.dep0 === sourceId || entry.dep1 === sourceId) return;
  const deps = entry.deps;
  const arrIdx = cur - 2;
  for (let i = 0; i < arrIdx; i++) {
    if (deps[i] === sourceId) return;
  }
  if (cur < entry.depsCount && deps[arrIdx] === sourceId) {
    ctx.activeCursor = cur + 1;
    return;
  }
  ctx.activeTopologyChanged = true;
  if (arrIdx < deps.length) {
    deps[arrIdx] = sourceId;
  } else {
    deps.push(sourceId);
  }
  ctx.activeCursor = cur + 1;
}

function commitTrackingFast(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  targetId: number,
  entry: EffectEntry | ComputedEntry<any>
): void {
  const cur = ctx.activeCursor;
  if (cur !== entry.depsCount) {
    entry.depsCount = cur;
    if (cur <= 2 && entry.deps.length > 0) {
      entry.deps.length = 0;
    } else if (cur > 2) {
      entry.deps.length = cur - 2;
    }
    ctx.activeTopologyChanged = true;
  }
  if (!ctx.activeTopologyChanged) {
    setNodeStateFast(core, ctx, targetId, STATE_CLEAN);
    return;
  }
  ctx.topologyVersion++;
  // Auto-bind direct 1-dependency effect to its source signal on the very first run!
  if (entry.kind === 2 && cur === 1 && !entry.wasmLinked) {
    const sig = ctx.signalRegistry[entry.dep0];
    if (sig && sig._tryRegisterDirectUnlinked(entry, ctx.topologyVersion)) {
      if (!entry.inUnlinkedList) {
        entry.inUnlinkedList = true;
        ctx.unlinkedEffects.push(entry);
      }
      return;
    }
  }
  if (entry.kind === 1) {
    // A computed node depends on these sources, so invalidate any direct-only effect cache on them
    if (cur >= 1) {
      const s0 = ctx.signalRegistry[entry.dep0];
      if (s0) s0._invalidateDirectEffect(ctx.topologyVersion, true);
    }
    if (cur >= 2) {
      const s1 = ctx.signalRegistry[entry.dep1];
      if (s1) s1._invalidateDirectEffect(ctx.topologyVersion, true);
    }
    for (let i = 2; i < cur; i++) {
      const sN = ctx.signalRegistry[entry.deps[i - 2]];
      if (sN) sN._invalidateDirectEffect(ctx.topologyVersion, true);
    }
  } else if (entry.kind === 2 && cur >= 2) {
    const s0 = ctx.signalRegistry[entry.dep0];
    if (s0) s0._invalidateDirectEffect(ctx.topologyVersion);
    const s1 = ctx.signalRegistry[entry.dep1];
    if (s1) s1._invalidateDirectEffect(ctx.topologyVersion);
    for (let i = 2; i < cur; i++) {
      const sN = ctx.signalRegistry[entry.deps[i - 2]];
      if (sN) sN._invalidateDirectEffect(ctx.topologyVersion);
    }
  }
  if (entry.kind === 2) {
    entry.wasmLinked = true;
  }
  if (cur === 0) {
    core.svet_sync_sources_buffered(targetId, 0);
  } else if (cur === 1) {
    core.svet_link_1(targetId, entry.dep0);
  } else if (cur === 2) {
    core.svet_link_2(targetId, entry.dep0, entry.dep1);
  } else {
    const ptr = core.svet_prepare_lis_input(cur);
    const buf = new Uint32Array(core.memory.buffer, ptr, cur);
    buf[0] = entry.dep0;
    buf[1] = entry.dep1;
    for (let i = 2; i < cur; i++) {
      buf[i] = entry.deps[i - 2];
    }
    core.svet_sync_sources_buffered(targetId, cur);
  }
}

function setNodeStateFast(core: SvetWasmExports, ctx: JsReactiveContext, id: number, state: number): void {
  let view = ctx.statesView;
  if (!view || view.buffer !== core.memory.buffer || id >= ctx.statesCap) {
    getNodeStateFast(core, ctx, id);
    view = ctx.statesView;
  }
  if (view && id < ctx.statesCap) {
    view[id] = state;
  }
}

function getNodeStateFast(core: SvetWasmExports, ctx: JsReactiveContext, id: number): number {
  let view = ctx.statesView;
  if (!view || view.buffer !== core.memory.buffer || id >= ctx.statesCap) {
    const cap = core.svet_get_capacity();
    if (cap === 0 || id >= cap) return STATE_CLEAN;
    const ptr = core.svet_get_states_ptr();
    view = new Uint8Array(core.memory.buffer, ptr, cap);
    ctx.statesView = view;
    ctx.statesPtr = ptr;
    ctx.statesCap = cap;
  }
  return view[id];
}

function disposeOwnerScope(scope: OwnerScope): void {
  if (scope.disposed) return;
  scope.disposed = true;

  // 1. Recursively dispose child scopes first
  if (scope.children) {
    for (const child of scope.children) {
      disposeOwnerScope(child);
    }
    scope.children.clear();
  }

  // 2. Run all registered disposables & cleanups in reverse order
  for (let i = scope.disposables.length - 1; i >= 0; i--) {
    try {
      scope.disposables[i]();
    } catch {
      // Ignore cleanup errors during teardown
    }
  }
  scope.disposables.length = 0;

  if (scope.parent && scope.parent.children) {
    scope.parent.children.delete(scope);
    scope.parent = null;
  }
}

function addChildScope(parent: OwnerScope, child: OwnerScope): void {
  if (!parent.children) {
    parent.children = new Set();
  }
  parent.children.add(child);
}

/**
 * Creates a hierarchical ownership root (`createRoot`).
 * All effects, computed nodes, and `onCleanup` callbacks created inside `fn` are tied to this scope
 * and automatically disposed + recycled in WASM when `dispose()` is called!
 */
export function createRoot<T>(fn: (dispose: () => void) => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const prevOwner = ctx.currentOwner;

  const scope: OwnerScope = {
    disposables: [],
    children: null,
    parent: null,
    disposed: false,
    context: prevOwner?.context ? new Map(prevOwner.context) : undefined,
  };

  ctx.currentOwner = scope;
  try {
    return fn(() => disposeOwnerScope(scope));
  } finally {
    ctx.currentOwner = prevOwner;
  }
}

/**
 * Registers a cleanup callback on the current active `OwnerScope` (component, effect, `<Show>`, or `<For>`).
 */
export function onCleanup(cleanupFn: () => void): void {
  const ctx = getJsContext();
  if (ctx.currentOwner && !ctx.currentOwner.disposed) {
    ctx.currentOwner.disposables.push(cleanupFn);
  }
}

/**
 * Returns the currently active `OwnerScope` (or `null` if outside a root/effect).
 */
export function getCurrentOwner(): OwnerScope | null {
  return getJsContext().currentOwner;
}

/**
 * Runs `fn` within a child `OwnerScope` that inherits from the current owner and attaches `contextValues`.
 */
export function runWithOwnerScope<T>(
  contextEntries: Array<[symbol, any]>,
  fn: () => T
): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const prevOwner = ctx.currentOwner;

  const contextMap = new Map<symbol, any>(prevOwner?.context);
  for (const [k, v] of contextEntries) {
    contextMap.set(k, v);
  }

  const scope: OwnerScope = {
    disposables: [],
    children: null,
    parent: prevOwner,
    disposed: false,
    context: contextMap,
  };

  if (prevOwner) {
    addChildScope(prevOwner, scope);
  }

  ctx.currentOwner = scope;
  try {
    return fn();
  } finally {
    ctx.currentOwner = prevOwner;
  }
}

/**
 * Looks up a value by symbol key up the `OwnerScope` chain.
 */
export function lookupOwnerContext<T>(key: symbol, defaultValue: T): T {
  let curr = getJsContext().currentOwner;
  while (curr) {
    if (curr.context && curr.context.has(key)) {
      return curr.context.get(key) as T;
    }
    curr = curr.parent;
  }
  return defaultValue;
}

export type DeepSignalStore<T> = T extends Record<string, any>
  ? {
      [K in keyof T]: T[K] extends Record<string, any>
        ? T[K] extends any[]
          ? Signal<T[K]>
          : DeepSignalStore<T[K]>
        : Signal<T[K]>;
    } & {
      snapshot(): T;
      patch(partial: Record<string, any>): void;
    }
  : Signal<T>;

/**
 * Creates a recursively destructurable Deep Signal Store (`createDeepStore`).
 * Every nested object property (`store.profile.settings.theme`) is an independent WASM-backed Signal,
 * so destructuring at ANY depth (`const { theme } = store.profile.settings`) retains 100% reactivity!
 */
export function createDeepStore<T extends Record<string, any>>(initial: T): DeepSignalStore<T> {
  const store = {} as Record<string, any>;
  const keys = (Object.keys(initial) as Array<keyof T>).filter(
    (k) => k !== '__proto__' && k !== 'constructor' && k !== 'prototype'
  );

  for (const key of keys) {
    const val = initial[key];
    if (val !== null && typeof val === 'object' && !Array.isArray(val)) {
      store[key as string] = createDeepStore(val);
    } else {
      store[key as string] = signal(val);
    }
  }

  Object.defineProperty(store, 'snapshot', {
    value: (): T => {
      const snap = {} as Record<string, any>;
      for (const key of keys) {
        const prop = store[key as string];
        if (prop && typeof prop.snapshot === 'function') {
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
    value: (partial: Record<string, any>) => {
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
          if (v !== null && typeof v === 'object' && !Array.isArray(v) && typeof target.patch === 'function') {
            target.patch(v);
          } else if (typeof target.set === 'function') {
            target.set(v);
          }
        }
      });
    },
    enumerable: false,
  });

  return store as DeepSignalStore<T>;
}

/**
 * Runs `fn` inside a completely isolated WASM instance and JS reactive context.
 * Solves concurrent SSR state bleed across parallel HTTP requests.
 */
export function runWithIsolatedContext<T>(fn: () => T): T {
  const isolatedCore = createWasmCoreInstance();
  return withWasmCore(isolatedCore, () => {
    return createRoot(() => fn());
  });
}

export function invalidateSingleSignalFastPath(): void {
  const ctx = getJsContext();
  ctx.singleSignalRootId = -2;
}

export function trackNodeRead(id: number): void {
  const ctx = getJsContext();
  if (ctx.activeObserverId >= 0) {
    trackReadFast(ctx, id);
  }
}

export function triggerNodeWrite(id: number): void {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  ctx.globalClock++;
  if (ctx.isRunningEffect) {
    ctx.cascadedDuringEffect = true;
    core.svet_trigger_write(id);
    return;
  }
  const res = core.svet_trigger_and_flush(id);
  if (res > 0) {
    const effId = res - 1;
    const entry = ctx.effectRegistry[effId];
    if (entry && !entry.disposed) {
      ctx.isRunningEffect = true;
      try {
        entry.run();
      } finally {
        ctx.isRunningEffect = false;
      }
      if (ctx.cascadedDuringEffect) {
        ctx.cascadedDuringEffect = false;
        flushEffects();
      }
    }
  } else if (res < 0) {
    const count = -res;
    const ptr = core.svet_get_flush_ptr();
    const copy = new Uint32Array(core.memory.buffer, ptr, count).slice();
    ctx.isRunningEffect = true;
    try {
      for (let i = 0; i < count; i++) {
        const effId = copy[i];
        const entry = ctx.effectRegistry[effId];
        if (entry && !entry.disposed && core.svet_get_state(effId) !== STATE_CLEAN) {
          entry.run();
        }
      }
    } finally {
      ctx.isRunningEffect = false;
    }
    if (ctx.cascadedDuringEffect) {
      ctx.cascadedDuringEffect = false;
      flushEffects();
    }
  }
}
export function flushEffects(): void {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  let iterations = 0;
  while (iterations < 100) {
    const queue = readFlushedEffects(core);
    const len = queue.length;
    if (len === 0) break;
    iterations++;
    if (len === 1) {
      const effectId = queue[0];
      const entry = ctx.effectRegistry[effectId];
      if (entry && !entry.disposed) {
        entry.run();
      }
    } else {
      const copy = queue.slice();
      for (let i = 0; i < len; i++) {
        const effectId = copy[i];
        const entry = ctx.effectRegistry[effectId];
        if (entry && !entry.disposed) {
          if (core.svet_get_state(effectId) !== STATE_CLEAN) {
            entry.run();
          }
        }
      }
    }
  }
}

/**
 * Creates a fine-grained reactive Signal backed by the WASM dependency graph.
 */
export function signal<T>(
  initialValue: T,
  options?: { equals?: (a: T, b: T) => boolean }
): Signal<T> {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const id = core.svet_create_node(KIND_SIGNAL);
  if (ctx.singleSignalRootId === -1) {
    ctx.singleSignalRootId = id;
  } else {
    ctx.singleSignalRootId = -2;
  }
  let currentValue = initialValue;
  let disposed = false;
  const equals = options?.equals ?? Object.is;

  let singleSubEffect: EffectEntry | null | undefined = undefined; // undefined = unknown, null = multiple/none
  let singleSubEpoch = -1;
  let hasComputedSubscriber = false;

  const getter = (() => {
    if (ctx.activeObserverId >= 0) {
      const obsEntry = ctx.activeEntry!;
      const cur = ctx.activeCursor;
      if (cur === 0 && obsEntry.depsCount >= 1 && obsEntry.dep0 === id) {
        ctx.activeCursor = 1;
      } else if (cur === 1 && obsEntry.depsCount >= 2 && obsEntry.dep1 === id && obsEntry.dep0 !== id) {
        ctx.activeCursor = 2;
      } else if (!disposed) {
        trackReadFast(ctx, id);
      }
    }
    return currentValue;
  }) as Signal<T>;

  (getter as any).id = id;
  (getter as any).__svet_sig = true;
  (getter as any)._tryRegisterDirectUnlinked = (eff: EffectEntry, ver: number): boolean => {
    if (!hasComputedSubscriber && (singleSubEffect === undefined || singleSubEffect === eff || singleSubEffect?.disposed)) {
      singleSubEffect = eff;
      singleSubEpoch = ver;
      return true;
    }
    if (singleSubEffect && !singleSubEffect.disposed && !singleSubEffect.wasmLinked) {
      singleSubEffect.wasmLinked = true;
      core.svet_link_1(singleSubEffect.id, id);
    }
    singleSubEffect = null;
    singleSubEpoch = ver;
    return false;
  };
  (getter as any)._invalidateDirectEffect = (ver: number, isComputed = false) => {
    if (isComputed) hasComputedSubscriber = true;
    if (singleSubEffect && !singleSubEffect.disposed && !singleSubEffect.wasmLinked) {
      singleSubEffect.wasmLinked = true;
      core.svet_link_1(singleSubEffect.id, id);
    }
    singleSubEffect = null;
    singleSubEpoch = ver;
  };
  ctx.signalRegistry[id] = getter;

  getter.peek = () => currentValue;

  getter.set = (next: T | ((prev: T) => T)) => {
    if (disposed) return;
    const newValue =
      typeof next === 'function' ? (next as (prev: T) => T)(currentValue) : next;
    if (!equals(currentValue, newValue)) {
      currentValue = newValue;
      ctx.globalClock++;
      if (ctx.isRunningEffect) {
        if (
          ctx.batchDepth === 0 &&
          singleSubEffect &&
          !singleSubEffect.disposed &&
          !hasComputedSubscriber &&
          singleSubEffect.depsCount === 1 &&
          singleSubEffect.dep0 === id
        ) {
          singleSubEffect.run();
          return;
        }
        ctx.cascadedDuringEffect = true;
        core.svet_trigger_write(id);
        return;
      }
      // Fast-path: if not batching and this signal has a validated unique terminal effect in the current topology,
      // invoke `singleSubEffect` directly with ZERO WASM FFI calls on the hot path!
      if (ctx.batchDepth === 0 && singleSubEffect && !singleSubEffect.disposed) {
        if (!hasComputedSubscriber && singleSubEffect.depsCount === 1 && singleSubEffect.dep0 === id) {
          const effOwner = singleSubEffect.owner;
          if (
            !singleSubEffect.cleanup &&
            (!effOwner || ((!effOwner.children || effOwner.children.size === 0) && effOwner.disposables.length === 0))
          ) {
            singleSubEffect.rawFn();
            return;
          }
          ctx.isRunningEffect = true;
          try {
            singleSubEffect.run();
          } finally {
            ctx.isRunningEffect = false;
          }
          if (ctx.cascadedDuringEffect) {
            ctx.cascadedDuringEffect = false;
            flushEffects();
          }
          return;
        }
        if (
          singleSubEpoch === ctx.topologyVersion &&
          singleSubEffect.pureSources &&
          !singleSubEffect.owner &&
          !singleSubEffect.cleanup &&
          ctx.singleSignalRootId === id
        ) {
          singleSubEffect.rawFn();
          return;
        }
      }
      const res = core.svet_trigger_and_flush(id);
      if (res > 0) {
        const effId = res - 1;
        const entry = ctx.effectRegistry[effId];
        if (ctx.batchDepth === 0) {
          singleSubEffect = entry ?? null;
          singleSubEpoch = ctx.topologyVersion;
        }
        if (entry && !entry.disposed) {
          ctx.isRunningEffect = true;
          try {
            entry.run();
          } finally {
            ctx.isRunningEffect = false;
          }
          if (ctx.cascadedDuringEffect) {
            ctx.cascadedDuringEffect = false;
            flushEffects();
          }
        }
      } else if (res < 0) {
        singleSubEffect = null;
        singleSubEpoch = ctx.topologyVersion;
        const count = -res;
        const ptr = core.svet_get_flush_ptr();
        const copy = new Uint32Array(core.memory.buffer, ptr, count).slice();
        ctx.isRunningEffect = true;
        try {
          for (let i = 0; i < count; i++) {
            const effId = copy[i];
            const entry = ctx.effectRegistry[effId];
            if (entry && !entry.disposed && core.svet_get_state(effId) !== STATE_CLEAN) {
              entry.run();
            }
          }
        } finally {
          ctx.isRunningEffect = false;
        }
        if (ctx.cascadedDuringEffect) {
          ctx.cascadedDuringEffect = false;
          flushEffects();
        }
      } else if (ctx.batchDepth === 0) {
        singleSubEffect = null;
        singleSubEpoch = ctx.topologyVersion;
      }
    }
  };

  getter.update = (updater: (prev: T) => T) => {
    getter.set(updater(currentValue));
  };

  getter.dispose = () => {
    if (disposed) return;
    disposed = true;
    ctx.topologyVersion++;
    core.svet_dispose_node(id);
  };

  return getter;
}

/**
 * Creates a glitch-free lazy Computed signal backed by the WASM DAG.
 * Automatically disposes its WASM node if created inside an `OwnerScope`.
 */
export function computed<T>(
  fn: () => T,
  options?: { equals?: (a: T, b: T) => boolean }
): ReadonlySignal<T> {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const id = core.svet_create_node(KIND_COMPUTED);
  const equals = options?.equals ?? Object.is;
  let disposed = false;

  const entry: ComputedEntry<T> = {
    kind: 1,
    id,
    compute: fn,
    value: undefined as unknown as T,
    initialized: false,
    equals,
    deps: [],
    dep0: -1,
    dep1: -1,
    depsCount: 0,
    lastCleanClock: 0,
    pureSources: false,
  };
  ctx.computedRegistry[id] = entry;

  const evaluateIfNeeded = (): T => {
    if (disposed) return entry.value;
    if (entry.initialized && entry.lastCleanClock === ctx.globalClock) {
      return entry.value;
    }

    // Ultra-fast path for initialized computed with static topology (`1` or `2` deps):
    // evaluate `fn()` without touching tracking cursor or state arrays!
    if (entry.initialized && entry.pureSources) {
      const prevObs = ctx.activeObserverId;
      ctx.activeObserverId = -1; // children will be called directly without re-recording static edges!
      const nextVal = entry.compute();
      ctx.activeObserverId = prevObs;
      if (nextVal !== entry.value && !entry.equals(entry.value, nextVal)) {
        entry.value = nextVal;
      }
      entry.lastCleanClock = ctx.globalClock;
      const view = ctx.statesView;
      if (view && view.buffer === core.memory.buffer && id < ctx.statesCap) {
        view[id] = STATE_CLEAN;
      }
      return entry.value;
    }

    let state = 2;
    if (entry.initialized) {
      const view = ctx.statesView;
      if (view && view.buffer === core.memory.buffer && id < ctx.statesCap) {
        state = view[id];
      } else {
        state = getNodeStateFast(core, ctx, id);
      }
      if (state === STATE_CLEAN) {
        entry.lastCleanClock = ctx.globalClock;
        return entry.value;
      }
    }

    const prevObs = ctx.activeObserverId;
    const prevEntry = ctx.activeEntry;
    const prevCursor = ctx.activeCursor;
    const prevChanged = ctx.activeTopologyChanged;

    ctx.activeObserverId = id;
    ctx.activeEntry = entry;
    ctx.activeCursor = 0;
    ctx.activeTopologyChanged = !entry.initialized;

    try {
      const nextVal = entry.compute();
      if (!entry.initialized || !entry.equals(entry.value, nextVal)) {
        entry.value = nextVal;
        entry.initialized = true;
      }
    } finally {
      const cur = ctx.activeCursor;
      if (!ctx.activeTopologyChanged && cur === entry.depsCount) {
        entry.pureSources = true;
        const view = ctx.statesView;
        if (view && view.buffer === core.memory.buffer && id < ctx.statesCap) {
          view[id] = STATE_CLEAN;
        } else {
          setNodeStateFast(core, ctx, id, STATE_CLEAN);
        }
      } else {
        entry.pureSources = false;
        commitTrackingFast(core, ctx, id, entry);
      }
      ctx.activeObserverId = prevObs;
      ctx.activeEntry = prevEntry;
      ctx.activeCursor = prevCursor;
      ctx.activeTopologyChanged = prevChanged;
    }
    entry.lastCleanClock = ctx.globalClock;
    return entry.value;
  };

  const getter = (() => {
    if (ctx.activeObserverId < 0) {
      if (entry.initialized) {
        if (entry.lastCleanClock === ctx.globalClock) {
          return entry.value;
        }
        if (entry.pureSources) {
          entry.value = entry.compute();
          entry.lastCleanClock = ctx.globalClock;
          return entry.value;
        }
      }
      return evaluateIfNeeded();
    }
    const val =
      entry.initialized && entry.lastCleanClock === ctx.globalClock
        ? entry.value
        : evaluateIfNeeded();
    const obsEntry = ctx.activeEntry!;
    const cur = ctx.activeCursor;
    if (cur === 0 && obsEntry.depsCount >= 1 && obsEntry.dep0 === id) {
      ctx.activeCursor = 1;
    } else if (cur === 1 && obsEntry.depsCount >= 2 && obsEntry.dep1 === id && obsEntry.dep0 !== id) {
      ctx.activeCursor = 2;
    } else if (!disposed) {
      trackReadFast(ctx, id);
    }
    return val;
  }) as ReadonlySignal<T>;

  (getter as any).id = id;
  getter.peek = () => evaluateIfNeeded();

  getter.dispose = () => {
    if (disposed) return;
    disposed = true;
    ctx.topologyVersion++;
    ctx.computedRegistry[id] = undefined;
    core.svet_dispose_node(id);
  };

  if (ctx.currentOwner) {
    ctx.currentOwner.disposables.push(getter.dispose);
  }

  return getter;
}

function cleanupEffectOwnerScope(effectOwner: OwnerScope | null): void {
  if (!effectOwner) return;
  if (effectOwner.children && effectOwner.children.size > 0) {
    for (const child of effectOwner.children) {
      disposeOwnerScope(child);
    }
    effectOwner.children.clear();
  }
  if (effectOwner.disposables.length > 0) {
    for (let i = effectOwner.disposables.length - 1; i >= 0; i--) {
      try {
        effectOwner.disposables[i]();
      } catch {
        // ignore
      }
    }
    effectOwner.disposables.length = 0;
  }
}

function createEffectEntryShell(core: SvetWasmExports, ctx: JsReactiveContext): EffectEntry {
  const entry: EffectEntry = {
    kind: 2,
    id: -1,
    core,
    ctx,
    userFn: undefined as any,
    initialized: false,
    wasmLinked: false,
    inUnlinkedList: false,
    disposed: false,
    owner: null,
    deps: [],
    dep0: -1,
    dep1: -1,
    depsCount: 0,
    pureSources: false,
    registerCleanup: (cleanupFn: () => void) => {
      entry.cleanup = cleanupFn;
    },
    rawFn: () => {
      const ret = entry.userFn(entry.registerCleanup);
      if (typeof ret === 'function') {
        entry.cleanup = ret;
      }
    },
    run: () => {
      if (entry.disposed) return;
      const effectOwner = entry.owner;
      if (effectOwner && ((effectOwner.children && effectOwner.children.size > 0) || effectOwner.disposables.length > 0)) {
        cleanupEffectOwnerScope(effectOwner);
      }
      if (entry.cleanup) {
        try {
          entry.cleanup();
        } finally {
          entry.cleanup = undefined;
        }
      }

      const id = entry.id;
      const c = entry.core;
      const x = entry.ctx;

      if (entry.initialized && entry.pureSources && !effectOwner) {
        const prevObs = x.activeObserverId;
        x.activeObserverId = -1;
        try {
          const ret = entry.userFn(entry.registerCleanup);
          if (typeof ret === 'function') {
            entry.cleanup = ret;
          }
        } finally {
          x.activeObserverId = prevObs;
          const view = x.statesView;
          if (view && view.buffer === c.memory.buffer && id < x.statesCap) {
            view[id] = STATE_CLEAN;
          } else {
            setNodeStateFast(c, x, id, STATE_CLEAN);
          }
        }
        return;
      }

      const prevOwner = x.currentOwner;
      const prevObs = x.activeObserverId;
      const prevEntry = x.activeEntry;
      const prevCursor = x.activeCursor;
      const prevChanged = x.activeTopologyChanged;

      if (effectOwner) {
        x.currentOwner = effectOwner;
      }
      x.activeObserverId = id;
      x.activeEntry = entry;
      x.activeCursor = 0;
      x.activeTopologyChanged = !entry.initialized;
      entry.initialized = true;

      try {
        const ret = entry.userFn(entry.registerCleanup);
        if (typeof ret === 'function') {
          entry.cleanup = ret;
        }
      } finally {
        const cur = x.activeCursor;
        if (!x.activeTopologyChanged && cur === entry.depsCount) {
          entry.pureSources = true;
          const view = x.statesView;
          if (view && view.buffer === c.memory.buffer && id < x.statesCap) {
            view[id] = STATE_CLEAN;
          } else {
            setNodeStateFast(c, x, id, STATE_CLEAN);
          }
        } else {
          entry.pureSources = false;
          commitTrackingFast(c, x, id, entry);
        }
        x.activeObserverId = prevObs;
        x.activeEntry = prevEntry;
        x.activeCursor = prevCursor;
        x.activeTopologyChanged = prevChanged;
        x.currentOwner = prevOwner;
      }
    },
    handle: undefined as any,
  };
  entry.handle = {
    get id() {
      return entry.id;
    },
    dispose() {
      if (entry.disposed) return;
      entry.disposed = true;
      const effectOwner = entry.owner;
      if (effectOwner) {
        cleanupEffectOwnerScope(effectOwner);
        if (effectOwner.parent && effectOwner.parent.children) {
          effectOwner.parent.children.delete(effectOwner);
          effectOwner.parent = null;
        }
      }
      if (entry.cleanup) {
        entry.cleanup();
        entry.cleanup = undefined;
      }
      const x = entry.ctx;
      const c = entry.core;
      const id = entry.id;
      x.topologyVersion++;
      x.effectRegistry[id] = undefined;
      if (!entry.wasmLinked && x.effectPool.length < 4096) {
        // Keep `entry.id` leased in WASM for the next pooled `effect()` so we skip both `svet_dispose_node` and `svet_create_node`!
        x.effectPool.push(entry);
        return;
      }
      if (id >= 0) {
        c.svet_dispose_node(id);
        entry.id = -1;
      }
      if (x.effectPool.length < 4096) {
        x.effectPool.push(entry);
      }
    },
  };
  return entry;
}

/**
 * Creates an automatically tracked side-effect.
 * - Registers with the enclosing `OwnerScope` (if any) for automatic hierarchical disposal.
 * - Creates its own child `OwnerScope` that is cleaned up before each re-execution!
 * - Recycles its WASM slot ID and JS `EffectEntry` shell back into `free_list` upon `dispose()`.
 */
export function effect(
  fn: (onCleanupFn: (cleanupFn: () => void) => void) => void | (() => void)
): EffectHandle {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const parentOwner = ctx.currentOwner;

  // Only allocate an `OwnerScope` for this effect if there is an active `parentOwner`
  // (e.g. inside `<Show>`, `<For>`, `createRoot`, or another `effect`).
  // Top-level effects without a parent skip `OwnerScope` allocation!
  const effectOwner: OwnerScope | null = parentOwner
    ? {
        disposables: [],
        children: null,
        parent: parentOwner,
        disposed: false,
        context: parentOwner.context,
      }
    : null;

  if (parentOwner && effectOwner) {
    addChildScope(parentOwner, effectOwner);
  }

  let entry = ctx.effectPool.pop();
  if (!entry) {
    entry = createEffectEntryShell(core, ctx);
    entry.id = core.svet_create_node(KIND_EFFECT);
  } else {
    entry.core = core;
    entry.ctx = ctx;
    entry.disposed = false;
    entry.initialized = false;
    entry.wasmLinked = false;
    entry.cleanup = undefined;
    entry.dep0 = -1;
    entry.dep1 = -1;
    entry.depsCount = 0;
    entry.pureSources = false;
    if (entry.deps.length > 0) entry.deps.length = 0;
    if (entry.id < 0) {
      entry.id = core.svet_create_node(KIND_EFFECT);
    }
  }
  const id = entry.id;
  entry.owner = effectOwner;
  entry.userFn = fn;

  ctx.effectRegistry[id] = entry;
  entry.run();

  if (parentOwner) {
    const h = entry.handle;
    parentOwner.disposables.push(() => h.dispose());
  }

  return entry.handle;
}

/**
 * Groups multiple signal writes into a single atomic transaction.
 */
export function batch<T>(fn: () => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  if (ctx.batchDepth === 0 && ctx.unlinkedEffects.length > 0) {
    flushUnlinkedEffectsToWasm(core, ctx);
  }
  ctx.batchDepth++;
  core.svet_batch_start();
  try {
    return fn();
  } finally {
    ctx.batchDepth--;
    const remainingDepth = core.svet_batch_end();
    if (remainingDepth === 0) {
      flushEffects();
    }
  }
}

/**
 * Reads signals inside `fn` without creating any dependency edges in the WASM graph.
 * Uses zero-allocation `svet_pause_tracking` / `svet_resume_tracking` in WASM.
 */
export function untrack<T>(fn: () => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const prevObs = ctx.activeObserverId;
  const prevEntry = ctx.activeEntry;
  ctx.activeObserverId = -1;
  ctx.activeEntry = null;
  try {
    return fn();
  } finally {
    ctx.activeObserverId = prevObs;
    ctx.activeEntry = prevEntry;
  }
}

/**
 * Creates a destructurable store of signals from a plain object.
 */
export type SignalStore<T extends Record<string, any>> = {
  [K in keyof T]: Signal<T[K]>;
} & {
  snapshot(): T;
  patch(partial: Partial<T>): void;
};

export function createStore<T extends Record<string, any>>(initial: T): SignalStore<T> {
  const store = {} as Record<string, any>;
  const keys = (Object.keys(initial) as Array<keyof T>).filter(
    (k) => k !== '__proto__' && k !== 'constructor' && k !== 'prototype'
  );

  for (const key of keys) {
    store[key as string] = signal(initial[key]);
  }

  Object.defineProperty(store, 'snapshot', {
    value: (): T => {
      const snap = {} as T;
      for (const key of keys) {
        snap[key] = store[key as string]();
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
            store[key as string].set(partial[key]);
          }
        }
      });
    },
    enumerable: false,
  });

  return store as SignalStore<T>;
}

/**
 * Resets the entire WASM graph and JS registries.
 */
export function resetReactiveSystem(): void {
  const core = getWasmCore();
  core.svet_reset();
  const ctx = getJsContext(core);
  ctx.signalRegistry.length = 0;
  ctx.effectRegistry.length = 0;
  ctx.computedRegistry.length = 0;
  ctx.currentOwner = null;
  ctx.activeObserverId = -1;
  ctx.activeEntry = null;
  ctx.activeCursor = 0;
  ctx.activeTopologyChanged = false;
  ctx.globalClock = 1;
  ctx.topologyVersion = 1;
  ctx.singleSignalRootId = -1;
  ctx.batchDepth = 0;
  ctx.isRunningEffect = false;
  ctx.cascadedDuringEffect = false;
  ctx.statesView = null;
  ctx.statesPtr = 0;
  ctx.statesCap = 0;
  ctx.effectPool.length = 0;
  ctx.unlinkedEffects.length = 0;
}

/**
 * Returns live telemetry from the WASM reactive graph, including active nodes vs allocated capacity.
 */
export function getGraphTelemetry(): {
  nodes: number;
  capacity: number;
  edges: number;
  wasmMemoryBytes: number;
} {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  if (ctx.unlinkedEffects.length > 0) {
    flushUnlinkedEffectsToWasm(core, ctx);
  }
  if (ctx.effectPool.length > 0) {
    flushPooledDisposalsToWasm(core, ctx);
  }
  return {
    nodes: core.svet_get_node_count(),
    capacity: core.svet_get_capacity(),
    edges: core.svet_get_edge_count(),
    wasmMemoryBytes: core.memory.buffer.byteLength,
  };
}
