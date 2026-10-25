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

import { type ChildNodeValue, h, insertChild } from './dom';
import {
  batch,
  createRoot,
  effect,
  lookupOwnerContext,
  onCleanup,
  ReadonlySignal,
  runWithOwnerScope,
  signal,
  Signal,
  untrack,
} from './reactivity';

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
    return runWithOwnerScope([[id, props.value]], () => {
      const frag = document.createDocumentFragment();
      const content =
        typeof props.children === 'function' ? (props.children as () => Node)() : props.children;
      insertChild(frag, content);
      return frag;
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
  const placeholder = document.createComment('svet-portal');
  const target = props.mount ?? document.body;
  const container = document.createElement('div');
  container.setAttribute('data-svet-portal', '');

  const content =
    typeof props.children === 'function' ? (props.children as () => Node)() : props.children;
  insertChild(container, content);
  target.appendChild(container);

  onCleanup(() => {
    if (container.parentNode) {
      container.parentNode.removeChild(container);
    }
  });

  return placeholder;
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
  const hasSource = typeof maybeFetcher === 'function';
  const sourceFn = hasSource ? (sourceOrFetcher as () => S) : () => true as unknown as S;
  const fetcher = hasSource
    ? maybeFetcher!
    : (sourceOrFetcher as unknown as (s: S, ctx: ResourceFetcherContext) => Promise<T>);

  const data = signal<T | undefined>(options?.initialValue);
  const loading = signal<boolean>(false);
  const error = signal<Error | null>(null);

  const suspense = useContext(SuspenseContext);
  const reportError = useContext(ErrorBoundaryContext);

  let fetchVersion = 0;
  let activeAbortController: AbortController | null = null;
  let isDisposed = false;

  const abortCurrent = () => {
    if (activeAbortController) {
      activeAbortController.abort();
      activeAbortController = null;
    }
  };

  onCleanup(() => {
    isDisposed = true;
    abortCurrent();
  });

  const load = async (srcVal: S): Promise<T | undefined> => {
    abortCurrent();
    const currentVersion = ++fetchVersion;
    const controller = new AbortController();
    activeAbortController = controller;

    const wasLoading = loading.peek();
    loading.set(true);
    error.set(null);
    if (!wasLoading && suspense) {
      suspense.increment();
    }

    try {
      const result = await (hasSource
        ? fetcher(srcVal, { signal: controller.signal })
        : (sourceOrFetcher as (ctx: ResourceFetcherContext) => Promise<T>)({
            signal: controller.signal,
          }));
      if (!isDisposed && currentVersion === fetchVersion && !controller.signal.aborted) {
        batch(() => {
          data.set(result);
          loading.set(false);
        });
      }
      return result;
    } catch (err) {
      if (isDisposed || controller.signal.aborted) {
        return undefined;
      }
      const normalized = err instanceof Error ? err : new Error(String(err));
      if (currentVersion === fetchVersion) {
        batch(() => {
          error.set(normalized);
          loading.set(false);
        });
        if (reportError) {
          reportError(normalized);
        }
      }
      return undefined;
    } finally {
      if (!wasLoading && suspense) {
        suspense.decrement();
      }
    }
  };

  effect(() => {
    const srcVal = sourceFn();
    if (srcVal !== false && srcVal !== null && srcVal !== undefined) {
      untrack(() => {
        void load(srcVal);
      });
    }
  });

  const accessor = (() => {
    const err = error();
    if (err && reportError) {
      reportError(err);
    }
    return data();
  }) as Resource<T>;

  Object.defineProperty(accessor, 'loading', { value: loading, enumerable: true });
  Object.defineProperty(accessor, 'error', { value: error, enumerable: true });
  accessor.refetch = () => load(untrack(sourceFn));
  accessor.mutate = (val: T | undefined) => data.set(val);
  accessor.abort = abortCurrent;

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
  const pendingCount = signal(0);
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

  runWithOwnerScope([[SuspenseContext.id, controller]], () => {
    const mainNodes =
      typeof props.children === 'function' ? (props.children as () => Node)() : props.children;
    insertChild(contentWrapper, mainNodes);
  });

  const fbNodes =
    typeof props.fallback === 'function' ? (props.fallback as () => Node)() : props.fallback;
  insertChild(fallbackWrapper, fbNodes);

  effect(() => {
    const isPending = pendingCount() > 0;
    contentWrapper.style.display = isPending ? 'none' : 'contents';
    fallbackWrapper.style.display = isPending ? 'contents' : 'none';
  });

  frag.appendChild(fallbackWrapper);
  frag.appendChild(contentWrapper);
  return frag;
}

/**
 * Catches synchronous rendering/effect errors AND async `createResource` errors in its subtree,
 * rendering `fallback(error, reset)` and allowing full recovery when `reset()` is invoked.
 */
export function ErrorBoundary(props: {
  fallback: (error: Error, reset: () => void) => Node;
  children: ChildNodeValue | (() => Node);
}): DocumentFragment {
  const caughtError = signal<Error | null>(null);
  const attemptCounter = signal(0);
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

  const mountSubtree = (nodesToMount: Node) => {
    const temp = document.createDocumentFragment();
    insertChild(temp, nodesToMount);
    const list = Array.from(temp.childNodes);
    if (anchor.parentNode) {
      for (const n of list) {
        anchor.parentNode.insertBefore(n, anchor);
      }
    } else {
      for (const n of list) {
        frag.insertBefore(n, anchor);
      }
    }
    currentNodes = list;
  };

  const reset = () => {
    caughtError.set(null);
    attemptCounter.update((c) => c + 1);
  };

  effect(() => {
    const err = caughtError();
    attemptCounter();
    clearCurrent();

    const mountErrorFallback = (errorObj: Error) => {
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
}

// ============================================================================
// 4. Reactive Router (`createRouter`, `<RouterView>`, `<Link>`)
// ============================================================================

export interface RouteDefinition {
  path: string;
  component: (params: Record<string, string>) => Node;
}

export interface RouterInstance {
  readonly path: ReadonlySignal<string>;
  readonly params: ReadonlySignal<Record<string, string>>;
  navigate(to: string): void;
  match(pathname: string): {
    route: RouteDefinition | null;
    params: Record<string, string>;
  };
  dispose(): void;
}

const RouterContext = createContext<RouterInstance | null>(null);

function matchRoutePattern(
  pattern: string,
  pathname: string
): Record<string, string> | null {
  if (pattern === '*') return {};
  const patternParts = pattern.split('/').filter(Boolean);
  const pathParts = pathname.split('?')[0].split('/').filter(Boolean);

  if (patternParts.length !== pathParts.length) {
    return null;
  }

  const params: Record<string, string> = {};
  for (let i = 0; i < patternParts.length; i++) {
    const p = patternParts[i];
    const actual = pathParts[i];
    if (p.startsWith(':')) {
      try {
        params[p.slice(1)] = decodeURIComponent(actual);
      } catch {
        params[p.slice(1)] = actual;
      }
    } else if (p !== actual) {
      return null;
    }
  }
  return params;
}

export function createRouter(options: {
  routes: RouteDefinition[];
  initialPath?: string;
}): RouterInstance {
  const initial =
    options.initialPath ??
    (typeof window !== 'undefined' ? window.location.pathname : '/');
  const currentPath = signal(initial);
  const currentParams = signal<Record<string, string>>({});

  const match = (pathname: string) => {
    for (const route of options.routes) {
      const matchedParams = matchRoutePattern(route.path, pathname);
      if (matchedParams !== null) {
        return { route, params: matchedParams };
      }
    }
    return { route: null, params: {} };
  };

  const syncPathInternal = (to: string, pushHistory: boolean) => {
    if (pushHistory && typeof window !== 'undefined' && window.history?.pushState) {
      window.history.pushState({}, '', to);
    }
    const m = match(to);
    batch(() => {
      currentPath.set(to);
      currentParams.set(m.params);
    });
  };

  const navigate = (to: string) => {
    syncPathInternal(to, true);
  };

  // Listen to browser Back/Forward (`popstate`) automatically!
  const onPopState = () => {
    if (typeof window !== 'undefined') {
      syncPathInternal(window.location.pathname, false);
    }
  };
  if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
    window.addEventListener('popstate', onPopState);
  }

  const dispose = () => {
    if (typeof window !== 'undefined' && typeof window.removeEventListener === 'function') {
      window.removeEventListener('popstate', onPopState);
    }
    currentPath.dispose();
    currentParams.dispose();
  };
  onCleanup(dispose);

  // Initialize initial params
  currentParams.set(match(initial).params);

  return {
    path: currentPath,
    params: currentParams,
    navigate,
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

  effect(() => {
    const pathname = props.router.path();
    clearRoute();

    const { route, params } = props.router.match(pathname);
    createRoot((dispose) => {
      disposeRouteScope = dispose;
      runWithOwnerScope([[RouterContext.id, props.router]], () => {
        const rendered = untrack(() => {
          if (route) {
            return route.component(params);
          }
          return props.fallback ? props.fallback() : document.createTextNode('404 Not Found');
        });

        const temp = document.createDocumentFragment();
        insertChild(temp, rendered);
        const list = Array.from(temp.childNodes);
        if (anchor.parentNode) {
          for (const n of list) {
            anchor.parentNode.insertBefore(n, anchor);
          }
        } else {
          for (const n of list) {
            frag.insertBefore(n, anchor);
          }
        }
        currentNodes = list;
      });
    });
  });

  return frag;
}

export function Link(props: {
  to: string;
  className?: string;
  children?: ChildNodeValue;
}): Node {
  const router = useContext(RouterContext);
  const safeHref = /^(javascript:|vbscript:|data:text\/html)/i.test(
    props.to.replace(/[\u0000-\u0020]+/g, '')
  )
    ? '#'
    : props.to;
  return h(
    'a',
    {
      href: safeHref,
      className: props.className,
      onClick: (e: MouseEvent) => {
        e.preventDefault();
        if (router && safeHref !== '#') {
          router.navigate(safeHref);
        }
      },
    },
    props.children
  );
}
