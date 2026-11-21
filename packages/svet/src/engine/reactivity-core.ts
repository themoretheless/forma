/** Core reactive primitives: `signal`, `computed`, `effect`, and `batch`. */

import {
  allocateReactiveNode,
  type AnySignalState,
  batchTransport,
  type ComputedEntry,
  type ComputedObserverEntry,
  disposeTrackedNode,
  type EffectEntry,
  type EffectHandle,
  enterObservationContext,
  flushUnlinkedEffectsToWasm,
  getJsContext,
  getNodeStateFast,
  type JsReactiveContext,
  leaveObservationContext,
  MAX_EFFECT_ITERATIONS,
  type OwnerScope,
  reactiveDomains,
  ReactiveCycleError,
  ReactiveStabilizationError,
  type ReadonlySignal,
  releaseStagedDeps,
  resolveCurrentOwner,
  retireNothing,
  setNodeStateFast,
  type Signal,
  type SignalState,
} from './reactivity-context.ts';
import {
  addChildScope,
  cleanupEffectOwnerScope,
  isOwnerScopeDisposed,
  runInForeignReactiveCore,
  runUntrackedInCore,
  untrack,
} from './reactivity-owner.ts';
import {
  beginForeignTracking,
  commitTrackingFast,
  finishForeignBatches,
  flushEffects,
  holdForeignBatch,
  linkDirectSubscriber,
  refreshForeignTracking,
  requestActiveEffectRerun,
  retireForeignSource,
  retireForeignTracking,
  runQueuedEffects,
  trackForeignSource,
  trackReadFast,
} from './reactivity-tracking.ts';
import {
  getWasmCore,
  KIND_COMPUTED,
  KIND_EFFECT,
  KIND_SIGNAL,
  readNodeSources,
  STATE_CLEAN,
  type SvetWasmExports,
  withWasmCore,
} from './wasm-bridge.ts';

const noopCompute = (): never => undefined as never;
const noopEffectUserFn: EffectEntry['userFn'] = () => {};
const noopEffectHandle: EffectHandle = { id: -1, dispose: () => {} };

function detachDisposedSignal(st: AnySignalState): void {
  if (!st.disposed || st.activeWrites > 0) return;
  st.singleSubEffect = null;
  (st as { equals?: unknown }).equals = Object.is;
  st.core = null!;
  st.ctx = null!;
}

function runWrittenEffects(ctx: JsReactiveContext, run: () => void): void {
  ctx.isRunningEffect = true;
  try {
    run();
  } finally {
    ctx.isRunningEffect = false;
  }
  if (ctx.cascadedDuringEffect) {
    ctx.cascadedDuringEffect = false;
    flushEffects();
  }
}

function writeSignal<T>(st: SignalState<T>, next: T | ((prev: T) => T)): void {
  if (st.disposed || st.generation !== st.ctx.generation) return;
  const core = st.core,
    ctx = st.ctx,
    id = st.id;
  st.activeWrites++;
  try {
    if (getWasmCore() !== core) return runInForeignReactiveCore(core, () => st.getter.set(next));
    if (!batchTransport.depth && !ctx.isRunningEffect && ctx.foreignForwarders) {
      return batch(() => st.getter.set(next));
    }
    const newValue = typeof next === 'function' ? (next as (prev: T) => T)(st.value) : next;
    if (st.disposed || st.generation !== ctx.generation) return;
    if (st.equals(st.value, newValue)) return;
    if (st.disposed || st.generation !== ctx.generation) return;
    holdForeignBatch(core, ctx);
    st.value = newValue;
    st.valueVersion++;
    ctx.globalClock++;
    requestActiveEffectRerun(core, ctx, id);
    const direct = st.singleSubEffect;
    if (ctx.isRunningEffect) {
      if (direct && direct.depsCount === 1 && direct.dep0 === id) linkDirectSubscriber(st);
      ctx.cascadedDuringEffect = true;
      core.svet_trigger_write(id);
      return;
    }
    if (
      ctx.batchDepth === 0 &&
      direct &&
      !direct.disposed &&
      !st.hasComputedSubscriber &&
      direct.depsCount === 1 &&
      direct.dep0 === id
    ) {
      runWrittenEffects(ctx, direct.run);
      return;
    }
    const res = core.svet_trigger_and_flush(id);
    if (res > 0) {
      const entry = ctx.effectRegistry[res - 1];
      if (ctx.batchDepth === 0) st.singleSubEffect = entry ?? null;
      if (entry && !entry.disposed) runWrittenEffects(ctx, entry.run);
    } else if (res < 0) {
      st.singleSubEffect = null;
      const copy = new Uint32Array(core.memory.buffer, core.svet_get_flush_ptr(), -res).slice();
      runWrittenEffects(ctx, () => runQueuedEffects(core, ctx, copy));
    } else if (ctx.batchDepth === 0) {
      st.singleSubEffect = null;
    }
  } finally {
    st.activeWrites--;
    detachDisposedSignal(st);
  }
}

function disposeSignal(st: AnySignalState): void {
  if (st.disposed) return;
  st.disposed = true;
  try {
    if (st.generation !== st.ctx.generation) return;
    retireForeignSource(st.getter);
    st.ctx.signalRegistry[st.id] = undefined;
    st.ctx.topologyVersion++;
    disposeTrackedNode(st.core, st.id);
  } finally {
    detachDisposedSignal(st);
  }
}

const SIGNAL_STATE = Symbol('svet.signal.state');

const signalPrototype = Object.create(Function.prototype, {
  __svet_sig: { value: true },
}) as object;
const signalMethods: Record<string, (st: SignalState<unknown>) => unknown> = {
  peek: st => () => st.value,
  set: st => (next: unknown) => writeSignal(st, next),
  update: st => (updater: (prev: unknown) => unknown) => st.getter.set(updater),
  dispose: st => () => disposeSignal(st),
  __svet_cached_value: st => st.getter.peek,
};
for (const [name, makeMethod] of Object.entries(signalMethods)) {
  const own = (handle: object, value: unknown) =>
    Object.defineProperty(handle, name, {
      value,
      writable: true,
      configurable: true,
      enumerable: true,
    });
  Object.defineProperty(signalPrototype, name, {
    configurable: true,
    get(this: { [SIGNAL_STATE]?: SignalState<unknown> }) {
      const st = this[SIGNAL_STATE];
      if (!st) return undefined;
      const method = makeMethod(st);
      own(this, method);
      return method;
    },
    set(this: object, value: unknown) {
      own(this, value);
    },
  });
}

/** @internal Disposes a signal through its state without binding `dispose` onto the handle. */
export function disposeSignalNow<T>(handle: Signal<T>): void {
  const st = (handle as unknown as { [SIGNAL_STATE]?: AnySignalState })[SIGNAL_STATE];
  if (st) disposeSignal(st);
  else handle.dispose();
}

/** @internal `handle.set(next)` without binding `set` onto the handle. */
export function setSignalNow<T>(handle: Signal<T>, next: T | ((prev: T) => T)): void {
  const st = (handle as unknown as { [SIGNAL_STATE]?: SignalState<T> })[SIGNAL_STATE];
  if (st) writeSignal(st, next);
  else handle.set(next as T);
}

function retireSignalOnReset(st: AnySignalState): void {
  disposeSignal(st);
}

export function signal<T>(
  initialValue: T,
  options?: { equals?: (a: T, b: T) => boolean },
): Signal<T> {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const generation = ctx.generation;
  const equals = options?.equals ?? Object.is;
  if (generation !== ctx.generation)
    throw new Error('Reactive graph reset during signal construction');
  if (typeof equals !== 'function') throw new TypeError('Signal equals must be a function');
  const id = allocateReactiveNode(core, KIND_SIGNAL);
  if (ctx.singleSignalRootId === -1) ctx.singleSignalRootId = id;
  else ctx.singleSignalRootId = -2;

  const getter = (() => {
    if (st.disposed || st.generation !== st.ctx.generation) return st.value;
    const c = st.ctx;
    trackForeignSource(st.core, getter, signal, effect, st);
    if (c.activeObserverId >= 0 && reactiveDomains.frames.at(-1) === c) {
      const obsEntry = c.activeEntry!;
      if (obsEntry.retiredDependencies?.delete(id)) c.activeTopologyChanged = true;
      const cur = c.activeCursor;
      if (cur === 0 && obsEntry.depsCount >= 1 && obsEntry.dep0 === id) {
        c.activeCursor = 1;
      } else if (
        cur === 1 &&
        obsEntry.depsCount >= 2 &&
        obsEntry.dep1 === id &&
        obsEntry.dep0 !== id
      ) {
        c.activeCursor = 2;
      } else if (!st.disposed) {
        trackReadFast(c, id);
      }
    }
    return st.value;
  }) as Signal<T>;
  const st: SignalState<T> = {
    id,
    generation,
    core,
    ctx,
    equals,
    value: initialValue,
    valueVersion: 0,
    disposed: false,
    activeWrites: 0,
    singleSubEffect: undefined,
    hasComputedSubscriber: false,
    getter,
    retire: retireSignalOnReset,
  };
  Object.setPrototypeOf(getter, signalPrototype);
  const internalGetter = getter as Signal<T> & {
    id: number;
    [SIGNAL_STATE]: SignalState<T>;
  };
  internalGetter.id = id;
  internalGetter[SIGNAL_STATE] = st;
  ctx.signalRegistry[id] = st;
  return getter;
}

type ComputedShell<T> = {
  core: SvetWasmExports;
  ctx: JsReactiveContext;
  generation: number;
  owner: OwnerScope | null;
  entry: ComputedEntry<T>;
  disposed: boolean;
  detachDisposedContext: () => void;
};

function buildComputedCycleError<T>(shell: ComputedShell<T>): ReactiveCycleError {
  const computeds = shell.ctx.observerStack.filter(item => item.kind === 1);
  const start = Math.max(0, computeds.indexOf(shell.entry));
  return new ReactiveCycleError([...computeds.slice(start).map(item => item.id), shell.entry.id]);
}

function isComputedStale<T>(shell: ComputedShell<T>): boolean {
  const { entry, ctx, core } = shell;
  if (entry.initialized && !entry.failed && entry.lastCleanClock === ctx.globalClock) return false;
  if (!entry.initialized || entry.failed) return true;
  const view = ctx.statesView;
  const state =
    view && view.buffer === core.memory.buffer && entry.id < ctx.statesCap
      ? view[entry.id]!
      : getNodeStateFast(core, ctx, entry.id);
  if (state === STATE_CLEAN) {
    entry.lastCleanClock = ctx.globalClock;
    return false;
  }
  return true;
}

function evaluateComputedEntry<T>(shell: ComputedShell<T>): T {
  const { entry, ctx, core, owner, generation } = shell;
  if (shell.disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
    return entry.value;
  if (entry.evaluating) throw buildComputedCycleError(shell);
  refreshForeignTracking(entry, ctx);
  if (!isComputedStale(shell)) return entry.value;

  const prevObs = ctx.activeObserverId,
    prevEntry = ctx.activeEntry,
    prevCursor = ctx.activeCursor,
    prevChanged = ctx.activeTopologyChanged;
  if (prevEntry) prevEntry.pausedCursor = prevCursor;

  ctx.activeObserverId = entry.id;
  ctx.activeEntry = entry;
  ctx.activeCursor = 0;
  ctx.activeTopologyChanged =
    !entry.initialized || entry.sourceRetirementVersion !== ctx.sourceRetirementVersion;
  entry.topologyInvalidated = false;
  entry.seenDeps = null;

  const evaluations = ctx.observerStack;
  beginForeignTracking(entry);
  evaluations.push(entry);
  enterObservationContext(ctx);
  entry.evaluating = true;
  let succeeded = false;
  try {
    const nextVal = entry.compute();
    if (shell.disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
      return entry.value;
    const changed = !entry.initialized || !entry.equals(entry.value, nextVal);
    if (shell.disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
      return entry.value;
    if (changed) {
      entry.valueVersion++;
      entry.value = nextVal;
      entry.initialized = true;
    }
    entry.failed = false;
    succeeded = true;
  } finally {
    try {
      if (generation === ctx.generation) {
        try {
          if (!shell.disposed && !isOwnerScopeDisposed(owner)) {
            commitTrackingFast(core, ctx, entry.id, entry);
            if (!succeeded) {
              entry.failed = true;
              setNodeStateFast(core, ctx, entry.id, STATE_CLEAN);
            }
          }
        } finally {
          releaseStagedDeps(ctx, entry);
          ctx.activeObserverId = prevObs;
          ctx.activeEntry = prevEntry;
          ctx.activeCursor = prevCursor;
          ctx.activeTopologyChanged = prevChanged || Boolean(prevEntry?.topologyInvalidated);
          if (prevEntry) prevEntry.topologyInvalidated = false;
        }
      } else {
        releaseStagedDeps(ctx, entry);
      }
    } finally {
      entry.evaluating = false;
      entry.retiredDependencies = null;
      if (generation === ctx.generation) evaluations.pop();
      leaveObservationContext(ctx);
      shell.detachDisposedContext();
    }
  }
  entry.lastCleanClock = ctx.globalClock;
  return entry.value;
}

function readComputedValue<T>(shell: ComputedShell<T>, getter: ReadonlySignal<T>): T {
  const { entry, ctx, core, owner, generation } = shell;
  if (shell.disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
    return entry.value;
  const foreignTarget = reactiveDomains.frames.at(-1);
  if (foreignTarget?.activeEntry && foreignTarget !== ctx && foreignTarget.activeObserverId >= 0) {
    if (entry.evaluating) throw buildComputedCycleError(shell);
    trackForeignSource(core, getter, signal, effect, () => entry.valueVersion);
    return runInForeignReactiveCore(core, () => getter());
  }
  if (ctx.activeObserverId >= 0 && reactiveDomains.frames.at(-1) !== ctx) {
    return runInForeignReactiveCore(core, () => getter());
  }
  if (ctx.activeObserverId >= 0 && getWasmCore() !== core) {
    return reactiveDomains.frames.at(-1) === ctx
      ? withWasmCore(core, () => getter())
      : runInForeignReactiveCore(core, () => getter());
  }
  try {
    if (entry.evaluating) throw buildComputedCycleError(shell);
    refreshForeignTracking(entry, ctx);
    if (ctx.activeObserverId < 0) {
      if (entry.initialized && !entry.failed && entry.lastCleanClock === ctx.globalClock) {
        return entry.value;
      }
      return getWasmCore() === core
        ? evaluateComputedEntry(shell)
        : runInForeignReactiveCore(core, () => evaluateComputedEntry(shell));
    }
    const val =
      entry.initialized && !entry.failed && entry.lastCleanClock === ctx.globalClock
        ? entry.value
        : evaluateComputedEntry(shell);
    if (shell.disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation) return val;
    recordComputedDependency(ctx, entry.id, shell);
    return val;
  } catch (error) {
    if (
      !shell.disposed &&
      !isOwnerScopeDisposed(owner) &&
      generation === ctx.generation &&
      ctx.activeObserverId >= 0 &&
      ctx.activeObserverId !== entry.id
    ) {
      trackReadFast(ctx, entry.id);
    }
    throw error;
  }
}

function recordComputedDependency<T>(
  ctx: JsReactiveContext,
  id: number,
  shell: ComputedShell<T>,
): void {
  const obsEntry = ctx.activeEntry!;
  if (obsEntry.retiredDependencies?.delete(id)) ctx.activeTopologyChanged = true;
  const cur = ctx.activeCursor;
  if (cur === 0 && obsEntry.depsCount >= 1 && obsEntry.dep0 === id) {
    ctx.activeCursor = 1;
  } else if (cur === 1 && obsEntry.depsCount >= 2 && obsEntry.dep1 === id && obsEntry.dep0 !== id) {
    ctx.activeCursor = 2;
  } else if (!shell.disposed && !isOwnerScopeDisposed(shell.owner)) {
    trackReadFast(ctx, id);
  }
}

/** Creates a glitch-free lazy Computed signal backed by the WASM DAG. */
export function computed<T>(
  fn: () => T,
  options?: { equals?: (a: T, b: T) => boolean },
): ReadonlySignal<T> {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const generation = ctx.generation;
  const owner = resolveCurrentOwner(ctx, core);
  if (isOwnerScopeDisposed(owner)) throw new Error('Cannot create computed in a disposed owner');
  if (typeof fn !== 'function') throw new TypeError('Computed body must be a function');
  const equals = options?.equals ?? Object.is;
  if (isOwnerScopeDisposed(owner)) throw new Error('Computed owner disposed during construction');
  if (generation !== ctx.generation)
    throw new Error('Reactive graph reset during computed construction');
  if (typeof equals !== 'function') throw new TypeError('Computed equals must be a function');
  const id = allocateReactiveNode(core, KIND_COMPUTED);

  const entry: ComputedEntry<T> = {
    kind: 1,
    id,
    core,
    compute: fn,
    value: undefined as unknown as T,
    initialized: false,
    equals,
    stagedDeps: null,
    dep0: -1,
    dep1: -1,
    depsCount: 0,
    pausedCursor: 0,
    lastCleanClock: 0,
    valueVersion: 0,
    evaluating: false,
    failed: false,
    retiredDependencies: null,
    seenDeps: null,
    retire: retireNothing,
    sourceRetirementVersion: 0,
    topologyInvalidated: false,
  };
  ctx.computedRegistry[id] = entry;

  const shell: ComputedShell<T> = {
    core,
    ctx,
    generation,
    owner,
    entry,
    disposed: false,
    detachDisposedContext() {
      if (!shell.disposed || entry.evaluating) return;
      entry.compute = noopCompute;
      entry.equals = Object.is;
      entry.stagedDeps = null;
      entry.dep0 = -1;
      entry.dep1 = -1;
      entry.depsCount = 0;
      entry.pausedCursor = 0;
      entry.topologyInvalidated = false;
      entry.core = null!;
      fn = null!;
      options = undefined;
      shell.owner = null;
      shell.core = null!;
      shell.ctx = null!;
    },
  };

  const getter = (() => readComputedValue(shell, getter)) as ReadonlySignal<T>;
  const internalComputed = getter as ReadonlySignal<T> & {
    id: number;
    __svet_cached_value: () => T;
  };
  internalComputed.id = id;
  internalComputed.__svet_cached_value = () => entry.value;
  getter.peek = () => {
    if (shell.disposed || isOwnerScopeDisposed(shell.owner) || generation !== shell.ctx.generation)
      return entry.value;
    if (!entry.evaluating) refreshForeignTracking(entry, shell.ctx);
    if (
      !entry.evaluating &&
      entry.initialized &&
      !entry.failed &&
      entry.lastCleanClock === shell.ctx.globalClock
    )
      return entry.value;
    return getWasmCore() === shell.core
      ? evaluateComputedEntry(shell)
      : runInForeignReactiveCore(shell.core, () => evaluateComputedEntry(shell));
  };

  const disposeComputed = (unregisterOwner: boolean) => {
    if (shell.disposed) return;
    shell.disposed = true;
    const currentOwner = shell.owner;
    if (unregisterOwner && currentOwner && !currentOwner.disposed) {
      const index = currentOwner.disposables.indexOf(ownerDisposer);
      if (index >= 0) currentOwner.disposables.splice(index, 1);
    }
    try {
      if (generation !== shell.ctx.generation) return;
      shell.ctx.topologyVersion++;
      retireForeignTracking(entry, true);
      retireForeignSource(getter);
      shell.ctx.computedRegistry[id] = undefined;
      disposeTrackedNode(shell.core, id);
    } finally {
      shell.detachDisposedContext();
    }
  };

  const ownerDisposer = () => disposeComputed(true);
  getter.dispose = ownerDisposer;
  entry.retire = (_self, owners) => {
    const currentOwner = shell.owner;
    if (!shell.disposed && currentOwner && !currentOwner.disposed) {
      let callbacks = owners.get(currentOwner);
      if (!callbacks) {
        callbacks = new Set();
        owners.set(currentOwner, callbacks);
      }
      callbacks.add(ownerDisposer);
      disposeComputed(false);
    } else disposeComputed(true);
  };
  if (owner) owner.disposables.push(ownerDisposer);

  return getter;
}

function recycleEffectEntry(entry: EffectEntry): void {
  const x = entry.ctx;
  entry.userFn = noopEffectUserFn;
  entry.owner = null;
  entry.cleanup = undefined;
  entry.retiredDependencies = null;
  entry.sourceRetirementVersion = 0;
  entry.topologyInvalidated = false;
  entry.pausedCursor = 0;
  releaseStagedDeps(x, entry);
  entry.depsCount = 0;
  entry.dep0 = -1;
  entry.dep1 = -1;
  if (entry.generation !== x.generation) return;
  if (x.effectPool.length < 4096) x.effectPool.push(entry);
  else if (entry.id >= 0) {
    entry.core.svet_dispose_node(entry.id);
    entry.id = -1;
  }
}

function rearmAfterCleanupFailure(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  entry: EffectEntry,
): void {
  const pending: Array<[number, number]> = [],
    seen = new Set<ComputedObserverEntry>();
  const append = (observer: EffectEntry | ComputedObserverEntry) => {
    if (observer.kind === 2 && !observer.wasmLinked) {
      if (observer.depsCount === 1) pending.push([observer.id, observer.dep0]);
      return;
    }
    const sources = readNodeSources(core, observer.id);
    for (let i = 0; i < sources.length; i++) {
      pending.push([observer.id, sources[i]!]);
    }
  };
  append(entry);
  while (pending.length) {
    const [parent, source] = pending.pop()!,
      dependency = ctx.computedRegistry[source];
    if (!dependency || seen.has(dependency) || !core.svet_depends_on(parent, source)) continue;
    seen.add(dependency);
    dependency.failed = true;
    setNodeStateFast(core, ctx, source, STATE_CLEAN);
    append(dependency);
  }
  setNodeStateFast(core, ctx, entry.id, STATE_CLEAN);
}

function drainEffectCleanups(callbacks: Array<() => void>): void {
  const pending = callbacks.splice(0);
  let failed = false,
    failure: unknown;
  for (let i = pending.length - 1; i >= 0; i--) {
    try {
      pending[i]!();
    } catch (error) {
      if (!failed) {
        failed = true;
        failure = error;
      }
    }
  }
  if (failed) throw failure;
}

function runEffectIteration(
  entry: EffectEntry,
  effectOwner: OwnerScope | null,
  c: SvetWasmExports,
  x: JsReactiveContext,
  generation: number,
  id: number,
): boolean {
  entry.rerunRequested = false;
  const version = ++entry.runVersion;
  if (effectOwner) cleanupEffectOwnerScope(effectOwner);
  const cleanup = entry.cleanup;
  entry.cleanup = undefined;
  if (cleanup) {
    try {
      untrack(cleanup);
    } catch (failure) {
      if (!entry.disposed && !isOwnerScopeDisposed(effectOwner) && generation === x.generation) {
        try {
          rearmAfterCleanupFailure(c, x, entry);
        } catch {
          /* Preserve the primary cleanup failure. */
        }
      }
      throw failure;
    }
  }
  if (entry.disposed || isOwnerScopeDisposed(effectOwner) || generation !== x.generation)
    return false;

  const prevOwner = x.currentOwner,
    prevObs = x.activeObserverId,
    prevEntry = x.activeEntry,
    prevRunning = x.isRunningEffect,
    prevCursor = x.activeCursor,
    prevChanged = x.activeTopologyChanged;
  if (prevEntry) prevEntry.pausedCursor = prevCursor;
  if (effectOwner) x.currentOwner = effectOwner;
  x.ownerDepth++;
  x.isRunningEffect = true;
  x.activeObserverId = id;
  x.activeEntry = entry;
  x.activeCursor = 0;
  x.activeTopologyChanged =
    !entry.initialized || entry.sourceRetirementVersion !== x.sourceRetirementVersion;
  entry.topologyInvalidated = false;
  entry.seenDeps = null;
  entry.initialized = true;
  let callbacks: Array<() => void> | undefined;
  const registerCleanup = (callback: () => void) => {
    if (typeof callback !== 'function') throw new TypeError('Effect cleanup must be a function');
    if (
      entry.disposed ||
      isOwnerScopeDisposed(effectOwner) ||
      generation !== x.generation ||
      entry.runVersion !== version
    ) {
      runUntrackedInCore(c, callback);
      return;
    }
    if (!callbacks) {
      callbacks = [];
      const registered = callbacks;
      entry.cleanup = () => drainEffectCleanups(registered);
    }
    callbacks.push(callback);
  };
  beginForeignTracking(entry);
  x.observerStack.push(entry);
  enterObservationContext(x);
  try {
    const ret = entry.userFn(registerCleanup);
    if (typeof ret === 'function') registerCleanup(ret);
  } finally {
    try {
      if (generation === x.generation) {
        try {
          if (!entry.disposed) commitTrackingFast(c, x, id, entry);
        } finally {
          releaseStagedDeps(x, entry);
          x.activeObserverId = prevObs;
          x.activeEntry = prevEntry;
          x.activeCursor = prevCursor;
          x.activeTopologyChanged = prevChanged || Boolean(prevEntry?.topologyInvalidated);
          if (prevEntry) prevEntry.topologyInvalidated = false;
          x.currentOwner = prevOwner;
          x.isRunningEffect = prevRunning;
          x.observerStack.pop();
          entry.retiredDependencies = null;
        }
      } else {
        releaseStagedDeps(x, entry);
        entry.retiredDependencies = null;
      }
    } finally {
      x.ownerDepth--;
      leaveObservationContext(x);
    }
  }
  return true;
}

function runEffectEntry(entry: EffectEntry): void {
  if (
    entry.disposed ||
    isOwnerScopeDisposed(entry.owner) ||
    entry.generation !== entry.ctx.generation
  )
    return;
  if (entry.running > 0) {
    entry.rerunRequested = true;
    return;
  }
  entry.running++;
  const generation = entry.generation,
    id = entry.id,
    c = entry.core,
    x = entry.ctx,
    effectOwner = entry.owner;
  try {
    let iterations = 0;
    do {
      if (iterations++ === MAX_EFFECT_ITERATIONS) throw new ReactiveStabilizationError(id);
      if (!runEffectIteration(entry, effectOwner, c, x, generation, id)) return;
    } while (
      entry.rerunRequested &&
      !entry.disposed &&
      !isOwnerScopeDisposed(effectOwner) &&
      generation === x.generation
    );
  } finally {
    entry.running--;
    if (entry.disposed && entry.running === 0) recycleEffectEntry(entry);
  }
}

function disposeEffectEntry(entry: EffectEntry): void {
  if (entry.disposed) return;
  entry.disposed = true;
  retireForeignTracking(entry, true);
  const effectOwner = entry.owner;
  if (effectOwner) {
    effectOwner.disposed = true;
    cleanupEffectOwnerScope(effectOwner);
    if (effectOwner.parent?.children) {
      effectOwner.parent.children.delete(effectOwner);
      effectOwner.parent = null;
    }
  }
  const cleanup = entry.cleanup;
  entry.cleanup = undefined;
  try {
    if (cleanup) untrack(cleanup);
  } finally {
    const x = entry.ctx,
      c = entry.core,
      id = entry.id;
    if (entry.generation === x.generation) {
      x.topologyVersion++;
      x.effectRegistry[id] = undefined;
      if (entry.wasmLinked || x.effectPool.length >= 4096) {
        if (id >= 0) {
          c.svet_dispose_node(id);
          entry.id = -1;
        }
      }
    }
    if (entry.running === 0) recycleEffectEntry(entry);
  }
}

function retireEffectOnReset(entry: EffectEntry): void {
  entry.userFn = noopEffectUserFn;
}

function createEffectEntryShell(core: SvetWasmExports, ctx: JsReactiveContext): EffectEntry {
  const entry: EffectEntry = {
    kind: 2,
    id: -1,
    core,
    ctx,
    generation: ctx.generation,
    userFn: noopEffectUserFn,
    initialized: false,
    wasmLinked: false,
    inUnlinkedList: false,
    disposed: false,
    owner: null,
    stagedDeps: null,
    dep0: -1,
    dep1: -1,
    depsCount: 0,
    running: 0,
    pausedCursor: 0,
    runVersion: 0,
    lifetime: 0,
    rerunRequested: false,
    retiredDependencies: null,
    seenDeps: null,
    retire: retireEffectOnReset,
    sourceRetirementVersion: 0,
    topologyInvalidated: false,
    run: () => runEffectEntry(entry),
    handle: noopEffectHandle,
  };
  entry.handle = {
    get id() {
      return entry.id;
    },
    dispose: () => disposeEffectEntry(entry),
  };
  return entry;
}

function createEffectPublicHandle(
  core: SvetWasmExports,
  parentOwner: OwnerScope | null,
  pooledHandle: EffectHandle,
  id: number,
): EffectHandle {
  const cell: {
    core: SvetWasmExports | null;
    parentOwner: OwnerScope | null;
    pooledHandle: EffectHandle | null;
  } = { core, parentOwner, pooledHandle };
  core = null!;
  parentOwner = null;
  pooledHandle = null!;
  const handle: EffectHandle = {
    id,
    dispose() {
      const currentCore = cell.core,
        currentOwner = cell.parentOwner,
        currentHandle = cell.pooledHandle;
      if (!currentCore || !currentHandle) return;
      cell.core = null;
      cell.parentOwner = null;
      cell.pooledHandle = null;
      runUntrackedInCore(currentCore, () => {
        if (currentOwner && !currentOwner.disposed) {
          const index = currentOwner.disposables.indexOf(handle.dispose);
          if (index >= 0) currentOwner.disposables.splice(index, 1);
        }
        currentHandle.dispose();
      });
    },
  };
  return handle;
}

/** Creates an automatically tracked side-effect. */
export function effect(
  fn: (onCleanupFn: (cleanupFn: () => void) => void) => void | (() => void),
): EffectHandle {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const parentOwner = resolveCurrentOwner(ctx, core);
  if (isOwnerScopeDisposed(parentOwner))
    throw new Error('Cannot create effect in a disposed owner');
  if (typeof fn !== 'function') throw new TypeError('Effect body must be a function');

  const effectOwner: OwnerScope = {
    disposables: [],
    children: null,
    parent: parentOwner,
    disposed: false,
    context: parentOwner?.context,
  };
  if (parentOwner) addChildScope(parentOwner, effectOwner);

  let entry = ctx.effectPool.pop();
  if (!entry) {
    entry = createEffectEntryShell(core, ctx);
    entry.id = allocateReactiveNode(core, KIND_EFFECT);
  } else {
    entry.core = core;
    entry.ctx = ctx;
    entry.generation = ctx.generation;
    entry.disposed = false;
    entry.initialized = false;
    entry.wasmLinked = false;
    entry.cleanup = undefined;
    entry.dep0 = -1;
    entry.dep1 = -1;
    entry.depsCount = 0;
    entry.stagedDeps = null;
    entry.topologyInvalidated = false;
    if (entry.id < 0) entry.id = allocateReactiveNode(core, KIND_EFFECT);
  }
  const id = entry.id;
  entry.lifetime++;
  entry.owner = effectOwner;
  entry.userFn = fn;
  ctx.effectRegistry[id] = entry;

  const handle = createEffectPublicHandle(core, parentOwner, entry.handle, id);
  if (parentOwner) parentOwner.disposables.push(handle.dispose);

  try {
    entry.run();
    if (!ctx.isRunningEffect && ctx.cascadedDuringEffect) {
      ctx.cascadedDuringEffect = false;
      flushEffects();
    }
  } catch (error) {
    try {
      handle.dispose();
    } catch {
      /* preserve the original error */
    }
    throw error;
  }
  return handle;
}

function finishBatchScope(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  generation: number,
  bodyFailed: boolean,
): void {
  let deliveryFailed = false;
  try {
    if (generation === ctx.generation) {
      ctx.batchDepth--;
      const remainingDepth = core.svet_batch_end();
      if (remainingDepth === 0) {
        if (bodyFailed) {
          try {
            flushEffects();
          } catch {
            /* Preserve the original body failure. */
          }
        } else flushEffects();
      }
    }
  } catch (error) {
    deliveryFailed = true;
    throw error;
  } finally {
    if (--batchTransport.depth === 0) {
      if (bodyFailed || deliveryFailed) {
        try {
          finishForeignBatches();
        } catch {
          /* Preserve the primary failure. */
        }
      } else finishForeignBatches();
    }
  }
}

/** Defers effect notifications until the outermost batch ends. */
export function batch<T>(fn: () => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  if (ctx.batchDepth === 0 && ctx.unlinkedEffects.length > 0) {
    flushUnlinkedEffectsToWasm(core, ctx);
  }
  const generation = ctx.generation;
  ctx.batchDepth++;
  core.svet_batch_start();
  batchTransport.depth++;
  let bodyFailed = false;
  try {
    return fn();
  } catch (error) {
    bodyFailed = true;
    throw error;
  } finally {
    finishBatchScope(core, ctx, generation, bodyFailed);
  }
}
