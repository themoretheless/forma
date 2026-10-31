/**
 * # Svet Application Primitives (`advanced.ts`)
 *
 * Completes the Svet ecosystem with zero external dependencies:
 * 1. **Dependency Injection (`createContext` / `useContext`)**: Hierarchical context lookup
 *    along the `OwnerScope` tree — zero VDOM provider wrapper overhead.
 * 2. **Portals (`<Portal mount={target}>`)**: Renders a reactive DOM subtree into any external
 *    DOM container (modals, tooltips, overlays) while keeping full `OwnerScope` lifecycle cleanup
 *    and context inheritance from its logical parent!
 * 3. **Async Resources & Boundaries (`createResource`, `<Suspense>`, `<ErrorBoundary>`)**:
 *    First-class reactive async data fetching with automatic `<Suspense>` loading orchestration
 *    and `<ErrorBoundary>` fault isolation with recovery (`reset`).
 * 4. **Reactive Router (`createRouter`, `<RouterView>`, `<Link>`)**: Signal-backed client/SSR
 *    router with dynamic parameters (`/users/:id`), programmatic navigation, and automatic
 *    `OwnerScope` disposal when switching routes.
 */

import { type ChildNodeValue, h, insertChild, captureViewLifetime, mountOwnedViewNodes, withDocumentAdoption } from './dom.ts';
import {
  batch,
  createRoot,
  effect,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  registerReactiveResetFinalizer,
  runUntrackedInCore,
  type EffectHandle,
  lookupOwnerContext,
  onCleanup,
  type ReadonlySignal,
  runWithOwnerScope,
  signal,
  type Signal,
  untrack,
} from './reactivity.ts';
import { getWasmCore } from './wasm-bridge.ts';

// ============================================================================
// 1. Hierarchical Context System (`createContext` / `useContext`)
// ============================================================================

export interface SvetContext<T> {
  readonly id: symbol;
  readonly defaultValue: T;
  Provider: (props: { value: T; children: ChildNodeValue | (() => Node) }) => DocumentFragment;
}

export function createContext<T>(defaultValue: T): SvetContext<T> {
  const id = Symbol('svet-context');

  const Provider = (props: { value: T; children: ChildNodeValue | (() => Node) }): DocumentFragment => {
    const assertCallerCurrent = captureViewLifetime();
    assertCallerCurrent();
    return runWithOwnerScope([], () => {
      const assertConstructionCurrent = captureViewLifetime();
      const assertConstruction = () => {assertCallerCurrent();assertConstructionCurrent();};
      assertConstruction();
      const value = props.value;
      assertConstruction();
      return runWithOwnerScope([[id, value]], () => {
        const assertProviderCurrent = captureViewLifetime();
        const assertCurrent = () => {assertConstruction();assertProviderCurrent();};
        assertCurrent();
        const children = props.children;
        assertCurrent();
        const frag = document.createDocumentFragment();
        assertCurrent();
        const content = typeof children === 'function' ? (children as () => Node)() : children;
        assertCurrent();
        insertChild(frag, content, assertCurrent);
        assertCurrent();
        return frag;
      });
    });
  };

  return { id, defaultValue, Provider };
}

export function useContext<T>(context: SvetContext<T>): T {
  return lookupOwnerContext(context.id, context.defaultValue);
}

// ============================================================================
// 2. Portals (`<Portal mount={...}>`)
// ============================================================================

export function Portal(props: {
  mount?: HTMLElement;
  children: ChildNodeValue | (() => Node);
}): Comment {
  return runWithOwnerScope([], () => {
    const owner = getCurrentOwner()!, core = getWasmCore(), generation = getReactiveGeneration(core);
    const assertCurrent = () => {
      if (owner.disposed) throw new Error('Portal construction owner disposed');
      if (generation !== getReactiveGeneration(core)) throw new Error('Reactive graph reset during portal construction');
    };
    const placeholder = document.createComment('svet-portal');
    const container = document.createElement('div');
    container.setAttribute('data-svet-portal', '');
    // Register before user rendering/insertion so partial attachment can be undone.
    const removeContainer = () => { container.parentNode?.removeChild(container); };
    onCleanup(removeContainer);
    try {
      const target = props.mount ?? document.body;
      assertCurrent();
      const children = props.children;
      assertCurrent();
      const content = typeof children === 'function' ? (children as () => Node)() : children;
      assertCurrent();
      insertChild(container, content, assertCurrent);
      assertCurrent();
      withDocumentAdoption(target, container, () => { target.appendChild(container); }, assertCurrent);
      assertCurrent();
      return placeholder;
    } catch (failure) {
      // An insertion hook can attach after owner cleanup has already run.
      try { removeContainer(); } catch { /* Preserve the construction failure. */ }
      throw failure;
    }
  });
}

// ============================================================================
// 3. Async Resources, `<Suspense>`, and `<ErrorBoundary>`
// ============================================================================

interface SuspenseController {
  increment(): void;
  decrement(): void;
}

const SuspenseContext = createContext<SuspenseController | null>(null);
const ErrorBoundaryContext = createContext<((err: Error) => void) | null>(null);

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

/**
 * Creates an async reactive resource (`createResource`).
 * Automatically notifies any enclosing `<Suspense>` boundary while loading, forwards
 * unhandled rejections to any enclosing `<ErrorBoundary>`, and automatically aborts
 * in-flight requests via `AbortController` when the source changes or the owner scope disposes!
 */
export function createResource<T, S = true>(
  sourceOrFetcher: (() => S) | ((ctx: ResourceFetcherContext) => Promise<T>),
  maybeFetcher?: (source: S, ctx: ResourceFetcherContext) => Promise<T>,
  options?: { initialValue?: T }
): Resource<T> {
  const owner = getCurrentOwner(), core = getWasmCore(), generation = getReactiveGeneration(core);
  const assertConstructionCurrent = () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('Resource construction owner disposed');
    if (generation !== getReactiveGeneration(core)) throw new Error('Resource graph reset during construction');
  };
  assertConstructionCurrent();
  const hasSource = typeof maybeFetcher === 'function';
  const sourceFn = hasSource ? (sourceOrFetcher as () => S) : () => true as unknown as S;
  const fetcher = hasSource
    ? maybeFetcher!
    : (sourceOrFetcher as unknown as (s: S, ctx: ResourceFetcherContext) => Promise<T>);

  const initialValue = options?.initialValue;
  assertConstructionCurrent();
  const data = signal<T | undefined>(initialValue);
  const loading = signal<boolean>(false);
  const error = signal<Error | null>(null);

  const suspense = useContext(SuspenseContext);
  const reportError = useContext(ErrorBoundaryContext);

  let fetchVersion = 0;
  let activeAbortController: AbortController | null = null;
  let activeCancellation: (() => void) | null = null;
  let unregisterReset: (() => void) | null = null;
  let isDisposed = false;
  let sourceObserver: EffectHandle | undefined;
  const isCurrent = () => !isDisposed && !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);

  let pending = false;
  const finishLoading = () => {
    if (!pending) return;
    pending = false;
    try { loading.set(false); } finally { suspense?.decrement(); }
  };

  const abortCurrent = () => runUntrackedInCore(core, () => {
    const controller = activeAbortController, cancel = activeCancellation;
    activeAbortController = null;
    activeCancellation = null;
    const unregister = unregisterReset;
    unregisterReset = null;
    unregister?.();
    // Settle the public wait independently of an uncooperative fetcher. Clear
    // both slots before synchronous abort listeners can start a replacement.
    cancel?.();
    controller?.abort();
  });

  const dispose = () => runUntrackedInCore(core, () => {
    if (isDisposed) return;
    isDisposed = true; ++fetchVersion;
    if (owner && !owner.disposed) {
      const index = owner.disposables.indexOf(dispose);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
    try { abortCurrent(); finishLoading(); }
    finally {
      try { sourceObserver?.dispose(); }
      finally { data.dispose(); loading.dispose(); error.dispose(); }
    }
  });
  onCleanup(dispose);

  const performLoad = async (srcVal: S, cancel: () => void): Promise<T | undefined> => {
    if (!isCurrent()) return undefined;
    const currentVersion = ++fetchVersion;
    abortCurrent();
    if (!isCurrent() || currentVersion !== fetchVersion) return undefined;
    const controller = new AbortController();
    activeAbortController = controller;
    activeCancellation = cancel;
    unregisterReset = registerReactiveResetFinalizer(core, () => {
      if (currentVersion !== fetchVersion) return;
      const version = ++fetchVersion;
      abortCurrent();
      if (version === fetchVersion) finishLoading();
    });
    const requestCurrent = () => isCurrent() && currentVersion === fetchVersion && !controller.signal.aborted;

    try {
      if (!pending) {
        pending = true;
        suspense?.increment();
      }
      batch(() => {
        loading.set(true);
        error.set(null);
      });

      if (!requestCurrent()) return undefined;
      const result = await (hasSource
        ? fetcher(srcVal, { signal: controller.signal })
        : (sourceOrFetcher as (ctx: ResourceFetcherContext) => Promise<T>)({
            signal: controller.signal,
          }));
      runUntrackedInCore(core, () => {
        if (requestCurrent()) {
          batch(() => {
            data.set(result);
            finishLoading();
          });
        }
      });
      return requestCurrent() ? result : undefined;
    } catch (err) {
      runUntrackedInCore(core, () => {
        if (!requestCurrent()) return;
        const normalized = err instanceof Error ? err : new Error(String(err));
        if (!requestCurrent()) return;
        batch(() => {
          error.set(normalized);
          finishLoading();
        });
        if (requestCurrent() && reportError) reportError(normalized);
      });
      return undefined;
    } finally {
      runUntrackedInCore(core, () => {
        if (currentVersion === fetchVersion) {
          activeAbortController = null;
          activeCancellation = null;
          const unregister = unregisterReset;
          unregisterReset = null;
          unregister?.();
          finishLoading();
        }
      });
    }
  };

  const load = (srcVal: S): Promise<T | undefined> => {
    let cancel!: () => void;
    const cancellation = new Promise<undefined>(resolve => { cancel = () => resolve(undefined); });
    // Keep publication in performLoad's direct await continuation, preserving
    // its ordering relative to observers awaiting the fetcher's original Promise.
    // race also observes eventual failures after cancellation has won.
    return Promise.race([runUntrackedInCore(core, () => performLoad(srcVal, cancel)), cancellation]);
  };

  try {
    sourceObserver = effect(() => {
      const srcVal = sourceFn();
      if (srcVal !== false && srcVal !== null && srcVal !== undefined) {
        untrack(() => {
          void load(srcVal);
        });
      } else {
        untrack(() => {
          const version = ++fetchVersion;
          abortCurrent();
          if (version === fetchVersion) finishLoading();
        });
      }
    });
    assertConstructionCurrent();
  } catch (failure) {
    try { dispose(); } catch { /* Preserve source initialization failure. */ }
    throw failure;
  }

  const accessor = (() => {
    const err = error();
    if (err && reportError) {
      reportError(err);
    }
    return data();
  }) as Resource<T>;

  Object.defineProperty(accessor, 'loading', { value: loading, enumerable: true });
  Object.defineProperty(accessor, 'error', { value: error, enumerable: true });
  accessor.dispose = dispose;
  accessor.refetch = async () => {
    if (!isCurrent()) return undefined;
    return runUntrackedInCore(core, () => isCurrent() ? load(sourceFn()) : undefined);
  };
  accessor.mutate = (val: T | undefined) => { if (isCurrent()) data.set(val); };
  accessor.abort = () => runUntrackedInCore(core, () => {
    if (isDisposed) return;
    const version = ++fetchVersion;
    abortCurrent();
    if (version === fetchVersion) finishLoading();
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
        pendingCount.update((c) => c + 1);
      },
      decrement() {
        pendingCount.update((c) => Math.max(0, c - 1));
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
        try { wrapper.parentNode?.removeChild(wrapper); } catch { /* Preserve the construction failure. */ }
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
    onCleanup(() => { caughtError.dispose(); attemptCounter.dispose(); });
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

    const reset = () => batch(() => {
      caughtError.set(null);
      attemptCounter.update((c) => c + 1);
    });

    effect(() => {
      const err = caughtError();
      attemptCounter();
      clearCurrent();

      const mountErrorFallback = (errorObj: Error) => {
        assertViewCurrent();
        clearCurrent();
        createRoot((dispose) => {
          disposeCurrentScope = dispose;
          const fbNode = untrack(() => props.fallback(errorObj, reset));
          mountSubtree(fbNode);
        });
      };

      if (err) {
        mountErrorFallback(err);
      } else {
        createRoot((dispose) => {
          disposeCurrentScope = dispose;
          try {
            const childNode = runWithOwnerScope(
              [[ErrorBoundaryContext.id, (asyncErr: Error) => caughtError.set(asyncErr)]],
              () =>
                untrack(() =>
                  typeof props.children === 'function'
                    ? (props.children as () => Node)()
                    : (props.children as Node)
                )
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

// ============================================================================
// 4. Reactive Router (`createRouter`, `<RouterView>`, `<Link>`)
// ============================================================================

export interface RouteDefinition {
  path: string;
  component: (params: Record<string, string>) => Node;
}

export type NavigationGuard = (
  to: { path: string; params: Record<string, string>; query: Record<string, string>; hash: string },
  from: { path: string; params: Record<string, string>; query: Record<string, string>; hash: string }
) => boolean | string | void;

export interface RouterInstance {
  readonly path: ReadonlySignal<string>;
  readonly params: ReadonlySignal<Record<string, string>>;
  readonly query: ReadonlySignal<Record<string, string>>;
  readonly hash: ReadonlySignal<string>;
  navigate(to: string): boolean;
  beforeEach(guard: NavigationGuard): () => void;
  match(pathname: string): {
    route: RouteDefinition | null;
    params: Record<string, string>;
    pathname: string;
    query: Record<string, string>;
    hash: string;
  };
  dispose(): void;
}

const RouterContext = createContext<RouterInstance | null>(null);

function parseUrlParts(rawUrl: string): {
  pathname: string;
  query: Record<string, string>;
  hash: string;
} {
  let working = rawUrl || '/';
  let hash = '';
  const hashIdx = working.indexOf('#');
  if (hashIdx !== -1) {
    hash = working.slice(hashIdx + 1);
    working = working.slice(0, hashIdx);
  }
  const query: Record<string, string> = {};
  const qIdx = working.indexOf('?');
  let pathname = working;
  if (qIdx !== -1) {
    pathname = working.slice(0, qIdx) || '/';
    const searchStr = working.slice(qIdx + 1);
    if (searchStr) {
      for (const pair of searchStr.split('&')) {
        if (!pair) continue;
        const eqIdx = pair.indexOf('=');
        const rawK = eqIdx === -1 ? pair : pair.slice(0, eqIdx);
        const rawV = eqIdx === -1 ? '' : pair.slice(eqIdx + 1);
        try {
          const k = decodeURIComponent(rawK.replace(/\+/g, ' '));
          const v = decodeURIComponent(rawV.replace(/\+/g, ' '));
          if (k && k !== '__proto__' && k !== 'constructor' && k !== 'prototype') {
            query[k] = v;
          }
        } catch {
          if (rawK && rawK !== '__proto__' && rawK !== 'constructor' && rawK !== 'prototype') {
            query[rawK] = rawV;
          }
        }
      }
    }
  }
  return { pathname: pathname || '/', query, hash };
}

function decodeRouteSegment(segment: string): string {
  if (!segment.includes('%')) return segment;
  try { return decodeURIComponent(segment); } catch { return segment; }
}

function matchRoutePattern(
  pattern: string,
  pathname: string
): Record<string, string> | null {
  if (pattern === '*') return {};
  const cleanPath = pathname.split('#')[0].split('?')[0];
  const patternParts = pattern.split('/').filter(Boolean);
  const pathParts = cleanPath.split('/').filter(Boolean);

  if (patternParts.length !== pathParts.length) {
    return null;
  }

  const params: Record<string, string> = {};
  for (let i = 0; i < patternParts.length; i++) {
    const p = patternParts[i];
    const actual = decodeRouteSegment(pathParts[i]);
    if (p.startsWith(':')) {
      Object.defineProperty(params, p.slice(1), {
        value: actual, enumerable: true, writable: true, configurable: true,
      });
    } else if (decodeRouteSegment(p) !== actual) {
      return null;
    }
  }
  return params;
}

// Reserved history metadata. The chain belongs to one live router; entries from
// another router/document are deliberately not assigned a guessed distance.
const routerHistoryKey = '__svet_router_history__';
let nextHistoryChain = 0;
type RouterHistoryPosition = {chain: string; index: number};
function readRouterHistoryPosition(state: unknown): RouterHistoryPosition | null {
  const value = state && typeof state === 'object'
    ? (state as Record<string, unknown>)[routerHistoryKey] : null;
  if (!value || typeof value !== 'object') return null;
  const {chain, index} = value as RouterHistoryPosition;
  return typeof chain === 'string' && Number.isSafeInteger(index) && index >= 0 ? {chain, index} : null;
}
function stampRouterHistory(state: unknown, position: RouterHistoryPosition): Record<string, unknown> {
  // Preserve host object fields. Non-record state is retained in an envelope.
  const base = state !== null && typeof state === 'object' && !Array.isArray(state)
    && Object.getPrototypeOf(state) === Object.prototype
    ? state : {__svet_router_state__: state};
  return {...base, [routerHistoryKey]: position};
}

export function createRouter(options: {
  routes: RouteDefinition[];
  initialPath?: string;
  beforeEach?: NavigationGuard;
}): RouterInstance {
  const owner = getCurrentOwner(), core = getWasmCore(), generation = getReactiveGeneration(core);
  if (isOwnerScopeDisposed(owner)) throw new Error('Router construction owner disposed');
  const browserWindow = typeof window !== 'undefined' ? window : undefined;
  const browserOrigin = browserWindow?.location.origin;
  const resolutionBase = browserOrigin && browserOrigin !== 'null' ? `${browserOrigin}/` : 'http://svet.invalid/';
  const origin = new URL(resolutionBase).origin;
  const normalize = (to: string, base: string): string | null => {
    if (typeof to !== 'string') return null;
    try {
      const destination = new URL(to, new URL(base, resolutionBase));
      if (!/^https?:$/.test(destination.protocol) || destination.origin !== origin) return null;
      return `${destination.pathname}${destination.search}${destination.hash}`;
    } catch { return null; }
  };
  const initialGuard = options.beforeEach;
  let disposed = false, navigationVersion = 0;
  const isCurrent = () => !disposed && !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);
  const initialFull =
    options.initialPath ??
    (browserWindow
      ? `${browserWindow.location.pathname}${browserWindow.location.search ?? ''}${browserWindow.location.hash ?? ''}`
      : '/');

  const match = (rawUrl: string) => {
    const { pathname, query, hash } = parseUrlParts(rawUrl);
    for (const route of options.routes) {
      const matchedParams = matchRoutePattern(route.path, pathname);
      if (matchedParams !== null) {
        return { route, params: matchedParams, pathname, query, hash };
      }
    }
    return { route: null, params: {}, pathname, query, hash };
  };

  const initialBase = browserWindow
    ? `${browserWindow.location.pathname}${browserWindow.location.search ?? ''}${browserWindow.location.hash ?? ''}`
    : '/';
  const initialTarget = normalize(initialFull, initialBase);
  if (initialTarget === null) throw new TypeError('Invalid or external initial router URL');
  let currentFull = initialTarget;
  const history = browserWindow?.history;
  const historyChains = new Set<string>();
  let physicalPosition: RouterHistoryPosition | null = null;
  let acceptedPosition: RouterHistoryPosition | null = null;
  let acceptedCommit = {};
  let restoring = false;
  const fullLocation = () => browserWindow
    ? `${browserWindow.location.pathname}${browserWindow.location.search}${browserWindow.location.hash}` : currentFull;
  const ownPosition = (state: unknown): RouterHistoryPosition | null => {
    const position = readRouterHistoryPosition(state);
    return position && historyChains.has(position.chain) ? position : null;
  };
  const stampPhysicalEntry = (): RouterHistoryPosition => {
    const chain = `svet:${Date.now()}:${++nextHistoryChain}`;
    historyChains.add(chain);
    const position = {chain, index: 0};
    history!.replaceState(stampRouterHistory(history!.state, position), '', fullLocation());
    physicalPosition = position;
    return position;
  };

  const initialMatch = match(currentFull);
  if (!isCurrent()) throw new Error('Router context or owner invalidated during construction');
  const currentPath = signal(initialMatch.pathname);
  const currentParams = signal<Record<string, string>>(initialMatch.params);
  const currentQuery = signal<Record<string, string>>(initialMatch.query);
  const currentHash = signal<string>(initialMatch.hash);

  const guards = new Set<NavigationGuard>();
  if (initialGuard) {
    guards.add(initialGuard);
  }

  const beforeEach = (guard: NavigationGuard): (() => void) => {
    if (!isCurrent()) return () => {};
    guards.add(guard);
    return () => {
      guards.delete(guard);
    };
  };

  const syncPathInternal = (to: string, pushHistory: boolean, redirectDepth = 0): boolean => {
    if (!isCurrent() || redirectDepth > 10) return false;
    const normalized = normalize(to, currentFull);
    if (normalized === null) return false;
    const attempt = ++navigationVersion;
    const nextMatch = match(normalized);
    if (!isCurrent() || attempt !== navigationVersion) return false;
    const fromState = {
      path: currentPath.peek(),
      params: currentParams.peek(),
      query: currentQuery.peek(),
      hash: currentHash.peek(),
    };
    const toState = {
      path: nextMatch.pathname,
      params: nextMatch.params,
      query: nextMatch.query,
      hash: nextMatch.hash,
    };

    for (const guard of Array.from(guards)) {
      if (!guards.has(guard)) continue;
      const res = guard(toState, fromState);
      if (!isCurrent() || attempt !== navigationVersion) return false;
      if (res === false) {
        return false;
      }
      if (typeof res === 'string') {
        const redirect = normalize(res, currentFull);
        if (redirect === null) return false;
        if (redirect !== normalized) return syncPathInternal(redirect, pushHistory, redirectDepth + 1);
      }
    }

    if (pushHistory && history) {
      physicalPosition = ownPosition(history.state);
      const previous = physicalPosition ?? stampPhysicalEntry();
      // Retain the initial entry as a rollback destination before the first push.
      if (!acceptedPosition) acceptedPosition = previous;
      const next = {chain: previous.chain, index: previous.index + 1};
      history.pushState(stampRouterHistory({}, next), '', normalized);
      physicalPosition = next;
    } else if (history) {
      // A guard redirect during traversal belongs to the visited slot, not a new push.
      if (normalized !== fullLocation()) history.replaceState(history.state, '', normalized);
      if (!physicalPosition) stampPhysicalEntry();
    }
    if (!isCurrent() || attempt !== navigationVersion) return false;
    currentFull = normalized;
    acceptedPosition = physicalPosition;
    acceptedCommit = {};
    batch(() => {
      currentPath.set(nextMatch.pathname);
      currentParams.set(nextMatch.params);
      currentQuery.set(nextMatch.query);
      currentHash.set(nextMatch.hash);
    });
    return true;
  };

  const navigate = (to: string): boolean => {
    return restoring ? false : syncPathInternal(to, true);
  };

  const restoreAcceptedEntry = () => {
    if (!isCurrent() || !history) return;
    const target = acceptedPosition;
    if (target && physicalPosition && target.chain === physicalPosition.chain
        && target.index !== physicalPosition.index) {
      restoring = true;
      try { history.go(target.index - physicalPosition.index); }
      catch (failure) { restoring = false; throw failure; }
    } else {
      // External entries have no trustworthy relative index. Preserve their data
      // and repair the URL in place rather than destroying forward history.
      restoring = false;
      history.replaceState(history.state, '', currentFull);
      acceptedPosition = stampPhysicalEntry();
    }
  };
  const onPopState = () => {
    if (!isCurrent() || !history) return;
    physicalPosition = ownPosition(history.state);
    if (restoring && acceptedPosition && physicalPosition
        && acceptedPosition.chain === physicalPosition.chain
        && acceptedPosition.index === physicalPosition.index && fullLocation() === currentFull) {
      restoring = false;
      return; // Compensation is not another guarded navigation.
    }
    if (restoring) {
      restoreAcceptedEntry();
      return;
    }
    const previousCommit = acceptedCommit;
    try { syncPathInternal(fullLocation(), false); }
    catch (failure) {
      // Native restoration failure must not hide the original guard failure.
      if (acceptedCommit === previousCommit) {
        try { restoreAcceptedEntry(); } catch { /* Keep the primary transition failure. */ }
      }
      throw failure;
    }
    // A nested successful navigation owns its new URL/position. Rejected guards
    // recover only when no newer commit superseded their transition.
    if (acceptedCommit === previousCommit) restoreAcceptedEntry();
  };
  if (browserWindow && typeof browserWindow.addEventListener === 'function') {
    browserWindow.addEventListener('popstate', onPopState);
  }

  const dispose = () => {
    if (disposed) return;
    disposed = true; navigationVersion++;
    if (owner && !owner.disposed) {
      const index = owner.disposables.indexOf(dispose);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
    if (browserWindow && typeof browserWindow.removeEventListener === 'function') {
      browserWindow.removeEventListener('popstate', onPopState);
    }
    guards.clear();
    historyChains.clear();
    currentPath.dispose();
    currentParams.dispose();
    currentQuery.dispose();
    currentHash.dispose();
  };
  onCleanup(dispose);

  return {
    path: currentPath,
    params: currentParams,
    query: currentQuery,
    hash: currentHash,
    navigate,
    beforeEach,
    match,
    dispose,
  };
}

export function useRouter(): RouterInstance {
  const router = useContext(RouterContext);
  if (!router) {
    throw new Error('useRouter() must be called inside a <RouterView>');
  }
  return router;
}

export function RouterView(props: {
  router: RouterInstance;
  fallback?: () => Node;
}): DocumentFragment {
  return runWithOwnerScope([], () => {
    const assertViewCurrent = captureViewLifetime();
    const router = props.router;
    const frag = document.createDocumentFragment();
    const anchor = document.createComment('svet-router-view');
    frag.appendChild(anchor);

    let currentNodes: Node[] = [];
    let disposeRouteScope: (() => void) | null = null;

    const clearRoute = () => {
      if (disposeRouteScope) {
        disposeRouteScope();
        disposeRouteScope = null;
      }
      for (const n of currentNodes) {
        if (n.parentNode) {
          n.parentNode.removeChild(n);
        }
      }
      currentNodes = [];
    };

    onCleanup(clearRoute);

    effect(() => {
      const pathname = router.path();
      clearRoute();

      const { route, params } = router.match(pathname);
      createRoot((dispose) => {
        disposeRouteScope = dispose;
        runWithOwnerScope([[RouterContext.id, router]], () => {
          const rendered = untrack(() => {
            if (route) {
              return route.component(params);
            }
            return props.fallback ? props.fallback() : document.createTextNode('404 Not Found');
          });

          currentNodes = mountOwnedViewNodes(frag, anchor, rendered, assertViewCurrent);
        });
      });
    });

    return frag;
  });
}

export function Link(props: {
  to: string;
  className?: string;
  target?: string;
  download?: boolean | string;
  rel?: string;
  children?: ChildNodeValue;
}): Node {
  const router = useContext(RouterContext);
  const to = props.to;
  const unsafe = /^(javascript:|vbscript:|data:text\/html)/i.test(to.replace(/[\u0000-\u0020]+/g, ''));
  const safeHref = unsafe ? '#' : to;
  return h(
    'a',
    {
      href: safeHref,
      className: props.className,
      target: props.target,
      download: props.download,
      rel: props.rel,
      onClick: (e: MouseEvent) => {
        if (e.defaultPrevented) return;
        if (unsafe) { e.preventDefault(); return; }
        if (!router || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
        const anchor = e.currentTarget as HTMLAnchorElement | null;
        if (!anchor || anchor.hasAttribute('download')) return;
        const doc = anchor.ownerDocument;
        const target = anchor.getAttribute('target') || doc.querySelector('base[target]')?.getAttribute('target') || '';
        if (target && target.toLowerCase() !== '_self') return;
        const location = doc.defaultView?.location;
        if (!location) return;
        let destination: URL;
        try { destination = new URL(anchor.href, location.href); } catch { return; }
        if (!/^https?:$/.test(destination.protocol) || destination.origin !== location.origin) return;
        e.preventDefault();
        router.navigate(`${destination.pathname}${destination.search}${destination.hash}`);
      },
    },
    props.children
  );
}
