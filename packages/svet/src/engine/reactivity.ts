/**
 * # Svet Reactive Primitives (`reactivity.ts`)
 *
 * Includes:
 * 1. **WASM Free-List Slot Recycling**: `dispose()` returns the node ID to the Rust WASM `free_list`
 *    so long-running SPAs reuse memory slots with 0 unbounded growth.
 * 2. **Hierarchical Owner/Scope Tree (`createRoot`, `onCleanup`)**: Any `effect` or `computed`
 *    created inside a `<Show>`, `<For>`, or `createRoot` scope automatically registers with its
 *    parent `Owner` and is disposed when the branch/item unmounts!
 * 3. **Zero-Allocation `untrack()`**: Temporarily pauses host read tracking
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
  context?: Map<symbol, any> | undefined;
}

type EffectEntry = {
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
  sourceRetirementVersion: number;
  rerunRequested: boolean;
  handle: EffectHandle;
  cleanup?: (() => void) | undefined;
  disposed: boolean;
  owner: OwnerScope | null;
  deps: number[];
  dep0: number;
  dep1: number;
  depsCount: number;
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
  valueVersion: number;
  evaluating: boolean;
  failed: boolean;
  retiredDependencies: Set<number> | null;
  /** Array dependencies recorded so far in the current run; built only for long lists. */
  seenDeps: Set<number> | null;
  sourceRetirementVersion: number;
};

interface JsReactiveContext {
  coreRef?: WeakRef<SvetWasmExports>;
  foreignForwarders?: number;
  generation: number;
  signalRegistry: Array<any>;
  effectRegistry: Array<EffectEntry | undefined>;
  computedRegistry: Array<ComputedEntry<any> | undefined>;
  observerStack: Array<EffectEntry | ComputedEntry<any>>;
  /** Live JS frames across generations; reset must not clear this count. */
  observationDepth: number;
  currentOwner: OwnerScope | null;
  /** Synchronous ownership overrides async adapter context; survives reset unwinding. */
  ownerDepth: number;
  activeObserverId: number;
  activeEntry: EffectEntry | ComputedEntry<any> | null;
  activeCursor: number;
  activeTopologyChanged: boolean;
  globalClock: number;
  topologyVersion: number;
  sourceRetirementVersion: number;
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

// Each synchronously observed core appears once, independent of nesting depth.
// Keep the per-copy observation stack local on the hot enter/leave path.
// Compatible copies discover one another weakly only when suppressing reads.
const activeObservationContexts: JsReactiveContext[] = [];
const observationCopiesKey = Symbol.for('svet.reactivity.observation-copies.v2');
const observationHost = globalThis as unknown as Record<
  symbol,
  Array<WeakRef<JsReactiveContext[]>> | undefined
>;
const observationCopies = (observationHost[observationCopiesKey] ??= []);
for (let i = observationCopies.length - 1; i >= 0; i--) {
  if (!observationCopies[i].deref()) observationCopies.splice(i, 1);
}
observationCopies.push(new WeakRef(activeObservationContexts));
function enterObservationContext(ctx: JsReactiveContext): void {
  if (ctx.observationDepth++ === 0) activeObservationContexts.push(ctx);
  reactiveDomains.frames.push(ctx);
}
function leaveObservationContext(ctx: JsReactiveContext): void {
  reactiveDomains.frames.pop();
  if (--ctx.observationDepth === 0) activeObservationContexts.pop();
}
type AsyncOwnerResolver = (core: SvetWasmExports) => OwnerScope | null | undefined;
type ReactiveResetOwnerRetirements = Map<OwnerScope, Set<() => void>>;
type ReactiveHandleRetirement = (owners?: ReactiveResetOwnerRetirements) => void;

type ReactiveDomains = {
  handleRetirements?: WeakMap<object, ReactiveHandleRetirement>;
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
// Node IDs and generations belong to a native core, not to a module copy.
const domainHost = globalThis as unknown as Record<symbol, ReactiveDomains | undefined>;
const reactiveDomains = (domainHost[Symbol.for('svet.reactivity.domains.v1')] ??= {
  frames: [],
  contexts: new WeakMap(),
  resetCleanups: new WeakMap(),
  resetFinalizers: new WeakMap(),
  retiredCaches: new WeakSet(),
  cacheRetirements: [],
});
const reactiveResetCleanups = reactiveDomains.resetCleanups;
/** Internal external-resource hook, retained for the lifetime of a WASM context. */
export function registerReactiveResetCleanup(core: SvetWasmExports, cleanup: () => void): void {
  let callbacks = reactiveResetCleanups.get(core);
  if (!callbacks) {
    callbacks = new Set();
    reactiveResetCleanups.set(core, callbacks);
  }
  callbacks.add(cleanup);
}

const reactiveResetFinalizers = reactiveDomains.resetFinalizers;
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

const contextByCore = reactiveDomains.contexts;
let cachedCore: SvetWasmExports | null = null;
let cachedCtx: JsReactiveContext | null = null;
const retiredContextCaches = reactiveDomains.retiredCaches;
const retireLocalCache = (core: SvetWasmExports) => {
  if (cachedCore === core) {
    cachedCore = null;
    cachedCtx = null;
  }
};
for (let i = reactiveDomains.cacheRetirements.length - 1; i >= 0; i--) {
  if (!reactiveDomains.cacheRetirements[i].deref()) reactiveDomains.cacheRetirements.splice(i, 1);
}
reactiveDomains.cacheRetirements.push(new WeakRef(retireLocalCache));

/** @internal Retired requests must not repopulate the hot cache during late cleanup. */
export function retireReactiveContextCache(core: SvetWasmExports): void {
  retiredContextCaches.add(core);
  retireLocalCache(core);
  for (const reference of reactiveDomains.cacheRetirements) {
    const retire = reference.deref();
    if (retire && retire !== retireLocalCache) retire(core);
  }
}

function getJsContext(core: SvetWasmExports = getWasmCore()): JsReactiveContext {
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
  if (!compatibleOwnerResolvers[i].deref()) compatibleOwnerResolvers.splice(i, 1);
}
compatibleOwnerResolvers.push(new WeakRef(resolveAsyncOwnerForCopy));
export function installAsyncOwnerResolver(resolve: AsyncOwnerResolver): void {
  asyncOwnerResolver = resolve;
}
function resolveCurrentOwner(ctx: JsReactiveContext, core: SvetWasmExports): OwnerScope | null {
  if (ctx.ownerDepth === 0) {
    const owner = resolveAsyncOwnerForCopy(core);
    if (owner !== undefined) return owner;
    if (compatibleOwnerResolvers.length > 1)
      for (const reference of compatibleOwnerResolvers) {
        const resolve = reference.deref();
        if (!resolve || resolve === resolveAsyncOwnerForCopy) continue;
        // Each host resolver validates core/request identity before resolving an
        // owner or refusing an expired child. Undefined permits the next copy.
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
    (observer.retiredDependencies ??= new Set()).add(id);
  }
  core.svet_dispose_node(id);
}

// Compatible copies share one synchronous batch boundary. Native batches keep
// writes immediately readable while postponing observer delivery in every core.
const batchTransport = (reactiveDomains.batchTransport ??= { depth: 0, held: new Map() });
function holdForeignBatch(core: SvetWasmExports, ctx: JsReactiveContext): void {
  const held = batchTransport.held.get(core);
  if (held && held.generation !== ctx.generation) batchTransport.held.delete(core);
  if (
    !batchTransport.depth ||
    ctx.batchDepth > 0 ||
    ctx.isRunningEffect ||
    batchTransport.held.has(core)
  )
    return;
  flushUnlinkedEffectsToWasm(core, ctx);
  core.svet_batch_start();
  ctx.batchDepth++;
  batchTransport.held.set(core, { ctx, generation: ctx.generation });
}
function finishForeignBatches(): void {
  let failed = false,
    failure: unknown;
  const deliveries = new Map<SvetWasmExports, number>();
  // Keep transport active while delivery creates downstream writes/bridges.
  batchTransport.depth++;
  try {
    while (batchTransport.held.size) {
      const [core, record] = batchTransport.held.entries().next().value!;
      batchTransport.held.delete(core);
      const { ctx, generation } = record;
      if (generation !== ctx.generation) continue;
      try {
        let remainingDepth: number;
        try {
          // Fan-in order may release this core before an upstream core. Resolve
          // foreign boundaries while its native batch still suppresses delivery,
          // so observers collect the final snapshot without self-invalidating.
          withWasmCore(core, () => {
            const seen = new Set<object>();
            for (const observer of ctx.effectRegistry) {
              if (observer && !observer.disposed) refreshForeignTracking(observer, ctx, seen);
            }
          });
        } finally {
          if (generation === ctx.generation) {
            ctx.batchDepth--;
            remainingDepth = core.svet_batch_end();
          } else remainingDepth = -1;
        }
        if (remainingDepth === 0) {
          const count = (deliveries.get(core) ?? 0) + 1;
          deliveries.set(core, count);
          if (count > MAX_EFFECT_ITERATIONS) throw new ReactiveStabilizationError(null);
          withWasmCore(core, () => flushEffects());
        }
      } catch (error) {
        if (!failed) {
          failed = true;
          failure = error;
        }
      }
    }
  } finally {
    batchTransport.depth--;
  }
  if (failed) throw failure;
}

type ForeignBridge = {
  proxy: Signal<number>;
  refresh?: () => void;
  seen: boolean;
  close: () => void;
};
const observerBridges = (reactiveDomains.bridges ??= new WeakMap<
  object,
  Map<object, ForeignBridge>
>());
const sourceBridges = (reactiveDomains.sourceBridges ??= new WeakMap<object, Set<ForeignBridge>>());
function refreshForeignTracking(
  entry: Pick<ComputedEntry, 'depsCount' | 'dep0' | 'dep1' | 'deps'>,
  ctx: JsReactiveContext,
  seen?: Set<object>,
): void {
  if (!batchTransport.depth || seen?.has(entry)) return;
  seen ??= new Set();
  seen.add(entry);
  for (const bridge of observerBridges.get(entry)?.values() ?? []) bridge.refresh?.();
  // An outer cached computed may only have local native computed dependencies.
  // Refresh their foreign boundaries before trusting this core's clock/state.
  for (let i = 0; i < entry.depsCount; i++) {
    const id = i === 0 ? entry.dep0 : i === 1 ? entry.dep1 : entry.deps[i - 2];
    const dependency = ctx.computedRegistry[id];
    if (dependency) refreshForeignTracking(dependency, ctx, seen);
  }
}
function beginForeignTracking(entry: object): void {
  for (const bridge of observerBridges.get(entry)?.values() ?? []) bridge.seen = false;
}
function retireForeignTracking(entry: object, all = false): void {
  for (const bridge of [...(observerBridges.get(entry)?.values() ?? [])]) {
    if (all || !bridge.seen) bridge.close();
  }
}
function retireForeignSource(source: object): void {
  for (const bridge of [...(sourceBridges.get(source) ?? [])]) bridge.close();
}
/** Native graphs own both dependency edges; the host only forwards source changes. */
function trackForeignSource<T>(
  sourceCore: SvetWasmExports,
  source: ReadonlySignal<T>,
  sourceVersion?: () => number,
): void {
  const target = reactiveDomains.frames.at(-1);
  if (!target?.activeEntry || target.activeObserverId < 0) return;
  const targetCore = target.coreRef?.deref();
  if (!targetCore || targetCore === sourceCore) return;
  const observer = target.activeEntry;
  let dependencies = observerBridges.get(observer);
  if (!dependencies) {
    dependencies = new Map();
    observerBridges.set(observer, dependencies);
  }
  let bridge = dependencies.get(source);
  if (!bridge) {
    const proxy = withWasmCore(targetCore, () => signal(0));
    const sourceCtx = getJsContext(sourceCore);
    sourceCtx.foreignForwarders = (sourceCtx.foreignForwarders ?? 0) + 1;
    let forwarding: EffectHandle | undefined,
      closed = false;
    const unregister: Array<() => void> = [];
    let initialized = false,
      previousVersion: number | undefined;
    let previousFailed = false,
      failureClock = -1;
    const publish = () => {
      if (closed) return;
      // The forwarding observer must survive a failed computed evaluation. Its
      // target owns error handling and reads/throws through the original getter.
      let failed = false;
      try {
        source();
      } catch {
        failed = true;
      }
      const version = sourceVersion?.();
      const changed =
        initialized &&
        (failed
          ? !previousFailed || failureClock !== sourceCtx.globalClock
          : previousFailed || !sourceVersion || previousVersion !== version);
      if (failed) failureClock = sourceCtx.globalClock;
      previousFailed = failed;
      previousVersion = version;
      initialized = true;
      if (changed) proxy.update(version => version + 1);
    };
    bridge = {
      refresh: () => runInForeignReactiveCore(sourceCore, publish),
      proxy,
      seen: true,
      close: () => {
        if (closed) return;
        closed = true;
        sourceCtx.foreignForwarders = Math.max(0, (sourceCtx.foreignForwarders ?? 1) - 1);
        dependencies!.delete(source);
        if (!dependencies!.size) observerBridges.delete(observer);
        const subscribers = sourceBridges.get(source);
        subscribers?.delete(bridge!);
        if (!subscribers?.size) sourceBridges.delete(source);
        for (const remove of unregister) remove();
        try {
          forwarding?.dispose();
        } finally {
          proxy.dispose();
        }
      },
    };
    dependencies.set(source, bridge);
    let subscribers = sourceBridges.get(source);
    if (!subscribers) {
      subscribers = new Set();
      sourceBridges.set(source, subscribers);
    }
    subscribers.add(bridge);
    unregister.push(
      registerReactiveResetFinalizer(sourceCore, bridge.close),
      registerReactiveResetFinalizer(targetCore, bridge.close),
    );
    try {
      // A bridge belongs to its dependency, not a suspended source-core owner.
      const previousOwner = sourceCtx.currentOwner;
      sourceCtx.currentOwner = null;
      sourceCtx.ownerDepth++;
      try {
        forwarding = withWasmCore(sourceCore, () => effect(publish));
      } finally {
        sourceCtx.ownerDepth--;
        sourceCtx.currentOwner = previousOwner;
      }
    } catch (error) {
      bridge.close();
      throw error;
    }
  }
  bridge.seen = true;
  // Collection must run in the target domain; no source-core node ID crosses it.
  bridge.proxy();
}

/** Below this many array dependencies a linear duplicate scan beats a Set. */
const SEEN_DEPS_THRESHOLD = 32;

function trackReadFast(ctx: JsReactiveContext, sourceId: number): void {
  // A source core may have a paused outer observer while another core owns
  // the current evaluation. Never attribute that foreign read to the caller.
  if (reactiveDomains.frames.at(-1) !== ctx) return;
  const entry = ctx.activeEntry;
  if (!entry) return;
  if (entry.retiredDependencies?.delete(sourceId)) ctx.activeTopologyChanged = true;
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
  let seen = entry.seenDeps;
  // Until this run changes the topology, positions below the cursor still hold
  // the previous run's dependencies, which are unique: a match at the cursor
  // cannot also be an earlier one. Reruns in the same order stay O(1) per read.
  if (!ctx.activeTopologyChanged && cur < entry.depsCount && deps[arrIdx] === sourceId) {
    seen?.add(sourceId);
    ctx.activeCursor = cur + 1;
    return;
  }
  if (seen === null && arrIdx >= SEEN_DEPS_THRESHOLD) {
    seen = entry.seenDeps = new Set();
    for (let i = 0; i < arrIdx; i++) {
      const dep = deps[i];
      if (dep !== undefined) seen.add(dep);
    }
  }
  if (seen !== null) {
    if (seen.has(sourceId)) return;
    seen.add(sourceId);
  } else {
    for (let i = 0; i < arrIdx; i++) {
      if (deps[i] === sourceId) return;
    }
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
  entry: EffectEntry | ComputedEntry<any>,
): void {
  retireForeignTracking(entry);
  entry.seenDeps = null;
  if (entry.sourceRetirementVersion !== ctx.sourceRetirementVersion) {
    ctx.activeTopologyChanged = true;
    entry.sourceRetirementVersion = ctx.sourceRetirementVersion;
  }
  let cur = ctx.activeCursor;
  if (entry.retiredDependencies?.size) {
    let kept = 0;
    for (let i = 0; i < cur; i++) {
      const source = i === 0 ? entry.dep0 : i === 1 ? entry.dep1 : entry.deps[i - 2];
      if (entry.retiredDependencies.has(source)) continue;
      if (kept === 0) entry.dep0 = source;
      else if (kept === 1) entry.dep1 = source;
      else entry.deps[kept - 2] = source;
      kept++;
    }
    if (kept !== cur) ctx.activeTopologyChanged = true;
    cur = kept;
    ctx.activeCursor = cur;
  }
  entry.retiredDependencies = null;
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
  if (entry.kind === 2 && cur === 1 && !entry.wasmLinked && ctx.batchDepth === 0) {
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
  } else if (entry.kind === 2 && cur >= 1) {
    const s0 = ctx.signalRegistry[entry.dep0];
    if (s0) s0._invalidateDirectEffect(ctx.topologyVersion);
    if (cur >= 2) {
      const s1 = ctx.signalRegistry[entry.dep1];
      if (s1) s1._invalidateDirectEffect(ctx.topologyVersion);
    }
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

function setNodeStateFast(
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

let ownerTeardownDepth = 0;
const retiringOwnerScopes = new WeakSet<OwnerScope>();
type RootDisposerCell = { core: SvetWasmExports | null; scope: OwnerScope | null };
const rootDisposerCells = (reactiveDomains.rootDisposerCells ??= new WeakMap<
  OwnerScope,
  RootDisposerCell
>());
const reactiveHandleRetirements = (reactiveDomains.handleRetirements ??= new WeakMap<
  object,
  ReactiveHandleRetirement
>());

function disposeOwnerScope(scope: OwnerScope): void {
  if (scope.disposed) return;
  return untrack(() => {
    ownerTeardownDepth++;
    try {
      scope.disposed = true;
      // Preserve depth-first child order without consuming the JavaScript call stack.
      const frames: Array<{ scope: OwnerScope; children: Iterator<OwnerScope> | undefined }> = [
        { scope, children: scope.children?.values() },
      ];
      while (frames.length > 0) {
        const frame = frames[frames.length - 1];
        const next = frame.children?.next();
        if (next && !next.done) {
          const child = next.value;
          if (!child.disposed) {
            child.disposed = true;
            frames.push({ scope: child, children: child.children?.values() });
          }
          continue;
        }
        const current = frame.scope;
        current.children?.clear();
        for (let i = current.disposables.length - 1; i >= 0; i--) {
          try {
            current.disposables[i]();
          } catch {
            /* Continue teardown after cleanup failure. */
          }
        }
        current.disposables.length = 0;
        const rootCell = rootDisposerCells.get(current);
        if (rootCell) {
          rootCell.core = null;
          rootCell.scope = null;
          rootDisposerCells.delete(current);
        }
        if (current.parent?.children) {
          current.parent.children.delete(current);
          current.parent = null;
        }
        frames.pop();
      }
    } finally {
      ownerTeardownDepth--;
    }
  });
}

function addChildScope(parent: OwnerScope, child: OwnerScope): void {
  if (!parent.children) {
    parent.children = new Set();
  }
  parent.children.add(child);
}

/** Keep a live root's core strongly owned, then detach retained terminal callbacks. */
function createRootDisposer(core: SvetWasmExports, scope: OwnerScope): () => void {
  const cell: RootDisposerCell = { core, scope };
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

/**
 * Creates a hierarchical ownership root (`createRoot`).
 * All effects, computed nodes, and `onCleanup` callbacks created inside `fn` are tied to this scope
 * and automatically disposed + recycled in WASM when `dispose()` is called!
 */
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

/**
 * Registers a cleanup callback on the current active `OwnerScope` (component, effect, `<Show>`, or `<For>`).
 */
export function onCleanup(cleanupFn: () => void): void {
  const owner = getCurrentOwner();
  if (owner && !owner.disposed) owner.disposables.push(cleanupFn);
}

/** @internal A descendant is not executable while any ancestor is stopping. */
export function isOwnerScopeDisposed(owner: OwnerScope | null | undefined): boolean {
  if (!owner) return false;
  if (owner.disposed) return true;
  // Physical teardown and effect rerun cleanup drain descendants synchronously.
  // Their callbacks can observe children not yet physically visited by cleanup.
  if (ownerTeardownDepth === 0) return false;
  for (let current: OwnerScope | null = owner; current; current = current.parent) {
    if (current.disposed || retiringOwnerScopes.has(current)) return true;
  }
  return false;
}

/**
 * Returns the currently active `OwnerScope` (or `null` if outside a root/effect).
 */
export function getCurrentOwner(): OwnerScope | null {
  const core = getWasmCore();
  return resolveCurrentOwner(getJsContext(core), core);
}

export class ReactiveContextDisposedError extends Error {
  readonly code = 'SVET_CONTEXT_DISPOSED';
  constructor() {
    super('Captured reactive context is no longer active');
    this.name = 'ReactiveContextDisposedError';
  }
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
    const group = state.group;
    if (group.first === dispose) group.first = undefined;
    else group.pending?.delete(dispose);
    if (!group.first && !group.pending?.size) {
      group.pending?.clear();
      if (!state.owner.disposed) {
        const callbacks = state.owner.disposables;
        const index =
          callbacks[group.slot] === group.cleanup ? group.slot : callbacks.indexOf(group.cleanup);
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

/**
 * Runs `fn` within a child owner that inherits context values from its parent.
 * The callback receives a bound disposer; otherwise the child lives until parent teardown.
 */
export function runWithOwnerScope<T>(
  contextEntries: Array<[symbol, any]>,
  fn: (dispose: () => void) => T,
): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const previousStoredOwner = ctx.currentOwner;
  const prevOwner = resolveCurrentOwner(ctx, core);
  if (isOwnerScopeDisposed(prevOwner))
    throw new Error('Cannot create child scope in a disposed owner');
  const generation = ctx.generation;

  const contextMap = new Map<symbol, any>(prevOwner?.context);
  for (const [k, v] of contextEntries) {
    contextMap.set(k, v);
  }
  // Iterators and tuple getters are user code and can retire this construction.
  // Recheck before attaching a scope or changing the current owner.
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

/**
 * Looks up a value by symbol key up the `OwnerScope` chain.
 */
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

export type DeepSignalStore<T> =
  T extends Record<string, any>
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
    k => k !== '__proto__' && k !== 'constructor' && k !== 'prototype',
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
          if (
            v !== null &&
            typeof v === 'object' &&
            !Array.isArray(v) &&
            typeof target.patch === 'function'
          ) {
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
 * Isolation covers synchronous execution only; active-core selection does not
 * propagate across await boundaries. Async request-local context remains separate work.
 */
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

/** Internal lifetime token used by non-scalar WASM-backed primitives. */
export function getReactiveGeneration(core: SvetWasmExports = getWasmCore()): number {
  return getJsContext(core).generation;
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

export class ReactiveCycleError extends Error {
  readonly nodeIds: readonly number[];
  constructor(nodeIds: readonly number[]) {
    super(`Cyclic computed dependency: ${nodeIds.join(' -> ')}`);
    this.name = 'ReactiveCycleError';
    this.nodeIds = Object.freeze([...nodeIds]);
  }
}

const MAX_EFFECT_ITERATIONS = 100;
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

function requestActiveEffectRerun(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  source: number,
): void {
  // untrack suppresses new reads, not writes to an already staged dependency.
  for (const entry of ctx.observerStack) {
    if (
      entry.kind !== 2 ||
      entry.disposed ||
      entry.running === 0 ||
      entry.generation !== ctx.generation
    )
      continue;
    const cursor = entry === ctx.activeEntry ? ctx.activeCursor : entry.pausedCursor;
    for (let i = 0; i < cursor; i++) {
      const dependency = i === 0 ? entry.dep0 : i === 1 ? entry.dep1 : entry.deps[i - 2];
      if (entry.retiredDependencies?.has(dependency)) continue;
      if (
        dependency === source ||
        (ctx.computedRegistry[dependency] && core.svet_depends_on(dependency, source))
      ) {
        entry.rerunRequested = true;
        break;
      }
    }
  }
}

/** Native IDs are reusable slots. Capture JS lifetimes before invoking any callback. */
function runQueuedEffects(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  ids: Uint32Array,
  onRun?: (entry: EffectEntry) => void,
): void {
  // Callers pass a private ID copy; capture identities before user code can recycle them.
  const entries = Array.from(ids, id => ctx.effectRegistry[id]);
  const lifetimes = entries.map(entry => entry?.lifetime);
  let failed = false,
    failure: unknown;
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i],
      id = ids[i];
    if (
      entry &&
      !entry.disposed &&
      entry.lifetime === lifetimes[i] &&
      entry.generation === ctx.generation &&
      ctx.effectRegistry[id] === entry &&
      core.svet_get_state(id) !== STATE_CLEAN
    ) {
      try {
        onRun?.(entry);
        entry.run();
      } catch (error) {
        if (!failed) {
          failed = true;
          failure = error;
        }
      }
    }
  }
  if (failed) throw failure;
}

export function triggerNodeWrite(id: number): void {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  ctx.globalClock++;
  requestActiveEffectRerun(core, ctx, id);
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
      runQueuedEffects(core, ctx, copy);
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
  const core = getWasmCore(),
    ctx = getJsContext(core);
  if (ctx.isRunningEffect) {
    ctx.cascadedDuringEffect = true;
    return;
  }
  if (!batchTransport.depth && ctx.foreignForwarders) {
    // Initial effect execution may queue writes without passing through an
    // external signal setter. Its whole native flush is one delivery boundary.
    batchTransport.depth++;
    let failed = false;
    try {
      return flushEffects();
    } catch (error) {
      failed = true;
      throw error;
    } finally {
      batchTransport.depth--;
      if (failed) {
        try {
          finishForeignBatches();
        } catch {
          /* Preserve the source flush error. */
        }
      } else finishForeignBatches();
    }
  }
  const generation = ctx.generation;
  const runs = new Map<EffectEntry, { lifetime: number; count: number }>();
  const saturated = new Map<EffectEntry, number>();
  const onRun = (entry: EffectEntry) => {
    let record = runs.get(entry);
    if (!record || record.lifetime !== entry.lifetime) {
      record = { lifetime: entry.lifetime, count: 0 };
      runs.set(entry, record);
    }
    if (++record.count >= MAX_EFFECT_ITERATIONS) saturated.set(entry, entry.lifetime);
  };
  ctx.isRunningEffect = true;
  try {
    while (generation === ctx.generation) {
      const queue = readFlushedEffects(core);
      if (queue.length === 0) break;
      if (queue.length === 1) {
        const id = queue[0],
          entry = ctx.effectRegistry[id];
        if (entry && !entry.disposed && core.svet_get_state(id) !== STATE_CLEAN) {
          onRun(entry);
          entry.run();
        }
      } else runQueuedEffects(core, ctx, queue.slice(), onRun);
      if (generation !== ctx.generation) break;
      // Check before draining the next native queue so recovery work remains queued.
      for (const [entry, lifetime] of saturated) {
        if (
          !entry.disposed &&
          entry.lifetime === lifetime &&
          ctx.effectRegistry[entry.id] === entry &&
          core.svet_get_state(entry.id) !== STATE_CLEAN &&
          core.svet_pending_effect_count() > 0
        ) {
          throw new ReactiveStabilizationError(null);
        }
      }
    }
  } finally {
    if (generation === ctx.generation) {
      ctx.isRunningEffect = false;
      ctx.cascadedDuringEffect = false;
    }
  }
}

/** Foreign reactive callbacks cannot collect reads into paused callers. */
function runInForeignReactiveCore<T>(core: SvetWasmExports, fn: () => T): T {
  return untrack(() => withWasmCore(core, () => untrack(fn)));
}

/** @internal Pause all existing observer frames; captured work can outlive an ambient request. */
export function runUntrackedInCore<T>(core: SvetWasmExports, fn: () => T): T {
  // Preserve ambient resolution: adapters may retire/reset a lifetime here and
  // callers recheck it inside fn. Captured work may also outlive that adapter.
  try {
    getWasmCore();
  } catch {
    /* The explicitly selected core may still be live. */
  }
  return withWasmCore(core, () => untrack(fn));
}

/**
 * Creates a fine-grained reactive Signal backed by the WASM dependency graph.
 */
export function signal<T>(
  initialValue: T,
  options?: { equals?: (a: T, b: T) => boolean },
): Signal<T> {
  let core = getWasmCore();
  let ctx = getJsContext(core);
  const generation = ctx.generation;
  let equals = options?.equals ?? Object.is;
  if (generation !== ctx.generation)
    throw new Error('Reactive graph reset during signal construction');
  if (typeof equals !== 'function') throw new TypeError('Signal equals must be a function');
  const id = allocateReactiveNode(core, KIND_SIGNAL);
  if (ctx.singleSignalRootId === -1) {
    ctx.singleSignalRootId = id;
  } else {
    ctx.singleSignalRootId = -2;
  }
  let currentValue = initialValue;
  let valueVersion = 0;
  let disposed = false;
  let activeWrites = 0;
  const detachDisposedContext = () => {
    if (!disposed || activeWrites > 0) return;
    singleSubEffect = null;
    equals = Object.is;
    core = null!;
    ctx = null!;
  };

  let singleSubEffect: EffectEntry | null | undefined = undefined; // undefined = unknown, null = multiple/none
  let hasComputedSubscriber = false;

  const getter = (() => {
    if (disposed || generation !== ctx.generation) return currentValue;
    trackForeignSource(core, getter, () => valueVersion);
    if (ctx.activeObserverId >= 0 && reactiveDomains.frames.at(-1) === ctx) {
      const obsEntry = ctx.activeEntry!;
      if (obsEntry.retiredDependencies?.delete(id)) ctx.activeTopologyChanged = true;
      const cur = ctx.activeCursor;
      if (cur === 0 && obsEntry.depsCount >= 1 && obsEntry.dep0 === id) {
        ctx.activeCursor = 1;
      } else if (
        cur === 1 &&
        obsEntry.depsCount >= 2 &&
        obsEntry.dep1 === id &&
        obsEntry.dep0 !== id
      ) {
        ctx.activeCursor = 2;
      } else if (!disposed) {
        trackReadFast(ctx, id);
      }
    }
    return currentValue;
  }) as Signal<T>;

  (getter as any).id = id;
  (getter as any).__svet_sig = true;
  (getter as any)._tryRegisterDirectUnlinked = (eff: EffectEntry, _ver: number): boolean => {
    if (disposed) return false;
    if (
      !hasComputedSubscriber &&
      (singleSubEffect === undefined || singleSubEffect === eff || singleSubEffect?.disposed)
    ) {
      singleSubEffect = eff;
      return true;
    }
    if (singleSubEffect && !singleSubEffect.disposed && !singleSubEffect.wasmLinked) {
      singleSubEffect.wasmLinked = true;
      core.svet_link_1(singleSubEffect.id, id);
    }
    singleSubEffect = null;
    return false;
  };
  (getter as any)._invalidateDirectEffect = (_ver: number, isComputed = false) => {
    if (disposed) return;
    if (isComputed) hasComputedSubscriber = true;
    if (singleSubEffect && !singleSubEffect.disposed && !singleSubEffect.wasmLinked) {
      singleSubEffect.wasmLinked = true;
      core.svet_link_1(singleSubEffect.id, id);
    }
    singleSubEffect = null;
  };
  ctx.signalRegistry[id] = getter;

  getter.peek = () => currentValue;
  (getter as any).__svet_cached_value = getter.peek;

  getter.set = (next: T | ((prev: T) => T)) => {
    if (disposed || generation !== ctx.generation) return;
    activeWrites++;
    try {
      if (getWasmCore() !== core) return runInForeignReactiveCore(core, () => getter.set(next));
      // One originating write may fan out through several native cores. Keep all
      // downstream deliveries in the same boundary even without an explicit batch.
      if (!batchTransport.depth && !ctx.isRunningEffect && ctx.foreignForwarders) {
        return batch(() => getter.set(next));
      }
      const newValue = typeof next === 'function' ? (next as (prev: T) => T)(currentValue) : next;
      if (disposed || generation !== ctx.generation) return;
      if (!equals(currentValue, newValue)) {
        if (disposed || generation !== ctx.generation) return;
        holdForeignBatch(core, ctx);
        currentValue = newValue;
        valueVersion++;
        ctx.globalClock++;
        requestActiveEffectRerun(core, ctx, id);
        if (ctx.isRunningEffect) {
          // A write inside a callback queues work after that callback commits.
          // Unlinked direct subscribers must first become visible to native scheduling.
          if (
            singleSubEffect &&
            !singleSubEffect.disposed &&
            !singleSubEffect.wasmLinked &&
            singleSubEffect.depsCount === 1 &&
            singleSubEffect.dep0 === id
          ) {
            singleSubEffect.wasmLinked = true;
            core.svet_link_1(singleSubEffect.id, id);
          }
          ctx.cascadedDuringEffect = true;
          core.svet_trigger_write(id);
          return;
        }
        // A unique terminal subscriber can run directly, but must still track reads:
        // observing the same sources twice does not prove that future reads are static.
        if (ctx.batchDepth === 0 && singleSubEffect && !singleSubEffect.disposed) {
          if (
            !hasComputedSubscriber &&
            singleSubEffect.depsCount === 1 &&
            singleSubEffect.dep0 === id
          ) {
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
        }
        const res = core.svet_trigger_and_flush(id);
        if (res > 0) {
          const effId = res - 1;
          const entry = ctx.effectRegistry[effId];
          if (ctx.batchDepth === 0) {
            singleSubEffect = entry ?? null;
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
          const count = -res;
          const ptr = core.svet_get_flush_ptr();
          const copy = new Uint32Array(core.memory.buffer, ptr, count).slice();
          ctx.isRunningEffect = true;
          try {
            runQueuedEffects(core, ctx, copy);
          } finally {
            ctx.isRunningEffect = false;
          }
          if (ctx.cascadedDuringEffect) {
            ctx.cascadedDuringEffect = false;
            flushEffects();
          }
        } else if (ctx.batchDepth === 0) {
          singleSubEffect = null;
        }
      }
    } finally {
      activeWrites--;
      detachDisposedContext();
    }
  };

  getter.update = (updater: (prev: T) => T) => {
    getter.set(updater);
  };

  getter.dispose = () => {
    if (disposed) return;
    disposed = true;
    try {
      if (generation !== ctx.generation) return;
      retireForeignSource(getter);
      ctx.signalRegistry[id] = undefined;
      ctx.topologyVersion++;
      disposeTrackedNode(core, id);
    } finally {
      // Active writes may still be unwinding subscriber callbacks.
      detachDisposedContext();
    }
  };

  reactiveHandleRetirements.set(getter, getter.dispose);
  return getter;
}

/**
 * Creates a glitch-free lazy Computed signal backed by the WASM DAG.
 * Automatically disposes its WASM node if created inside an `OwnerScope`.
 */
export function computed<T>(
  fn: () => T,
  options?: { equals?: (a: T, b: T) => boolean },
): ReadonlySignal<T> {
  let core = getWasmCore();
  let ctx = getJsContext(core);
  const generation = ctx.generation;
  let owner = resolveCurrentOwner(ctx, core);
  if (isOwnerScopeDisposed(owner)) throw new Error('Cannot create computed in a disposed owner');
  if (typeof fn !== 'function') throw new TypeError('Computed body must be a function');
  const equals = options?.equals ?? Object.is;
  if (isOwnerScopeDisposed(owner)) throw new Error('Computed owner disposed during construction');
  if (generation !== ctx.generation)
    throw new Error('Reactive graph reset during computed construction');
  if (typeof equals !== 'function') throw new TypeError('Computed equals must be a function');
  const id = allocateReactiveNode(core, KIND_COMPUTED);
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
    valueVersion: 0,
    evaluating: false,
    failed: false,
    retiredDependencies: null,
    seenDeps: null,
    sourceRetirementVersion: 0,
  };
  ctx.computedRegistry[id] = entry;

  const detachDisposedContext = () => {
    if (!disposed || entry.evaluating) return;
    entry.compute = undefined as any;
    entry.equals = Object.is;
    entry.deps.length = 0;
    entry.dep0 = -1;
    entry.dep1 = -1;
    entry.depsCount = 0;
    fn = null!;
    options = undefined;
    owner = null;
    core = null!;
    ctx = null!;
  };

  const cycleError = () => {
    const computeds = ctx.observerStack.filter(item => item.kind === 1);
    const start = Math.max(0, computeds.indexOf(entry));
    return new ReactiveCycleError([...computeds.slice(start).map(item => item.id), id]);
  };

  const evaluateIfNeeded = (): T => {
    if (disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
      return entry.value;
    if (entry.evaluating) throw cycleError();
    refreshForeignTracking(entry, ctx);
    if (entry.initialized && !entry.failed && entry.lastCleanClock === ctx.globalClock) {
      return entry.value;
    }

    let state = 2;
    if (entry.initialized && !entry.failed) {
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
    if (prevEntry?.kind === 2) prevEntry.pausedCursor = prevCursor;

    ctx.activeObserverId = id;
    ctx.activeEntry = entry;
    ctx.activeCursor = 0;
    ctx.activeTopologyChanged = !entry.initialized;
    entry.seenDeps = null;

    const evaluations = ctx.observerStack;
    beginForeignTracking(entry);
    evaluations.push(entry);
    enterObservationContext(ctx);
    entry.evaluating = true;
    let succeeded = false;
    try {
      const nextVal = entry.compute();
      if (disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
        return entry.value;
      const changed = !entry.initialized || !entry.equals(entry.value, nextVal);
      if (disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
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
            if (!disposed && !isOwnerScopeDisposed(owner)) {
              commitTrackingFast(core, ctx, id, entry);
              if (!succeeded) {
                entry.failed = true;
                // The host failure flag forces retries; clean native state allows
                // the next invalidation to propagate through failed branches.
                setNodeStateFast(core, ctx, id, STATE_CLEAN);
              }
            }
          } finally {
            ctx.activeObserverId = prevObs;
            ctx.activeEntry = prevEntry;
            ctx.activeCursor = prevCursor;
            ctx.activeTopologyChanged = prevChanged;
          }
        }
      } finally {
        entry.evaluating = false;
        entry.retiredDependencies = null;
        if (generation === ctx.generation) evaluations.pop();
        leaveObservationContext(ctx);
        detachDisposedContext();
      }
    }
    entry.lastCleanClock = ctx.globalClock;
    return entry.value;
  };

  const getter = (() => {
    if (disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
      return entry.value;
    const foreignTarget = reactiveDomains.frames.at(-1);
    if (
      foreignTarget?.activeEntry &&
      foreignTarget !== ctx &&
      foreignTarget.activeObserverId >= 0
    ) {
      if (entry.evaluating) throw cycleError();
      trackForeignSource(core, getter, () => entry.valueVersion);
      return runInForeignReactiveCore(core, () => getter());
    }
    if (ctx.activeObserverId >= 0 && reactiveDomains.frames.at(-1) !== ctx) {
      return runInForeignReactiveCore(core, () => getter());
    }
    if (ctx.activeObserverId >= 0 && getWasmCore() !== core) {
      // A compatible copy may own the current native observer even though this
      // module's ambient core differs. Preserve that shared-domain dependency;
      // suspended observers in a genuinely foreign domain remain untracked.
      return reactiveDomains.frames.at(-1) === ctx
        ? withWasmCore(core, () => getter())
        : runInForeignReactiveCore(core, () => getter());
    }
    try {
      if (entry.evaluating) throw cycleError();
      refreshForeignTracking(entry, ctx);
      if (ctx.activeObserverId < 0) {
        if (entry.initialized && !entry.failed) {
          if (entry.lastCleanClock === ctx.globalClock) {
            return entry.value;
          }
        }
        return getWasmCore() === core
          ? evaluateIfNeeded()
          : runInForeignReactiveCore(core, evaluateIfNeeded);
      }
      const val =
        entry.initialized && !entry.failed && entry.lastCleanClock === ctx.globalClock
          ? entry.value
          : evaluateIfNeeded();
      if (disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation) return val;
      const obsEntry = ctx.activeEntry!;
      if (obsEntry.retiredDependencies?.delete(id)) ctx.activeTopologyChanged = true;
      const cur = ctx.activeCursor;
      if (cur === 0 && obsEntry.depsCount >= 1 && obsEntry.dep0 === id) {
        ctx.activeCursor = 1;
      } else if (
        cur === 1 &&
        obsEntry.depsCount >= 2 &&
        obsEntry.dep1 === id &&
        obsEntry.dep0 !== id
      ) {
        ctx.activeCursor = 2;
      } else if (!disposed && !isOwnerScopeDisposed(owner)) {
        trackReadFast(ctx, id);
      }
      return val;
    } catch (error) {
      if (
        !disposed &&
        !isOwnerScopeDisposed(owner) &&
        generation === ctx.generation &&
        ctx.activeObserverId >= 0 &&
        ctx.activeObserverId !== id
      ) {
        trackReadFast(ctx, id);
      }
      throw error;
    }
  }) as ReadonlySignal<T>;

  (getter as any).id = id;
  (getter as any).__svet_cached_value = () => entry.value;
  getter.peek = () => {
    if (disposed || isOwnerScopeDisposed(owner) || generation !== ctx.generation)
      return entry.value;
    if (!entry.evaluating) refreshForeignTracking(entry, ctx);
    if (
      !entry.evaluating &&
      entry.initialized &&
      !entry.failed &&
      entry.lastCleanClock === ctx.globalClock
    )
      return entry.value;
    return getWasmCore() === core
      ? evaluateIfNeeded()
      : runInForeignReactiveCore(core, evaluateIfNeeded);
  };

  const disposeComputed = (unregisterOwner: boolean) => {
    if (disposed) return;
    disposed = true;
    if (unregisterOwner && owner && !owner.disposed) {
      const index = owner.disposables.indexOf(ownerDisposer);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
    try {
      if (generation !== ctx.generation) return;
      ctx.topologyVersion++;
      retireForeignTracking(entry, true);
      retireForeignSource(getter);
      ctx.computedRegistry[id] = undefined;
      disposeTrackedNode(core, id);
    } finally {
      // A computation may dispose itself; its observation frame must unwind
      // before the captured context can be released.
      detachDisposedContext();
    }
  };

  const ownerDisposer = () => disposeComputed(true);
  getter.dispose = ownerDisposer;
  reactiveHandleRetirements.set(entry, owners => {
    if (owners && !disposed && owner && !owner.disposed) {
      let callbacks = owners.get(owner);
      if (!callbacks) {
        callbacks = new Set();
        owners.set(owner, callbacks);
      }
      callbacks.add(ownerDisposer);
      disposeComputed(false);
    } else disposeComputed(true);
  });
  if (owner) owner.disposables.push(ownerDisposer);

  return getter;
}

function cleanupEffectOwnerScope(effectOwner: OwnerScope | null): void {
  if (!effectOwner || (!effectOwner.children?.size && effectOwner.disposables.length === 0)) return;
  return untrack(() => {
    const alreadyRetiring = retiringOwnerScopes.has(effectOwner);
    ownerTeardownDepth++;
    try {
      retiringOwnerScopes.add(effectOwner);
      if (effectOwner.children && effectOwner.children.size > 0) {
        for (const child of effectOwner.children) {
          disposeOwnerScope(child);
        }
        effectOwner.children.clear();
      }
      if (effectOwner.disposables.length > 0) {
        // Detach before invoking callbacks: manual disposal can unregister a sibling
        // cleanup, otherwise shifting the live list can invoke a callback twice.
        const disposables = effectOwner.disposables.splice(0);
        for (let i = disposables.length - 1; i >= 0; i--) {
          try {
            disposables[i]();
          } catch {
            // ignore
          }
        }
      }
    } finally {
      if (!alreadyRetiring) retiringOwnerScopes.delete(effectOwner);
      ownerTeardownDepth--;
    }
  });
}

function recycleEffectEntry(entry: EffectEntry): void {
  const x = entry.ctx;
  entry.userFn = undefined as any;
  entry.owner = null;
  entry.cleanup = undefined;
  entry.retiredDependencies = null;
  entry.sourceRetirementVersion = 0;
  entry.pausedCursor = 0;
  entry.deps.length = 0;
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
    seen = new Set<ComputedEntry>();
  const append = (observer: EffectEntry | ComputedEntry) => {
    for (let i = 0; i < observer.depsCount; i++)
      pending.push([
        observer.id,
        i === 0 ? observer.dep0 : i === 1 ? observer.dep1 : observer.deps[i - 2],
      ]);
  };
  append(entry);
  while (pending.length) {
    const [parent, source] = pending.pop()!,
      dependency = ctx.computedRegistry[source];
    // JS dependency snapshots can contain a retired numeric slot. Only current
    // native ancestors may have their cache/state repaired, never replacements.
    if (!dependency || seen.has(dependency) || !core.svet_depends_on(parent, source)) continue;
    seen.add(dependency);
    dependency.failed = true;
    setNodeStateFast(core, ctx, source, STATE_CLEAN);
    append(dependency);
  }
  // Dirty ancestors must also be rearmed or they would suppress propagation of
  // later writes to this otherwise-clean effect. Failed caches retry on read.
  setNodeStateFast(core, ctx, entry.id, STATE_CLEAN);
}

function drainEffectCleanups(callbacks: Array<() => void>): void {
  const pending = callbacks.splice(0);
  let failed = false,
    failure: unknown;
  for (let i = pending.length - 1; i >= 0; i--) {
    try {
      pending[i]();
    } catch (error) {
      if (!failed) {
        failed = true;
        failure = error;
      }
    }
  }
  if (failed) throw failure;
}

function createEffectEntryShell(core: SvetWasmExports, ctx: JsReactiveContext): EffectEntry {
  const entry: EffectEntry = {
    kind: 2,
    id: -1,
    core,
    ctx,
    generation: ctx.generation,
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
    running: 0,
    pausedCursor: 0,
    runVersion: 0,
    lifetime: 0,
    rerunRequested: false,
    retiredDependencies: null,
    seenDeps: null,
    sourceRetirementVersion: 0,
    run: () => {
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
        x = entry.ctx;
      const effectOwner = entry.owner;
      try {
        let iterations = 0;
        do {
          if (iterations++ === MAX_EFFECT_ITERATIONS) throw new ReactiveStabilizationError(id);
          entry.rerunRequested = false;
          const version = ++entry.runVersion;
          if (effectOwner) cleanupEffectOwnerScope(effectOwner);
          // Detach before invoking user code, which may dispose this effect.
          const cleanup = entry.cleanup;
          entry.cleanup = undefined;
          if (cleanup) {
            try {
              untrack(cleanup);
            } catch (failure) {
              if (
                !entry.disposed &&
                !isOwnerScopeDisposed(effectOwner) &&
                generation === x.generation
              ) {
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
            return;

          const prevOwner = x.currentOwner,
            prevObs = x.activeObserverId,
            prevEntry = x.activeEntry;
          const prevRunning = x.isRunningEffect,
            prevCursor = x.activeCursor,
            prevChanged = x.activeTopologyChanged;
          if (prevEntry?.kind === 2) prevEntry.pausedCursor = prevCursor;
          if (effectOwner) x.currentOwner = effectOwner;
          x.ownerDepth++;
          x.isRunningEffect = true;
          x.activeObserverId = id;
          x.activeEntry = entry;
          x.activeCursor = 0;
          x.activeTopologyChanged = !entry.initialized;
          entry.seenDeps = null;
          entry.initialized = true;
          let callbacks: Array<() => void> | undefined;
          const registerCleanup = (callback: () => void) => {
            if (typeof callback !== 'function')
              throw new TypeError('Effect cleanup must be a function');
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
              // Retired sources belong to this invocation, including paused ancestors.
              if (generation === x.generation) {
                try {
                  if (!entry.disposed) commitTrackingFast(c, x, id, entry);
                } finally {
                  x.activeObserverId = prevObs;
                  x.activeEntry = prevEntry;
                  x.activeCursor = prevCursor;
                  x.activeTopologyChanged = prevChanged;
                  x.currentOwner = prevOwner;
                  x.isRunningEffect = prevRunning;
                  x.observerStack.pop();
                  entry.retiredDependencies = null;
                }
              } else {
                entry.retiredDependencies = null;
              }
            } finally {
              x.ownerDepth--;
              leaveObservationContext(x);
            }
          }
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
    },
  };
  return entry;
}

/** Reset invalidates scheduling, while registered cleanup remains owner-owned. */
function createEffectResetRetirement(entry: EffectEntry): () => void {
  return () => {
    entry.userFn = undefined as any;
  };
}

/** Public effect lifetimes must not retain recycled native entry shells. */
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
      // Detach before user cleanup: reentry is inert, and thrown cleanup cannot
      // leave a terminal public handle pointing at a core or a recycled shell.
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

/**
 * Creates an automatically tracked side-effect.
 * - Registers with the enclosing `OwnerScope` (if any) for automatic hierarchical disposal.
 * - Creates its own child `OwnerScope` that is cleaned up before each re-execution!
 * - Recycles its WASM slot ID and JS `EffectEntry` shell back into `free_list` upon `dispose()`.
 */
export function effect(
  fn: (onCleanupFn: (cleanupFn: () => void) => void) => void | (() => void),
): EffectHandle {
  const core = getWasmCore();
  const ctx = getJsContext(core);
  const parentOwner = resolveCurrentOwner(ctx, core);
  if (isOwnerScopeDisposed(parentOwner))
    throw new Error('Cannot create effect in a disposed owner');
  if (typeof fn !== 'function') throw new TypeError('Effect body must be a function');

  // Every effect owns resources created during its execution, including effects
  // created outside a root. Otherwise nested subscriptions survive each rerun.
  const effectOwner: OwnerScope = {
    disposables: [],
    children: null,
    parent: parentOwner,
    disposed: false,
    context: parentOwner?.context,
  };

  if (parentOwner && effectOwner) {
    addChildScope(parentOwner, effectOwner);
  }

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
    if (entry.deps.length > 0) entry.deps.length = 0;
    if (entry.id < 0) {
      entry.id = allocateReactiveNode(core, KIND_EFFECT);
    }
  }
  const id = entry.id;
  entry.lifetime++;
  entry.owner = effectOwner;
  entry.userFn = fn;

  ctx.effectRegistry[id] = entry;
  reactiveHandleRetirements.set(entry, createEffectResetRetirement(entry));

  // The entry and its internal handle are pooled, but a caller's handle belongs
  // to exactly one effect lifetime. In particular, a retained owner cleanup must
  // never dispose a replacement effect after this entry has been reused.
  const handle = createEffectPublicHandle(core, parentOwner, entry.handle, id);

  if (parentOwner) {
    parentOwner.disposables.push(handle.dispose);
  }

  try {
    entry.run();
    if (!ctx.isRunningEffect && ctx.cascadedDuringEffect) {
      ctx.cascadedDuringEffect = false;
      flushEffects();
    }
  } catch (error) {
    // Initialization did not yield a handle to the caller. Release everything
    // created so far and preserve the initialization error if cleanup also fails.
    try {
      handle.dispose();
    } catch {
      /* preserve the original error */
    }
    throw error;
  }

  return handle;
}

/**
 * Defers effect notifications until the outermost batch ends. Writes are not rolled back.
 */
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
}

/**
 * Reads signals inside `fn` without creating any dependency edges in the WASM graph.
 * Existing synchronous observers in every core of compatible copies are paused;
 * new observers created by fn still collect their own dependencies.
 * Temporarily disables host read collection; writes still invalidate already-read dependencies.
 */
export function untrack<T>(fn: () => T): T {
  const core = getWasmCore();
  const ctx = getJsContext(core);
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
        entry: EffectEntry | ComputedEntry<any> | null;
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
        const saved = suspended[i];
        if (saved.generation === saved.ctx.generation) {
          saved.ctx.activeObserverId = saved.id;
          saved.ctx.activeEntry = saved.entry;
        }
      }
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
    k => k !== '__proto__' && k !== 'constructor' && k !== 'prototype',
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
  const ctx = getJsContext(core);
  // Capture the retiring generation before reset cleanups can create new nodes.
  // Use private callbacks rather than mutable public dispose properties.
  const retirements: ReactiveHandleRetirement[] = [];
  for (const registry of [ctx.signalRegistry, ctx.computedRegistry, ctx.effectRegistry]) {
    for (const handle of registry) {
      if (!handle) continue;
      const retire = reactiveHandleRetirements.get(handle);
      if (retire) retirements.push(retire);
    }
  }
  ctx.generation++;
  const retiredOwners: ReactiveResetOwnerRetirements = new Map();
  for (const retire of retirements) retire(retiredOwners);
  // Preserve the public callback array and user cleanup order. Per-node splice
  // would shift/scan a large shared owner once for every retired computed.
  for (const [owner, retired] of retiredOwners) {
    const callbacks = owner.disposables;
    let write = 0;
    for (let read = 0; read < callbacks.length; read++) {
      const callback = callbacks[read];
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
  // Retired generations can still have JS frames on the stack; their finally
  // blocks, not reset, release observationDepth and the shared context entry.
  // ownerDepth likewise belongs to live synchronous frames across generations.
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
  // Detach the old subscriptions before invoking user code. A finalizer may
  // start a fresh request or recursively reset; neither joins this old batch.
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

/**
 * Returns live telemetry from the WASM reactive graph, including active nodes vs allocated capacity.
 */
export function getGraphTelemetry(): {
  nodes: number;
  capacity: number;
  edges: number;
  wasmMemoryBytes: number;
  /** Dirty jobs in the native pending queue, excluding running/drained JS callbacks. */
  pendingEffectCount: number;
  /** Native queue storage entries, including expired tokens waiting for collection. */
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
