/** Shared reactive state, domain registries, errors, and low-level WASM helpers. */

import {
  getWasmCore,
  readNodeSources,
  STATE_CLEAN,
  type SvetWasmExports,
  withWasmCore,
} from './wasm-bridge.ts';

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
  context?: Map<symbol, unknown> | undefined;
}

export type EffectEntry = {
  kind: 2;
  id: number;
  core: SvetWasmExports;
  ctx: JsReactiveContext;
  generation: number;
  userFn: (onCleanupFn: (cleanupFn: () => void) => void) => void | (() => void);
  initialized: boolean;
  wasmLinked: boolean;
  inUnlinkedList: boolean;
  run: () => void;
  running: number;
  pausedCursor: number;
  runVersion: number;
  lifetime: number;
  retiredDependencies: Set<number> | null;
  /** Array dependencies recorded so far in the current run; built only for long lists. */
  seenDeps: Set<number> | null;
  /** Private reset hook; `resetReactiveSystem` calls it instead of a public `dispose`. */
  retire: (self: EffectEntry, owners: ReactiveResetOwnerRetirements) => void;
  sourceRetirementVersion: number;
  topologyInvalidated: boolean;
  rerunRequested: boolean;
  handle: EffectHandle;
  cleanup?: (() => void) | undefined;
  disposed: boolean;
  owner: OwnerScope | null;
  stagedDeps: number[] | null;
  dep0: number;
  dep1: number;
  depsCount: number;
};

export interface ComputedObserverEntry {
  kind: 1;
  id: number;
  core: SvetWasmExports;
  initialized: boolean;
  stagedDeps: number[] | null;
  dep0: number;
  dep1: number;
  depsCount: number;
  pausedCursor: number;
  lastCleanClock: number;
  valueVersion: number;
  evaluating: boolean;
  failed: boolean;
  retiredDependencies: Set<number> | null;
  /** Array dependencies recorded so far in the current run; built only for long lists. */
  seenDeps: Set<number> | null;
  /** Private reset hook; `resetReactiveSystem` calls it instead of a public `dispose`. */
  retire: (self: ComputedObserverEntry, owners: ReactiveResetOwnerRetirements) => void;
  sourceRetirementVersion: number;
  topologyInvalidated: boolean;
}

export type ComputedEntry<T = unknown> = ComputedObserverEntry & {
  compute: () => T;
  value: T;
  equals: (a: T, b: T) => boolean;
};

export interface AnySignalState {
  id: number;
  generation: number;
  core: SvetWasmExports;
  ctx: JsReactiveContext;
  valueVersion: number;
  disposed: boolean;
  activeWrites: number;
  /** undefined = unknown, null = multiple/none */
  singleSubEffect: EffectEntry | null | undefined;
  hasComputedSubscriber: boolean;
  readonly getter: object;
  retire: (self: AnySignalState, owners: ReactiveResetOwnerRetirements) => void;
}

export type SignalState<T = unknown> = AnySignalState & {
  equals: (a: T, b: T) => boolean;
  value: T;
  getter: Signal<T>;
};

export interface JsReactiveContext {
  coreRef?: WeakRef<SvetWasmExports>;
  foreignForwarders?: number;
  generation: number;
  signalRegistry: Array<AnySignalState | undefined>;
  effectRegistry: Array<EffectEntry | undefined>;
  computedRegistry: Array<ComputedObserverEntry | undefined>;
  observerStack: Array<EffectEntry | ComputedObserverEntry>;
  /** Live JS frames across generations; reset must not clear this count. */
  observationDepth: number;
  currentOwner: OwnerScope | null;
  /** Synchronous ownership overrides async adapter context; survives reset unwinding. */
  ownerDepth: number;
  activeObserverId: number;
  activeEntry: EffectEntry | ComputedObserverEntry | null;
  activeCursor: number;
  activeTopologyChanged: boolean;
  globalClock: number;
  topologyVersion: number;
  sourceRetirementVersion: number;
  singleSignalRootId: number;
  batchDepth: number;
  isRunningEffect: boolean;
  cascadedDuringEffect: boolean;
  statesView: Uint8Array | null;
  statesPtr: number;
  statesCap: number;
  effectPool: EffectEntry[];
  handlePool: EffectHandle[];
  unlinkedEffects: EffectEntry[];
  stagedDepsPool: number[][];
}

export type AsyncOwnerResolver = (core: SvetWasmExports) => OwnerScope | null | undefined;
export type ReactiveResetOwnerRetirements = Map<OwnerScope, Set<() => void>>;
export type RootDisposerCell = { core: SvetWasmExports | null; scope: OwnerScope | null };
export type ForeignBridge = {
  proxy: Signal<number>;
  refresh?: () => void;
  seen: boolean;
  close: () => void;
};

export type ReactiveDomains = {
  rootDisposerCells?: WeakMap<OwnerScope, RootDisposerCell>;
  batchTransport?: {
    depth: number;
    held: Map<SvetWasmExports, { ctx: JsReactiveContext; generation: number }>;
  };
  bridges?: WeakMap<object, Map<object, ForeignBridge>>;
  sourceBridges?: WeakMap<object, Set<ForeignBridge>>;
  frames: JsReactiveContext[];
  ownerResolvers?: Array<WeakRef<AsyncOwnerResolver>>;
  contexts: WeakMap<SvetWasmExports, JsReactiveContext>;
  resetCleanups: WeakMap<SvetWasmExports, Set<() => void>>;
  resetFinalizers: WeakMap<SvetWasmExports, Set<() => void>>;
  retiredCaches: WeakSet<SvetWasmExports>;
  cacheRetirements: Array<WeakRef<(core: SvetWasmExports) => void>>;
};

export const activeObservationContexts: JsReactiveContext[] = [];
const observationCopiesKey = Symbol.for('svet.reactivity.observation-copies.v2');
const observationHost = globalThis as unknown as Record<
  symbol,
  Array<WeakRef<JsReactiveContext[]>> | undefined
>;
export const observationCopies = (observationHost[observationCopiesKey] ??= []);
for (let i = observationCopies.length - 1; i >= 0; i--) {
  if (!observationCopies[i]!.deref()) observationCopies.splice(i, 1);
}
observationCopies.push(new WeakRef(activeObservationContexts));

export function enterObservationContext(ctx: JsReactiveContext): void {
  if (ctx.observationDepth++ === 0) activeObservationContexts.push(ctx);
  reactiveDomains.frames.push(ctx);
}

export function leaveObservationContext(ctx: JsReactiveContext): void {
  reactiveDomains.frames.pop();
  if (--ctx.observationDepth === 0) activeObservationContexts.pop();
}

const domainHost = globalThis as unknown as Record<symbol, ReactiveDomains | undefined>;
export const reactiveDomains = (domainHost[Symbol.for('svet.reactivity.domains.v1')] ??= {
  frames: [],
  contexts: new WeakMap(),
  resetCleanups: new WeakMap(),
  resetFinalizers: new WeakMap(),
  retiredCaches: new WeakSet(),
  cacheRetirements: [],
});

export const ownerRuntimeState = { teardownDepth: 0 };
export const retiringOwnerScopes = new WeakSet<OwnerScope>();
export const rootDisposerCells = (reactiveDomains.rootDisposerCells ??= new WeakMap<
  OwnerScope,
  RootDisposerCell
>());
export const batchTransport = (reactiveDomains.batchTransport ??= { depth: 0, held: new Map() });
export const observerBridges = (reactiveDomains.bridges ??= new WeakMap<
  object,
  Map<object, ForeignBridge>
>());
export const sourceBridges = (reactiveDomains.sourceBridges ??= new WeakMap<
  object,
  Set<ForeignBridge>
>());

const reactiveResetCleanups = reactiveDomains.resetCleanups;
const reactiveResetFinalizers = reactiveDomains.resetFinalizers;
const contextByCore = reactiveDomains.contexts;
const retiredContextCaches = reactiveDomains.retiredCaches;

let cachedCore: SvetWasmExports | null = null;
let cachedCtx: JsReactiveContext | null = null;

const retireLocalCache = (core: SvetWasmExports) => {
  if (cachedCore === core) {
    cachedCore = null;
    cachedCtx = null;
  }
};
for (let i = reactiveDomains.cacheRetirements.length - 1; i >= 0; i--) {
  if (!reactiveDomains.cacheRetirements[i]!.deref()) reactiveDomains.cacheRetirements.splice(i, 1);
}
reactiveDomains.cacheRetirements.push(new WeakRef(retireLocalCache));

/** Internal external-resource hook, retained for the lifetime of a WASM context. */
export function registerReactiveResetCleanup(core: SvetWasmExports, cleanup: () => void): void {
  let callbacks = reactiveResetCleanups.get(core);
  if (!callbacks) {
    callbacks = new Set();
    reactiveResetCleanups.set(core, callbacks);
  }
  callbacks.add(cleanup);
}

/** @internal One-shot notification after old graph storage is fully cleared. */
export function registerReactiveResetFinalizer(
  core: SvetWasmExports,
  finalize: () => void,
): () => void {
  let callbacks = reactiveResetFinalizers.get(core);
  if (!callbacks) {
    callbacks = new Set();
    reactiveResetFinalizers.set(core, callbacks);
  }
  callbacks.add(finalize);
  const registered = callbacks;
  return () => {
    registered.delete(finalize);
    if (!registered.size && reactiveResetFinalizers.get(core) === registered)
      reactiveResetFinalizers.delete(core);
  };
}

/** @internal Retired requests must not repopulate the hot cache during late cleanup. */
export function retireReactiveContextCache(core: SvetWasmExports): void {
  retiredContextCaches.add(core);
  retireLocalCache(core);
  for (const reference of reactiveDomains.cacheRetirements) {
    const retire = reference.deref();
    if (retire && retire !== retireLocalCache) retire(core);
  }
}

export function getJsContext(core: SvetWasmExports = getWasmCore()): JsReactiveContext {
  if (core === cachedCore && cachedCtx) {
    return cachedCtx;
  }
  let ctx = contextByCore.get(core);
  if (!ctx) {
    ctx = {
      generation: 0,
      signalRegistry: [],
      effectRegistry: [],
      computedRegistry: [],
      observerStack: [],
      observationDepth: 0,
      currentOwner: null,
      ownerDepth: 0,
      activeObserverId: -1,
      activeEntry: null,
      activeCursor: 0,
      activeTopologyChanged: false,
      globalClock: 1,
      topologyVersion: 1,
      sourceRetirementVersion: 1,
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
      stagedDepsPool: [],
    };
    contextByCore.set(core, ctx);
  }
  ctx.coreRef ??= new WeakRef(core);
  if (!retiredContextCaches.has(core)) {
    cachedCore = core;
    cachedCtx = ctx;
  }
  return ctx;
}

/** @internal Node adapters can resolve an owner without importing Node in this module. */
let asyncOwnerResolver: AsyncOwnerResolver | undefined;
const resolveAsyncOwnerForCopy: AsyncOwnerResolver = core => asyncOwnerResolver?.(core);
const compatibleOwnerResolvers = (reactiveDomains.ownerResolvers ??= []);
for (let i = compatibleOwnerResolvers.length - 1; i >= 0; i--) {
  if (!compatibleOwnerResolvers[i]!.deref()) compatibleOwnerResolvers.splice(i, 1);
}
compatibleOwnerResolvers.push(new WeakRef(resolveAsyncOwnerForCopy));

export function installAsyncOwnerResolver(resolve: AsyncOwnerResolver): void {
  asyncOwnerResolver = resolve;
}

export function resolveCurrentOwner(
  ctx: JsReactiveContext,
  core: SvetWasmExports,
): OwnerScope | null {
  if (ctx.ownerDepth === 0) {
    const owner = resolveAsyncOwnerForCopy(core);
    if (owner !== undefined) return owner;
    if (compatibleOwnerResolvers.length > 1)
      for (const reference of compatibleOwnerResolvers) {
        const resolve = reference.deref();
        if (!resolve || resolve === resolveAsyncOwnerForCopy) continue;
        const foreignOwner = resolve(core);
        if (foreignOwner !== undefined) return foreignOwner;
      }
  }
  return ctx.currentOwner;
}

/** @internal A native Vec may relocate without growing WebAssembly.Memory. */
export function allocateReactiveNode(core: SvetWasmExports, kind: number): number {
  const id = core.svet_create_node(kind);
  getJsContext(core).statesView = null;
  return id;
}

export function flushUnlinkedEffectsToWasm(core: SvetWasmExports, ctx: JsReactiveContext): void {
  const list = ctx.unlinkedEffects;
  const len = list.length;
  if (len === 0) return;
  for (let i = 0; i < len; i++) {
    const eff = list[i]!;
    eff.inUnlinkedList = false;
    if (!eff.disposed && !eff.wasmLinked && eff.depsCount === 1) {
      eff.wasmLinked = true;
      core.svet_link_1(eff.id, eff.dep0);
    }
  }
  list.length = 0;
}

export function flushPooledDisposalsToWasm(core: SvetWasmExports, ctx: JsReactiveContext): void {
  const pool = ctx.effectPool;
  for (let i = 0; i < pool.length; i++) {
    const eff = pool[i]!;
    if (eff.id >= 0) {
      core.svet_dispose_node(eff.id);
      eff.id = -1;
    }
  }
}

export function borrowStagedDeps(ctx: JsReactiveContext): number[] {
  const arr = ctx.stagedDepsPool.pop();
  if (arr) {
    arr.length = 0;
    return arr;
  }
  return [];
}

export function releaseStagedDeps(
  ctx: JsReactiveContext,
  entry: { stagedDeps: number[] | null },
): void {
  const staged = entry.stagedDeps;
  if (staged === null) return;
  entry.stagedDeps = null;
  staged.length = 0;
  if (ctx.stagedDepsPool.length < 16) {
    ctx.stagedDepsPool.push(staged);
  }
}

export function materializeObserverStagedDeps(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  entry: { id: number; stagedDeps: number[] | null },
  uptoCursor: number,
): number[] {
  if (entry.stagedDeps !== null) return entry.stagedDeps;
  const staged = borrowStagedDeps(ctx);
  if (uptoCursor > 2) {
    const wasmSources = readNodeSources(core, entry.id);
    const limit = Math.min(uptoCursor, wasmSources.length);
    for (let i = 2; i < limit; i++) {
      staged.push(wasmSources[i]!);
    }
  }
  entry.stagedDeps = staged;
  return staged;
}

/** @internal Source primitives must invalidate their public handle before calling this. */
export function disposeTrackedNode(core: SvetWasmExports, id: number): void {
  const ctx = getJsContext(core);
  if (ctx.sourceRetirementVersion === Number.MAX_SAFE_INTEGER) {
    ctx.sourceRetirementVersion = 0;
    for (const observer of [...ctx.effectRegistry, ...ctx.computedRegistry, ...ctx.observerStack]) {
      if (observer) observer.sourceRetirementVersion = -1;
    }
  }
  ctx.sourceRetirementVersion++;
  for (const observer of ctx.observerStack) {
    const obsCur = observer === ctx.activeEntry ? ctx.activeCursor : observer.pausedCursor;
    if (obsCur > 2 && observer.stagedDeps === null) {
      materializeObserverStagedDeps(observer.core, ctx, observer, obsCur);
    }
    observer.topologyInvalidated = true;
    if (observer === ctx.activeEntry) ctx.activeTopologyChanged = true;
    (observer.retiredDependencies ??= new Set()).add(id);
  }
  core.svet_dispose_node(id);
}

export function setNodeStateFast(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  id: number,
  state: number,
): void {
  let view = ctx.statesView;
  if (!view || view.buffer !== core.memory.buffer || id >= ctx.statesCap) {
    getNodeStateFast(core, ctx, id);
    view = ctx.statesView;
  }
  if (view && id < ctx.statesCap) {
    view[id] = state;
  }
}

export function getNodeStateFast(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  id: number,
): number {
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
  return view[id]!;
}

/** True when nothing in any realm is collecting reads. */
export function isObservationIdle(ctx: JsReactiveContext): boolean {
  if (ctx.activeObserverId >= 0 || ctx.activeEntry) return false;
  for (const other of activeObservationContexts) if (other.activeObserverId >= 0) return false;
  if (observationCopies.length > 1)
    for (const reference of observationCopies) {
      const contexts = reference.deref();
      if (!contexts || contexts === activeObservationContexts) continue;
      for (const other of contexts) if (other.activeObserverId >= 0) return false;
    }
  return true;
}

/** Internal lifetime token used by non-scalar WASM-backed primitives. */
export function getReactiveGeneration(core: SvetWasmExports = getWasmCore()): number {
  return getJsContext(core).generation;
}

export function invalidateSingleSignalFastPath(): void {
  const ctx = getJsContext();
  ctx.singleSignalRootId = -2;
}

export function retireNothing(): void {}

export class ReactiveContextDisposedError extends Error {
  readonly code = 'SVET_CONTEXT_DISPOSED';
  constructor() {
    super('Captured reactive context is no longer active');
    this.name = 'ReactiveContextDisposedError';
  }
}

export class ReactiveCycleError extends Error {
  readonly nodeIds: readonly number[];
  constructor(nodeIds: readonly number[]) {
    super(`Cyclic computed dependency: ${nodeIds.join(' -> ')}`);
    this.name = 'ReactiveCycleError';
    this.nodeIds = Object.freeze([...nodeIds]);
  }
}

export const MAX_EFFECT_ITERATIONS = 100;

export class ReactiveStabilizationError extends Error {
  constructor(
    readonly effectId: number | null,
    readonly iterations = MAX_EFFECT_ITERATIONS,
  ) {
    super(
      effectId === null
        ? `Reactive graph did not stabilize within ${iterations} queued runs per effect`
        : `Effect ${effectId} did not stabilize within ${iterations} synchronous runs`,
    );
    this.name = 'ReactiveStabilizationError';
  }
}

/** Resets the entire WASM graph and JS registries. */
export function resetReactiveSystem(): void {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const retiredOwners: ReactiveResetOwnerRetirements = new Map();
  const retiring: Array<() => void> = [];
  for (const entry of ctx.signalRegistry)
    if (entry) retiring.push(() => entry.retire(entry, retiredOwners));
  for (const entry of ctx.computedRegistry)
    if (entry) retiring.push(() => entry.retire(entry, retiredOwners));
  for (const entry of ctx.effectRegistry)
    if (entry) retiring.push(() => entry.retire(entry, retiredOwners));
  ctx.generation++;
  for (const retire of retiring) retire();
  for (const [owner, retired] of retiredOwners) {
    const callbacks = owner.disposables;
    let write = 0;
    for (let read = 0; read < callbacks.length; read++) {
      const callback = callbacks[read]!;
      if (!retired.has(callback)) callbacks[write++] = callback;
    }
    callbacks.length = write;
  }
  for (const cleanup of reactiveResetCleanups.get(core) ?? []) {
    try {
      cleanup();
    } catch {
      /* Reset must still invalidate graph storage. */
    }
  }
  core.svet_reset();
  ctx.signalRegistry.length = 0;
  ctx.effectRegistry.length = 0;
  ctx.computedRegistry.length = 0;
  ctx.observerStack.length = 0;
  ctx.currentOwner = null;
  ctx.activeObserverId = -1;
  ctx.activeEntry = null;
  ctx.activeCursor = 0;
  ctx.activeTopologyChanged = false;
  ctx.globalClock = 1;
  ctx.topologyVersion = 1;
  ctx.sourceRetirementVersion = 1;
  ctx.singleSignalRootId = -1;
  ctx.batchDepth = 0;
  ctx.isRunningEffect = false;
  ctx.cascadedDuringEffect = false;
  ctx.statesView = null;
  ctx.statesPtr = 0;
  ctx.statesCap = 0;
  ctx.effectPool.length = 0;
  ctx.unlinkedEffects.length = 0;
  ctx.stagedDepsPool.length = 0;
  const finalizers = reactiveResetFinalizers.get(core);
  reactiveResetFinalizers.delete(core);
  if (finalizers)
    withWasmCore(core, () => {
      for (const finalize of finalizers) {
        try {
          finalize();
        } catch {
          /* Continue notifying other retired requests. */
        }
      }
    });
}

/** Returns live telemetry from the WASM reactive graph. */
export function getGraphTelemetry(): {
  nodes: number;
  capacity: number;
  edges: number;
  wasmMemoryBytes: number;
  pendingEffectCount: number;
  pendingEffectEntryCount: number;
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
    pendingEffectCount: core.svet_pending_effect_count(),
    pendingEffectEntryCount: core.svet_pending_effect_entry_count(),
  };
}
