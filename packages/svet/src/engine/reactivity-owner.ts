/** Hierarchical owner scopes, async context capture, and untracked execution. */

import {
  activeObservationContexts,
  type ComputedObserverEntry,
  type EffectEntry,
  getJsContext,
  getReactiveGeneration,
  isObservationIdle,
  type JsReactiveContext,
  observationCopies,
  ownerRuntimeState,
  type OwnerScope,
  ReactiveContextDisposedError,
  resetReactiveSystem,
  resolveCurrentOwner,
  retiringOwnerScopes,
  rootDisposerCells,
} from './reactivity-context.ts';
import {
  createWasmCoreInstance,
  getWasmCore,
  type SvetWasmExports,
  withWasmCore,
} from './wasm-bridge.ts';

export function untrack<T>(fn: () => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  if (isObservationIdle(ctx)) return fn();
  const prevObs = ctx.activeObserverId;
  const prevEntry = ctx.activeEntry;
  if (prevEntry?.kind === 2) prevEntry.pausedCursor = ctx.activeCursor;
  const generation = ctx.generation;
  ctx.activeObserverId = -1;
  ctx.activeEntry = null;
  let suspended:
    | Array<{
        ctx: JsReactiveContext;
        generation: number;
        id: number;
        entry: EffectEntry | ComputedObserverEntry | null;
      }>
    | undefined;
  for (const other of activeObservationContexts) {
    if (other === ctx || other.activeObserverId < 0) continue;
    (suspended ??= []).push({
      ctx: other,
      generation: other.generation,
      id: other.activeObserverId,
      entry: other.activeEntry,
    });
    if (other.activeEntry?.kind === 2) other.activeEntry.pausedCursor = other.activeCursor;
    other.activeObserverId = -1;
    other.activeEntry = null;
  }
  if (observationCopies.length > 1)
    for (const reference of observationCopies) {
      const contexts = reference.deref();
      if (!contexts || contexts === activeObservationContexts) continue;
      for (const other of contexts) {
        if (other.activeObserverId < 0) continue;
        (suspended ??= []).push({
          ctx: other,
          generation: other.generation,
          id: other.activeObserverId,
          entry: other.activeEntry,
        });
        if (other.activeEntry?.kind === 2) other.activeEntry.pausedCursor = other.activeCursor;
        other.activeObserverId = -1;
        other.activeEntry = null;
      }
    }
  try {
    return fn();
  } finally {
    if (generation === ctx.generation) {
      ctx.activeObserverId = prevObs;
      ctx.activeEntry = prevEntry;
    }
    if (suspended)
      for (let i = suspended.length - 1; i >= 0; i--) {
        const saved = suspended[i]!;
        if (saved.generation === saved.ctx.generation) {
          saved.ctx.activeObserverId = saved.id;
          saved.ctx.activeEntry = saved.entry;
        }
      }
  }
}

/** Foreign reactive callbacks cannot collect reads into paused callers. */
export function runInForeignReactiveCore<T>(core: SvetWasmExports, fn: () => T): T {
  return untrack(() => withWasmCore(core, () => untrack(fn)));
}

/** @internal Pause all existing observer frames; captured work can outlive an ambient request. */
export function runUntrackedInCore<T>(core: SvetWasmExports, fn: () => T): T {
  let ambient: SvetWasmExports | undefined;
  try {
    ambient = getWasmCore();
  } catch {
    /* The explicitly selected core may still be live. */
  }
  if (ambient === core && isObservationIdle(getJsContext(core))) return fn();
  return withWasmCore(core, () => untrack(fn));
}

function finishOwnerScope(current: OwnerScope): void {
  current.children?.clear();
  for (let i = current.disposables.length - 1; i >= 0; i--) {
    try {
      current.disposables[i]!();
    } catch {
      /* Continue teardown after cleanup failure. */
    }
  }
  current.disposables.length = 0;
  const rootCell = current.parent === null ? rootDisposerCells.get(current) : undefined;
  if (rootCell) {
    rootCell.core = null;
    rootCell.scope = null;
    rootDisposerCells.delete(current);
  }
  if (current.parent?.children) {
    current.parent.children.delete(current);
    current.parent = null;
  }
}

export function disposeOwnerScope(scope: OwnerScope): void {
  if (scope.disposed) return;
  if (!scope.children || scope.children.size === 0) {
    const ctx = getJsContext();
    if (isObservationIdle(ctx)) {
      ownerRuntimeState.teardownDepth++;
      try {
        scope.disposed = true;
        finishOwnerScope(scope);
      } finally {
        ownerRuntimeState.teardownDepth--;
      }
      return;
    }
  }
  return untrack(() => {
    ownerRuntimeState.teardownDepth++;
    try {
      scope.disposed = true;
      const frames: Array<{ scope: OwnerScope; children: Iterator<OwnerScope> | undefined }> = [
        { scope, children: scope.children?.values() },
      ];
      while (frames.length > 0) {
        const frame = frames[frames.length - 1]!;
        const next = frame.children?.next();
        if (next && !next.done) {
          const child = next.value;
          if (!child.disposed) {
            child.disposed = true;
            frames.push({ scope: child, children: child.children?.values() });
          }
          continue;
        }
        finishOwnerScope(frame.scope);
        frames.pop();
      }
    } finally {
      ownerRuntimeState.teardownDepth--;
    }
  });
}

export function cleanupEffectOwnerScope(effectOwner: OwnerScope | null): void {
  if (!effectOwner || (!effectOwner.children?.size && effectOwner.disposables.length === 0)) return;
  return untrack(() => {
    const alreadyRetiring = retiringOwnerScopes.has(effectOwner);
    ownerRuntimeState.teardownDepth++;
    try {
      retiringOwnerScopes.add(effectOwner);
      if (effectOwner.children && effectOwner.children.size > 0) {
        for (const child of effectOwner.children) {
          disposeOwnerScope(child);
        }
        effectOwner.children.clear();
      }
      if (effectOwner.disposables.length > 0) {
        const disposables = effectOwner.disposables.splice(0);
        for (let i = disposables.length - 1; i >= 0; i--) {
          try {
            disposables[i]!();
          } catch {
            // ignore
          }
        }
      }
    } finally {
      if (!alreadyRetiring) retiringOwnerScopes.delete(effectOwner);
      ownerRuntimeState.teardownDepth--;
    }
  });
}

export function addChildScope(parent: OwnerScope, child: OwnerScope): void {
  if (!parent.children) {
    parent.children = new Set();
  }
  parent.children.add(child);
}

function createRootDisposer(core: SvetWasmExports, scope: OwnerScope): () => void {
  const cell = { core, scope };
  rootDisposerCells.set(scope, cell);
  core = null!;
  scope = null!;
  return () => {
    const currentScope = cell.scope,
      currentCore = cell.core;
    if (!currentScope || !currentCore || currentScope.disposed) return;
    runUntrackedInCore(currentCore, () => disposeOwnerScope(currentScope));
  };
}

/** Creates a hierarchical ownership root (`createRoot`). */
export function createRoot<T>(fn: (dispose: () => void) => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const previousStoredOwner = ctx.currentOwner;
  const prevOwner = resolveCurrentOwner(ctx, core);
  const generation = ctx.generation;

  const scope: OwnerScope = {
    disposables: [],
    children: null,
    parent: null,
    disposed: false,
    context: prevOwner?.context ? new Map(prevOwner.context) : undefined,
  };

  ctx.currentOwner = scope;
  ctx.ownerDepth++;
  try {
    return fn(createRootDisposer(core, scope));
  } catch (error) {
    disposeOwnerScope(scope);
    throw error;
  } finally {
    ctx.ownerDepth--;
    if (generation === ctx.generation) ctx.currentOwner = previousStoredOwner;
  }
}

/** @internal Runs `fn` in a new parentless scope, returning the scope to the caller. */
export function runInDetachedScope<T>(fn: (scope: OwnerScope) => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const previousStoredOwner = ctx.currentOwner;
  const prevOwner = resolveCurrentOwner(ctx, core);
  const generation = ctx.generation;
  const scope: OwnerScope = {
    disposables: [],
    children: null,
    parent: null,
    disposed: false,
    context: prevOwner?.context ? new Map(prevOwner.context) : undefined,
  };
  ctx.currentOwner = scope;
  ctx.ownerDepth++;
  try {
    return fn(scope);
  } catch (error) {
    disposeOwnerScope(scope);
    throw error;
  } finally {
    ctx.ownerDepth--;
    if (generation === ctx.generation) ctx.currentOwner = previousStoredOwner;
  }
}

/** @internal Disposes a scope from `runInDetachedScope` in the core that created it. */
export function disposeDetachedScope(core: SvetWasmExports, scope: OwnerScope): void {
  if (scope.disposed) return;
  runUntrackedInCore(core, () => disposeOwnerScope(scope));
}

/** @internal True when the running owner scope has collected no cleanups or child scopes. */
export function currentOwnerIsEmpty(): boolean {
  const owner = getCurrentOwner();
  return !owner || (owner.disposables.length === 0 && !owner.children?.size);
}

/** @internal Moves everything the running owner scope collected into a new root. */
export function adoptCurrentOwnerIntoRoot<T>(fn: (dispose: () => void) => T): T {
  const source = getCurrentOwner();
  return createRoot(dispose => {
    const target = getCurrentOwner();
    if (source && target && source !== target) {
      if (source.children) {
        for (const child of source.children) {
          child.parent = target;
          addChildScope(target, child);
        }
        source.children.clear();
      }
      if (source.disposables.length > 0) {
        target.disposables.push(...source.disposables);
        source.disposables.length = 0;
      }
    }
    return fn(dispose);
  });
}

/** Registers a cleanup callback on the current active `OwnerScope`. */
export function onCleanup(cleanupFn: () => void): void {
  const owner = getCurrentOwner();
  if (owner && !owner.disposed) owner.disposables.push(cleanupFn);
}

/** @internal A descendant is not executable while any ancestor is stopping. */
export function isOwnerScopeDisposed(owner: OwnerScope | null | undefined): boolean {
  if (!owner) return false;
  if (owner.disposed) return true;
  if (ownerRuntimeState.teardownDepth === 0) return false;
  for (let current: OwnerScope | null = owner; current; current = current.parent) {
    if (current.disposed || retiringOwnerScopes.has(current)) return true;
  }
  return false;
}

/** Returns the currently active `OwnerScope` (or `null` if outside a root/effect). */
export function getCurrentOwner(): OwnerScope | null {
  const core = getWasmCore();
  return resolveCurrentOwner(getJsContext(core), core);
}

export interface CapturedReactiveContext {
  readonly active: boolean;
  run<T>(callback: () => T): T;
  dispose(): void;
}

type CapturedContextGroup = {
  first: (() => void) | undefined;
  pending: Set<() => void> | undefined;
  slot: number;
  draining: boolean;
  cleanup: () => void;
};
const capturedContextGroups = new WeakMap<OwnerScope, CapturedContextGroup>();
function getCapturedContextGroup(owner: OwnerScope): CapturedContextGroup {
  const existing = capturedContextGroups.get(owner);
  if (existing) return existing;
  const group: CapturedContextGroup = {
    first: undefined,
    pending: undefined,
    slot: -1,
    draining: false,
    cleanup() {
      if (capturedContextGroups.get(owner) === group) capturedContextGroups.delete(owner);
      group.draining = true;
      try {
        group.first?.();
        if (group.pending) for (const dispose of group.pending) dispose();
      } finally {
        group.first = undefined;
        group.pending?.clear();
        group.draining = false;
      }
    },
  };
  capturedContextGroups.set(owner, group);
  return group;
}

type CapturedContextState = {
  core: SvetWasmExports;
  owner: OwnerScope;
  generation: number;
  group: CapturedContextGroup;
};
function isCapturedContextActive(
  state: CapturedContextState | undefined,
): state is CapturedContextState {
  return (
    !!state &&
    state.generation === getReactiveGeneration(state.core) &&
    !isOwnerScopeDisposed(state.owner)
  );
}

/** Capture an owned lifetime for explicit synchronous reentry after async work. */
export function captureReactiveContext(): CapturedReactiveContext {
  const core = getWasmCore(),
    ctx = getJsContext(core),
    owner = resolveCurrentOwner(ctx, core);
  if (!owner) throw new Error('captureReactiveContext requires an active owner');
  if (isOwnerScopeDisposed(owner)) throw new ReactiveContextDisposedError();
  const group = getCapturedContextGroup(owner);
  let captured: CapturedContextState | undefined = {
    core,
    owner,
    generation: ctx.generation,
    group,
  };
  const dispose = () => {
    const state = captured;
    captured = undefined;
    if (!state || state.group.draining) return;
    const g = state.group;
    if (g.first === dispose) g.first = undefined;
    else g.pending?.delete(dispose);
    if (!g.first && !g.pending?.size) {
      g.pending?.clear();
      if (!state.owner.disposed) {
        const callbacks = state.owner.disposables;
        const index = callbacks[g.slot] === g.cleanup ? g.slot : callbacks.indexOf(g.cleanup);
        if (index >= 0) callbacks.splice(index, 1);
      }
    }
  };
  if (group.first) {
    const pending = (group.pending ??= new Set());
    pending.add(group.first);
    pending.add(dispose);
    group.first = undefined;
  } else if (group.pending?.size) group.pending.add(dispose);
  else {
    group.first = dispose;
    group.slot = owner.disposables.length;
    owner.disposables.push(group.cleanup);
  }
  return {
    get active() {
      return isCapturedContextActive(captured);
    },
    run<T>(callback: () => T): T {
      const state = captured;
      if (!isCapturedContextActive(state)) throw new ReactiveContextDisposedError();
      if (typeof callback !== 'function')
        throw new TypeError('Reactive context callback must be a function');
      return runUntrackedInCore(state.core, () => {
        if (captured !== state || !isCapturedContextActive(state))
          throw new ReactiveContextDisposedError();
        return runWithCapturedOwner(state.core, state.owner, callback);
      });
    },
    dispose,
  };
}

/** Internal async adapter re-entry after the caller verifies its context generation. */
export function runWithCapturedOwner<T>(
  core: SvetWasmExports,
  owner: OwnerScope | null,
  fn: () => T,
): T {
  return withWasmCore(core, () => {
    const ctx = getJsContext(core),
      previous = ctx.currentOwner,
      generation = ctx.generation;
    if (isOwnerScopeDisposed(owner)) throw new Error('Reactive owner has been disposed');
    ctx.currentOwner = owner;
    ctx.ownerDepth++;
    try {
      return fn();
    } finally {
      ctx.ownerDepth--;
      if (generation === ctx.generation) ctx.currentOwner = previous;
    }
  });
}

/** Runs `fn` within a child owner that inherits context values from its parent. */
export function runWithOwnerScope<T>(
  contextEntries: Array<[symbol, unknown]>,
  fn: (dispose: () => void) => T,
): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const previousStoredOwner = ctx.currentOwner;
  const prevOwner = resolveCurrentOwner(ctx, core);
  if (isOwnerScopeDisposed(prevOwner))
    throw new Error('Cannot create child scope in a disposed owner');
  const generation = ctx.generation;

  const contextMap = new Map<symbol, unknown>(prevOwner?.context);
  for (const [k, v] of contextEntries) {
    contextMap.set(k, v);
  }
  if (generation !== ctx.generation)
    throw new Error('Reactive graph reset during owner scope construction');
  if (isOwnerScopeDisposed(prevOwner))
    throw new Error('Cannot create child scope in a disposed owner');

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
  ctx.ownerDepth++;
  try {
    return fn(() => runUntrackedInCore(core, () => disposeOwnerScope(scope)));
  } catch (error) {
    disposeOwnerScope(scope);
    throw error;
  } finally {
    ctx.ownerDepth--;
    if (generation === ctx.generation) ctx.currentOwner = previousStoredOwner;
  }
}

/** Looks up a value by symbol key up the `OwnerScope` chain. */
export function lookupOwnerContext<T>(key: symbol, defaultValue: T): T {
  let curr = getCurrentOwner();
  while (curr) {
    if (curr.context && curr.context.has(key)) {
      return curr.context.get(key) as T;
    }
    curr = curr.parent;
  }
  return defaultValue;
}

/** Runs `fn` inside an isolated WASM instance and JS reactive context (sync only). */
export function runWithIsolatedContext<T>(fn: () => T): T {
  const isolatedCore = createWasmCoreInstance();
  return withWasmCore(isolatedCore, () => {
    return createRoot(() => fn());
  });
}

/** Internal server adapter entry: use only with a new request-local WASM core. */
export async function runWithAsyncRequestScope<T>(fn: () => T | PromiseLike<T>): Promise<T> {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const previousOwner = ctx.currentOwner;
  const scope: OwnerScope = {
    disposables: [],
    children: null,
    parent: null,
    disposed: false,
  };
  ctx.currentOwner = scope;
  try {
    return await fn();
  } finally {
    withWasmCore(core, () => {
      try {
        disposeOwnerScope(scope);
      } finally {
        resetReactiveSystem();
        ctx.currentOwner = previousOwner;
      }
    });
  }
}
