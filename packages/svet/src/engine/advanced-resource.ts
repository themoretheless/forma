/**
 * Async reactive resources (`createResource`), `<Suspense>`, and `<ErrorBoundary>`.
 */

import { retirableCallback, type SvetContext, useContext } from './advanced-context.ts';
import {
  captureViewLifetime,
  type ChildNodeValue,
  insertChild,
  mountOwnedViewNodes,
} from './dom.ts';
import {
  batch,
  createRoot,
  effect,
  type EffectHandle,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  type OwnerScope,
  registerReactiveResetFinalizer,
  runUntrackedInCore,
  runWithOwnerScope,
  signal,
  type Signal,
  untrack,
} from './reactivity.ts';
import { getWasmCore, type SvetWasmExports, withWasmCore } from './wasm-bridge.ts';

interface SuspenseController {
  increment(): void;
  decrement(): void;
}

// Built-in protocols must agree across compatible copies sharing an owner/core.
// User createContext calls intentionally retain unique identities.
const SuspenseContext: Pick<SvetContext<SuspenseController | null>, 'id' | 'defaultValue'> = {
  id: Symbol.for('svet.context.suspense.v1'),
  defaultValue: null,
} as const;

const ErrorBoundaryContext: Pick<
  SvetContext<((err: Error) => void) | null>,
  'id' | 'defaultValue'
> = {
  id: Symbol.for('svet.context.error-boundary.v1'),
  defaultValue: null,
} as const;

export interface ResourceFetcherContext {
  readonly signal: AbortSignal;
}

export interface Resource<T> {
  (): T | undefined;
  readonly loading: Signal<boolean>;
  readonly error: Signal<Error | null>;
  refetch(): Promise<T | undefined>;
  mutate(value: T | undefined): void;
  abort(): void;
  /** Permanently cancels this resource and releases its signals and source observer. */
  dispose(): void;
}

type ResourceRuntimeState<T, S> = {
  core: SvetWasmExports;
  owner: OwnerScope | null;
  generation: number;
  hasSource: boolean;
  sourceFn: () => S;
  invokeFetch: (srcVal: S, abortSignal: AbortSignal) => PromiseLike<T>;
  data: Signal<T | undefined>;
  loading: Signal<boolean>;
  error: Signal<Error | null>;
  suspense: SuspenseController | null;
  reportError: ((err: Error) => void) | null;
  fetchVersion: number;
  activeAbortController: AbortController | null;
  activeCancellation: (() => void) | null;
  unregisterReset: (() => void) | null;
  isDisposed: boolean;
  pending: boolean;
  sourceObserver: EffectHandle | undefined;
  isCurrent: () => boolean;
};

function finishResourceLoading<T, S>(state: ResourceRuntimeState<T, S>): void {
  if (!state.pending) return;
  state.pending = false;
  try {
    state.loading.set(false);
  } finally {
    state.suspense?.decrement();
  }
}

function abortResourceCurrent<T, S>(state: ResourceRuntimeState<T, S>): void {
  runUntrackedInCore(state.core, () => {
    const controller = state.activeAbortController,
      cancel = state.activeCancellation;
    state.activeAbortController = null;
    state.activeCancellation = null;
    const unregister = state.unregisterReset;
    state.unregisterReset = null;
    unregister?.();
    // Settle the public wait independently of an uncooperative fetcher. Clear
    // both slots before synchronous abort listeners can start a replacement.
    cancel?.();
    controller?.abort();
  });
}

function launchResourceLoad<T, S>(
  state: ResourceRuntimeState<T, S>,
  srcVal: S,
): Promise<T | undefined> {
  let cancel!: () => void;
  const cancellation = new Promise<undefined>(resolve => {
    cancel = () => resolve(undefined);
  });
  // Keep publication in performResourceLoad's direct await continuation, preserving
  // its ordering relative to observers awaiting the fetcher's original Promise.
  // race also observes eventual failures after cancellation has won.
  return Promise.race([
    runUntrackedInCore(state.core, () => performResourceLoad(state, srcVal, cancel)),
    cancellation,
  ]);
}

async function performResourceLoad<T, S>(
  state: ResourceRuntimeState<T, S>,
  srcVal: S,
  cancel: () => void,
): Promise<T | undefined> {
  if (!state.isCurrent()) return undefined;
  const currentVersion = ++state.fetchVersion;
  abortResourceCurrent(state);
  if (!state.isCurrent() || currentVersion !== state.fetchVersion) return undefined;
  const controller = new AbortController();
  state.activeAbortController = controller;
  state.activeCancellation = cancel;
  state.unregisterReset = registerReactiveResetFinalizer(state.core, () => {
    if (currentVersion !== state.fetchVersion) return;
    const version = ++state.fetchVersion;
    abortResourceCurrent(state);
    if (version === state.fetchVersion) finishResourceLoading(state);
  });
  const requestCurrent = () =>
    state.isCurrent() && currentVersion === state.fetchVersion && !controller.signal.aborted;

  try {
    if (!state.pending) {
      state.pending = true;
      state.suspense?.increment();
    }
    batch(() => {
      state.loading.set(true);
      state.error.set(null);
    });

    if (!requestCurrent()) return undefined;
    const result = await state.invokeFetch(srcVal, controller.signal);
    runUntrackedInCore(state.core, () => {
      if (requestCurrent()) {
        batch(() => {
          state.data.set(result);
          finishResourceLoading(state);
        });
      }
    });
    return requestCurrent() ? result : undefined;
  } catch (err) {
    runUntrackedInCore(state.core, () => {
      if (!requestCurrent()) return;
      const normalized = err instanceof Error ? err : new Error(String(err));
      if (!requestCurrent()) return;
      batch(() => {
        state.error.set(normalized);
        finishResourceLoading(state);
      });
      if (requestCurrent() && state.reportError) state.reportError(normalized);
    });
    return undefined;
  } finally {
    runUntrackedInCore(state.core, () => {
      if (currentVersion === state.fetchVersion) {
        state.activeAbortController = null;
        state.activeCancellation = null;
        const unregister = state.unregisterReset;
        state.unregisterReset = null;
        unregister?.();
        finishResourceLoading(state);
      }
    });
  }
}

function disposeResourceState<T, S>(
  state: ResourceRuntimeState<T, S>,
  disposeHook: () => void,
): void {
  runUntrackedInCore(state.core, () => {
    if (state.isDisposed) return;
    state.isDisposed = true;
    ++state.fetchVersion;
    if (state.owner && !state.owner.disposed) {
      const index = state.owner.disposables.indexOf(disposeHook);
      if (index >= 0) state.owner.disposables.splice(index, 1);
    }
    try {
      abortResourceCurrent(state);
      finishResourceLoading(state);
    } finally {
      try {
        state.sourceObserver?.dispose();
      } finally {
        state.data.dispose();
        state.loading.dispose();
        state.error.dispose();
      }
    }
  });
}

/**
 * Creates an async reactive resource (`createResource`).
 * Automatically notifies any enclosing `<Suspense>` boundary while loading, forwards
 * unhandled rejections to any enclosing `<ErrorBoundary>`, and automatically aborts
 * in-flight requests via `AbortController` when the source changes or the owner scope disposes!
 */
export function createResource<T, S = true>(
  sourceOrFetcher: (() => S) | ((ctx: ResourceFetcherContext) => PromiseLike<T>),
  maybeFetcher?: (source: S, ctx: ResourceFetcherContext) => PromiseLike<T>,
  options?: { initialValue?: T },
): Resource<T> {
  const owner = getCurrentOwner(),
    core = getWasmCore(),
    generation = getReactiveGeneration(core);
  const assertConstructionCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Resource construction owner disposed');
    if (generation !== getReactiveGeneration(core))
      throw new Error('Resource graph reset during construction');
  };
  assertConstructionCurrent();
  const hasSource = typeof maybeFetcher === 'function';
  const sourceFn = hasSource ? (sourceOrFetcher as () => S) : () => true as unknown as S;
  const invokeFetch = (srcVal: S, abortSignal: AbortSignal) =>
    hasSource
      ? maybeFetcher!(srcVal, { signal: abortSignal })
      : (sourceOrFetcher as (ctx: ResourceFetcherContext) => PromiseLike<T>)({
          signal: abortSignal,
        });

  const initialValue = options?.initialValue;
  assertConstructionCurrent();
  const data = signal<T | undefined>(initialValue);
  const loading = signal<boolean>(false);
  const error = signal<Error | null>(null);
  const suspense = useContext(SuspenseContext);
  const reportError = useContext(ErrorBoundaryContext);

  const state: ResourceRuntimeState<T, S> = {
    core,
    owner,
    generation,
    hasSource,
    sourceFn,
    invokeFetch,
    data,
    loading,
    error,
    suspense,
    reportError,
    fetchVersion: 0,
    activeAbortController: null,
    activeCancellation: null,
    unregisterReset: null,
    isDisposed: false,
    pending: false,
    sourceObserver: undefined,
    isCurrent: () =>
      !state.isDisposed &&
      !isOwnerScopeDisposed(owner) &&
      generation === getReactiveGeneration(core),
  };

  const dispose = () => disposeResourceState(state, dispose);
  onCleanup(dispose);

  try {
    state.sourceObserver = effect(() => {
      const srcVal = sourceFn();
      if (srcVal !== false && srcVal !== null && srcVal !== undefined) {
        untrack(() => {
          void launchResourceLoad(state, srcVal);
        });
      } else {
        untrack(() => {
          const version = ++state.fetchVersion;
          abortResourceCurrent(state);
          if (version === state.fetchVersion) finishResourceLoading(state);
        });
      }
    });
    assertConstructionCurrent();
  } catch (failure) {
    try {
      dispose();
    } catch {
      /* Preserve source initialization failure. */
    }
    throw failure;
  }

  const accessor = (() => {
    const err = error();
    if (err && reportError) reportError(err);
    return data();
  }) as Resource<T>;
  Object.defineProperty(accessor, 'loading', { value: loading, enumerable: true });
  Object.defineProperty(accessor, 'error', { value: error, enumerable: true });
  accessor.dispose = dispose;
  accessor.refetch = async () =>
    state.isCurrent()
      ? runUntrackedInCore(core, () =>
          state.isCurrent() ? launchResourceLoad(state, sourceFn()) : undefined,
        )
      : undefined;
  accessor.mutate = (val: T | undefined) => {
    if (state.isCurrent()) data.set(val);
  };
  accessor.abort = () =>
    runUntrackedInCore(core, () => {
      if (state.isDisposed) return;
      const version = ++state.fetchVersion;
      abortResourceCurrent(state);
      if (version === state.fetchVersion) finishResourceLoading(state);
    });
  return accessor;
}

/**
 * Coordinates one or more async `createResource` calls inside its subtree,
 * displaying `fallback` while any child resource is pending while keeping the main tree alive.
 */
export function Suspense(props: {
  fallback: ChildNodeValue | (() => Node);
  children: ChildNodeValue | (() => Node);
}): DocumentFragment {
  return runWithOwnerScope([], () => {
    const assertViewCurrent = captureViewLifetime();
    const pendingCount = signal(0);
    onCleanup(pendingCount.dispose);
    const controller: SuspenseController = {
      increment() {
        pendingCount.update(c => c + 1);
      },
      decrement() {
        pendingCount.update(c => Math.max(0, c - 1));
      },
    };

    const frag = document.createDocumentFragment();
    const contentWrapper = document.createElement('div');
    contentWrapper.setAttribute('data-svet-suspense-content', '');

    const fallbackWrapper = document.createElement('div');
    fallbackWrapper.setAttribute('data-svet-suspense-fallback', '');

    try {
      runWithOwnerScope([[SuspenseContext.id, controller]], () => {
        const children = props.children;
        assertViewCurrent();
        const mainNodes = typeof children === 'function' ? (children as () => Node)() : children;
        assertViewCurrent();
        insertChild(contentWrapper, mainNodes, assertViewCurrent);
        assertViewCurrent();
      });

      assertViewCurrent();
      const fallback = props.fallback;
      assertViewCurrent();
      const fbNodes = typeof fallback === 'function' ? (fallback as () => Node)() : fallback;
      assertViewCurrent();
      insertChild(fallbackWrapper, fbNodes, assertViewCurrent);
      assertViewCurrent();

      effect(() => {
        assertViewCurrent();
        const isPending = pendingCount() > 0;
        contentWrapper.style.display = isPending ? 'none' : 'contents';
        assertViewCurrent();
        fallbackWrapper.style.display = isPending ? 'contents' : 'none';
        assertViewCurrent();
      });

      assertViewCurrent();
      frag.appendChild(fallbackWrapper);
      assertViewCurrent();
      frag.appendChild(contentWrapper);
      assertViewCurrent();
      return frag;
    } catch (failure) {
      for (const wrapper of [fallbackWrapper, contentWrapper]) {
        try {
          wrapper.parentNode?.removeChild(wrapper);
        } catch {
          /* Preserve the construction failure. */
        }
      }
      throw failure;
    }
  });
}

/**
 * Catches synchronous rendering/effect errors AND async `createResource` errors in its subtree,
 * rendering `fallback(error, reset)` and allowing full recovery when `reset()` is invoked.
 */
export function ErrorBoundary(props: {
  fallback: (error: Error, reset: () => void) => Node;
  children: ChildNodeValue | (() => Node);
}): DocumentFragment {
  return runWithOwnerScope([], () => {
    const assertViewCurrent = captureViewLifetime();
    const caughtError = signal<Error | null>(null);
    const attemptCounter = signal(0);
    onCleanup(() => {
      caughtError.dispose();
      attemptCounter.dispose();
    });
    const frag = document.createDocumentFragment();
    const anchor = document.createComment('svet-error-boundary');
    frag.appendChild(anchor);

    let currentNodes: Node[] = [];
    let disposeCurrentScope: (() => void) | null = null;

    const clearCurrent = () => {
      if (disposeCurrentScope) {
        disposeCurrentScope();
        disposeCurrentScope = null;
      }
      for (const n of currentNodes) {
        if (n.parentNode) {
          n.parentNode.removeChild(n);
        }
      }
      currentNodes = [];
    };

    onCleanup(clearCurrent);

    const mountSubtree = (nodesToMount: Node) => {
      currentNodes = mountOwnedViewNodes(frag, anchor, nodesToMount, assertViewCurrent);
    };

    // Retry can be called outside this copy's ambient core (e.g. host SSR).
    // A weak reference avoids extending core lifetime through a retained retry.
    const boundaryCore = new WeakRef(getWasmCore());
    const retry = retirableCallback(() => {
      const core = boundaryCore.deref();
      if (!core) return;
      withWasmCore(core, () =>
        batch(() => {
          caughtError.set(null);
          attemptCounter.update(c => c + 1);
        }),
      );
    });
    onCleanup(retry.dispose);
    const reset = retry.invoke;

    effect(() => {
      const err = caughtError();
      attemptCounter();
      clearCurrent();

      const mountErrorFallback = (errorObj: Error) => {
        assertViewCurrent();
        clearCurrent();
        createRoot(dispose => {
          disposeCurrentScope = dispose;
          const fbNode = untrack(() => props.fallback(errorObj, reset));
          mountSubtree(fbNode);
        });
      };

      if (err) {
        mountErrorFallback(err);
      } else {
        createRoot(dispose => {
          disposeCurrentScope = dispose;
          try {
            const childNode = runWithOwnerScope(
              [[ErrorBoundaryContext.id, (asyncErr: Error) => caughtError.set(asyncErr)]],
              () =>
                untrack(() =>
                  typeof props.children === 'function'
                    ? (props.children as () => Node)()
                    : (props.children as Node),
                ),
            );
            mountSubtree(childNode);
          } catch (syncErr) {
            const normalized = syncErr instanceof Error ? syncErr : new Error(String(syncErr));
            mountErrorFallback(normalized);
          }
        });
      }
    });

    return frag;
  });
}
