/**
 * # Svet Reactive Primitives (`reactivity.ts`)
 *
 * Public barrel over `reactivity-{context,owner,tracking,core,store}.ts`.
 */

export type { EffectHandle, OwnerScope, ReadonlySignal, Signal } from './reactivity-context.ts';
export {
  allocateReactiveNode,
  disposeTrackedNode,
  getGraphTelemetry,
  getReactiveGeneration,
  installAsyncOwnerResolver,
  invalidateSingleSignalFastPath,
  ReactiveContextDisposedError,
  ReactiveCycleError,
  ReactiveStabilizationError,
  registerReactiveResetCleanup,
  registerReactiveResetFinalizer,
  resetReactiveSystem,
  retireReactiveContextCache,
} from './reactivity-context.ts';
export type { CapturedReactiveContext } from './reactivity-owner.ts';
export {
  adoptCurrentOwnerIntoRoot,
  captureReactiveContext,
  createRoot,
  currentOwnerIsEmpty,
  disposeDetachedScope,
  getCurrentOwner,
  isOwnerScopeDisposed,
  lookupOwnerContext,
  onCleanup,
  runInDetachedScope,
  runUntrackedInCore,
  runWithAsyncRequestScope,
  runWithCapturedOwner,
  runWithIsolatedContext,
  runWithOwnerScope,
  untrack,
} from './reactivity-owner.ts';
export { flushEffects, trackNodeRead, triggerNodeWrite } from './reactivity-tracking.ts';
export {
  batch,
  computed,
  disposeSignalNow,
  effect,
  setSignalNow,
  signal,
} from './reactivity-core.ts';
export type { DeepSignalStore, SignalStore } from './reactivity-store.ts';
export { createDeepStore, createStore } from './reactivity-store.ts';
