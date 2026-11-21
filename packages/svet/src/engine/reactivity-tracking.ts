/** Dependency tracking, foreign-core bridging, and effect queue flushing. */

import {
  type AnySignalState,
  batchTransport,
  type ComputedObserverEntry,
  type EffectEntry,
  type EffectHandle,
  flushUnlinkedEffectsToWasm,
  getJsContext,
  type JsReactiveContext,
  materializeObserverStagedDeps,
  MAX_EFFECT_ITERATIONS,
  observerBridges,
  reactiveDomains,
  ReactiveStabilizationError,
  registerReactiveResetFinalizer,
  releaseStagedDeps,
  setNodeStateFast,
  type Signal,
  sourceBridges,
  type ReadonlySignal,
} from './reactivity-context.ts';
import { runInForeignReactiveCore } from './reactivity-owner.ts';
import {
  getWasmCore,
  readFlushedEffects,
  readNodeSources,
  STATE_CLEAN,
  type SvetWasmExports,
  withWasmCore,
} from './wasm-bridge.ts';

const SEEN_DEPS_THRESHOLD = 32;

export function linkDirectSubscriber(st: AnySignalState): void {
  const direct = st.singleSubEffect;
  if (direct && !direct.disposed && !direct.wasmLinked) {
    direct.wasmLinked = true;
    st.core.svet_link_1(direct.id, st.id);
  }
}

export function tryRegisterDirectUnlinked(st: AnySignalState, eff: EffectEntry): boolean {
  if (st.disposed) return false;
  const direct = st.singleSubEffect;
  if (!st.hasComputedSubscriber && (direct === undefined || direct === eff || direct?.disposed)) {
    st.singleSubEffect = eff;
    return true;
  }
  linkDirectSubscriber(st);
  st.singleSubEffect = null;
  return false;
}

export function invalidateDirectEffect(st: AnySignalState, isComputed = false): void {
  if (st.disposed) return;
  if (isComputed) st.hasComputedSubscriber = true;
  linkDirectSubscriber(st);
  st.singleSubEffect = null;
}

export function requestActiveEffectRerun(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  source: number,
): void {
  for (const entry of ctx.observerStack) {
    if (
      entry.kind !== 2 ||
      entry.disposed ||
      entry.running === 0 ||
      entry.generation !== ctx.generation
    )
      continue;
    const cursor = entry === ctx.activeEntry ? ctx.activeCursor : entry.pausedCursor;
    let wasmSources: Uint32Array | null = null;
    for (let i = 0; i < cursor; i++) {
      let dependency: number | undefined;
      if (i === 0) dependency = entry.dep0;
      else if (i === 1) dependency = entry.dep1;
      else if (entry.stagedDeps !== null) dependency = entry.stagedDeps[i - 2];
      else {
        if (!wasmSources || wasmSources.buffer !== core.memory.buffer) {
          wasmSources = readNodeSources(core, entry.id);
        }
        dependency = wasmSources[i];
      }
      if (dependency === undefined || entry.retiredDependencies?.has(dependency)) continue;
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
export function runQueuedEffects(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  ids: Uint32Array,
  onRun?: (entry: EffectEntry) => void,
): void {
  const entries = Array.from(ids, id => ctx.effectRegistry[id]);
  const lifetimes = entries.map(entry => entry?.lifetime);
  let failed = false,
    failure: unknown;
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i],
      id = ids[i]!;
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

export function holdForeignBatch(core: SvetWasmExports, ctx: JsReactiveContext): void {
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

export function finishForeignBatches(): void {
  let failed = false,
    failure: unknown;
  const deliveries = new Map<SvetWasmExports, number>();
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

export function refreshForeignTracking(
  entry: Pick<ComputedObserverEntry, 'id' | 'core'>,
  ctx: JsReactiveContext,
  seen?: Set<object>,
): void {
  if (!batchTransport.depth || seen?.has(entry)) return;
  seen ??= new Set();
  seen.add(entry);
  for (const bridge of observerBridges.get(entry)?.values() ?? []) bridge.refresh?.();
  const rawSources = readNodeSources(entry.core, entry.id);
  if (rawSources.length === 0) return;
  const first = rawSources[0]!;
  const second = rawSources.length > 1 ? rawSources[1]! : -1;
  const tail = rawSources.length > 2 ? Array.from(rawSources.subarray(2)) : null;
  const dep0 = ctx.computedRegistry[first];
  if (dep0) refreshForeignTracking(dep0, ctx, seen);
  if (second >= 0) {
    const dep1 = ctx.computedRegistry[second];
    if (dep1) refreshForeignTracking(dep1, ctx, seen);
  }
  if (tail) {
    for (let i = 0; i < tail.length; i++) {
      const dependency = ctx.computedRegistry[tail[i]!];
      if (dependency) refreshForeignTracking(dependency, ctx, seen);
    }
  }
}

export function beginForeignTracking(entry: object): void {
  for (const bridge of observerBridges.get(entry)?.values() ?? []) bridge.seen = false;
}

export function retireForeignTracking(entry: object, all = false): void {
  const bridges = observerBridges.get(entry);
  if (!bridges || bridges.size === 0) return;
  for (const bridge of [...bridges.values()]) {
    if (all || !bridge.seen) bridge.close();
  }
}

export function retireForeignSource(source: object): void {
  const bridges = sourceBridges.get(source);
  if (!bridges || bridges.size === 0) return;
  for (const bridge of [...bridges]) bridge.close();
}

/** Native graphs own both dependency edges; the host only forwards source changes. */
export function trackForeignSource<T>(
  sourceCore: SvetWasmExports,
  source: ReadonlySignal<T>,
  createProxy: (val: number) => Signal<number>,
  createForwarder: (fn: () => void) => EffectHandle,
  sourceVersion?: (() => number) | AnySignalState,
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
    const proxy = withWasmCore(targetCore, () => createProxy(0));
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
      let failed = false;
      try {
        source();
      } catch {
        failed = true;
      }
      const version =
        typeof sourceVersion === 'function' ? sourceVersion() : sourceVersion?.valueVersion;
      const changed =
        initialized &&
        (failed
          ? !previousFailed || failureClock !== sourceCtx.globalClock
          : previousFailed || !sourceVersion || previousVersion !== version);
      if (failed) failureClock = sourceCtx.globalClock;
      previousFailed = failed;
      previousVersion = version;
      initialized = true;
      if (changed) proxy.update(v => v + 1);
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
      const previousOwner = sourceCtx.currentOwner;
      sourceCtx.currentOwner = null;
      sourceCtx.ownerDepth++;
      try {
        forwarding = withWasmCore(sourceCore, () => createForwarder(publish));
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
  bridge.proxy();
}

export function trackReadFast(ctx: JsReactiveContext, sourceId: number): void {
  if (reactiveDomains.frames.at(-1) !== ctx) return;
  const entry = ctx.activeEntry;
  if (!entry) return;
  if (entry.retiredDependencies?.delete(sourceId)) {
    if (ctx.activeCursor > 2 && entry.stagedDeps === null) {
      materializeObserverStagedDeps(entry.core, ctx, entry, ctx.activeCursor);
    }
    ctx.activeTopologyChanged = true;
  }
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
  if (entry.dep0 === sourceId || entry.dep1 === sourceId) return;
  const arrIdx = cur - 2;
  let seen = entry.seenDeps;
  let wasmSources: Uint32Array | null = null;
  if (entry.stagedDeps === null && !ctx.activeTopologyChanged && cur < entry.depsCount) {
    wasmSources = readNodeSources(entry.core, entry.id);
    if (cur < wasmSources.length && wasmSources[cur] === sourceId) {
      seen?.add(sourceId);
      ctx.activeCursor = cur + 1;
      return;
    }
  }
  if (seen === null && arrIdx >= SEEN_DEPS_THRESHOLD) {
    seen = entry.seenDeps = new Set();
    if (entry.stagedDeps !== null) {
      const staged = entry.stagedDeps;
      for (let i = 0; i < arrIdx; i++) {
        const dep = staged[i];
        if (dep !== undefined) seen.add(dep);
      }
    } else if (arrIdx > 0) {
      wasmSources ??= readNodeSources(entry.core, entry.id);
      const limit = Math.min(cur, wasmSources.length);
      for (let i = 2; i < limit; i++) {
        seen.add(wasmSources[i]!);
      }
    }
  }
  if (seen !== null) {
    if (seen.has(sourceId)) return;
  } else if (entry.stagedDeps !== null) {
    const staged = entry.stagedDeps;
    for (let i = 0; i < arrIdx; i++) {
      if (staged[i] === sourceId) return;
    }
  } else if (arrIdx > 0) {
    wasmSources ??= readNodeSources(entry.core, entry.id);
    const limit = Math.min(cur, wasmSources.length);
    for (let i = 2; i < limit; i++) {
      if (wasmSources[i] === sourceId) return;
    }
  }
  const deps = materializeObserverStagedDeps(entry.core, ctx, entry, cur);
  if (seen !== null) seen.add(sourceId);
  ctx.activeTopologyChanged = true;
  if (arrIdx < deps.length) {
    deps[arrIdx] = sourceId;
  } else {
    deps.push(sourceId);
  }
  ctx.activeCursor = cur + 1;
}

export function commitTrackingFast(
  core: SvetWasmExports,
  ctx: JsReactiveContext,
  targetId: number,
  entry: EffectEntry | ComputedObserverEntry,
): void {
  try {
    retireForeignTracking(entry);
    entry.seenDeps = null;
    if (entry.sourceRetirementVersion !== ctx.sourceRetirementVersion) {
      ctx.activeTopologyChanged = true;
      entry.sourceRetirementVersion = ctx.sourceRetirementVersion;
    }
    let cur = ctx.activeCursor;
    if (entry.retiredDependencies?.size) {
      if (cur > 2 && entry.stagedDeps === null) {
        materializeObserverStagedDeps(core, ctx, entry, cur);
      }
      const staged = entry.stagedDeps;
      let kept = 0;
      for (let i = 0; i < cur; i++) {
        const source = i === 0 ? entry.dep0 : i === 1 ? entry.dep1 : staged![i - 2]!;
        if (entry.retiredDependencies.has(source)) continue;
        if (kept === 0) entry.dep0 = source;
        else if (kept === 1) entry.dep1 = source;
        else staged![kept - 2] = source;
        kept++;
      }
      if (kept !== cur) ctx.activeTopologyChanged = true;
      cur = kept;
      ctx.activeCursor = cur;
      if (cur > 2 && staged) staged.length = cur - 2;
    }
    entry.retiredDependencies = null;
    if (cur !== entry.depsCount) {
      entry.depsCount = cur;
      ctx.activeTopologyChanged = true;
    }
    if (!ctx.activeTopologyChanged) {
      setNodeStateFast(core, ctx, targetId, STATE_CLEAN);
      return;
    }
    if (cur > 2 && entry.stagedDeps === null) {
      materializeObserverStagedDeps(core, ctx, entry, cur);
    }
    const staged = entry.stagedDeps;
    ctx.topologyVersion++;
    if (entry.kind === 2 && cur === 1 && !entry.wasmLinked && ctx.batchDepth === 0) {
      const sig = ctx.signalRegistry[entry.dep0];
      if (sig && tryRegisterDirectUnlinked(sig, entry)) {
        if (!entry.inUnlinkedList) {
          entry.inUnlinkedList = true;
          ctx.unlinkedEffects.push(entry);
        }
        return;
      }
    }
    if (entry.kind === 1) {
      if (cur >= 1) invalidateOptionalDirect(ctx, entry.dep0, true);
      if (cur >= 2) invalidateOptionalDirect(ctx, entry.dep1, true);
      for (let i = 2; i < cur; i++) invalidateOptionalDirect(ctx, staged![i - 2]!, true);
    } else if (entry.kind === 2 && cur >= 1) {
      invalidateOptionalDirect(ctx, entry.dep0, false);
      if (cur >= 2) invalidateOptionalDirect(ctx, entry.dep1, false);
      for (let i = 2; i < cur; i++) invalidateOptionalDirect(ctx, staged![i - 2]!, false);
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
      for (let i = 2; i < cur; i++) buf[i] = staged![i - 2]!;
      core.svet_sync_sources_buffered(targetId, cur);
    }
  } finally {
    releaseStagedDeps(ctx, entry);
  }
}

function invalidateOptionalDirect(
  ctx: JsReactiveContext,
  depId: number,
  isComputed: boolean,
): void {
  const sig = ctx.signalRegistry[depId];
  if (sig) invalidateDirectEffect(sig, isComputed);
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
  requestActiveEffectRerun(core, ctx, id);
  if (ctx.isRunningEffect) {
    ctx.cascadedDuringEffect = true;
    core.svet_trigger_write(id);
    return;
  }
  const res = core.svet_trigger_and_flush(id);
  if (res > 0) {
    const entry = ctx.effectRegistry[res - 1];
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
    const copy = new Uint32Array(core.memory.buffer, core.svet_get_flush_ptr(), -res).slice();
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
        const id = queue[0]!,
          entry = ctx.effectRegistry[id];
        if (entry && !entry.disposed && core.svet_get_state(id) !== STATE_CLEAN) {
          onRun(entry);
          entry.run();
        }
      } else runQueuedEffects(core, ctx, queue.slice(), onRun);
      if (generation !== ctx.generation) break;
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
