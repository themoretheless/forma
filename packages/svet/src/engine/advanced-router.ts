/**
 * Signal-backed reactive client/SSR router (`createRouter`, `useRouter`, `RouterView`, `Link`).
 */

import { retirableCallback, type SvetContext, useContext } from './advanced-context.ts';
import {
  captureViewLifetime,
  type ChildNodeValue,
  h,
  isUnsafeUrl,
  mountOwnedViewNodes,
} from './dom.ts';
import {
  batch,
  createRoot,
  effect,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  type ReadonlySignal,
  runWithOwnerScope,
  signal,
  untrack,
} from './reactivity.ts';
import { getWasmCore, withWasmCore } from './wasm-bridge.ts';

export interface RouteDefinition {
  path: string;
  component: (params: Record<string, string>) => Node;
}

export type NavigationGuard = (
  to: { path: string; params: Record<string, string>; query: Record<string, string>; hash: string },
  from: {
    path: string;
    params: Record<string, string>;
    query: Record<string, string>;
    hash: string;
  },
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

const RouterContext: Pick<SvetContext<RouterInstance | null>, 'id' | 'defaultValue'> = {
  id: Symbol.for('svet.context.router.v1'),
  defaultValue: null,
};

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
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

function matchRoutePattern(pattern: string, pathname: string): Record<string, string> | null {
  if (pattern === '*') return {};
  // split always yields at least one element.
  const cleanPath = pathname.split('#')[0]!.split('?')[0]!;
  const patternParts = pattern.split('/').filter(Boolean);
  const pathParts = cleanPath.split('/').filter(Boolean);

  if (patternParts.length !== pathParts.length) {
    return null;
  }

  const params: Record<string, string> = {};
  for (let i = 0; i < patternParts.length; i++) {
    // Both lists have the same length, checked above.
    const p = patternParts[i]!;
    const actual = decodeRouteSegment(pathParts[i]!);
    if (p.startsWith(':')) {
      Object.defineProperty(params, p.slice(1), {
        value: actual,
        enumerable: true,
        writable: true,
        configurable: true,
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
type RouterHistoryPosition = { chain: string; index: number };

function readRouterHistoryPosition(state: unknown): RouterHistoryPosition | null {
  const value =
    state && typeof state === 'object'
      ? (state as Record<string, unknown>)[routerHistoryKey]
      : null;
  if (!value || typeof value !== 'object') return null;
  const { chain, index } = value as RouterHistoryPosition;
  return typeof chain === 'string' && Number.isSafeInteger(index) && index >= 0
    ? { chain, index }
    : null;
}

function stampRouterHistory(
  state: unknown,
  position: RouterHistoryPosition,
): Record<string, unknown> {
  // Preserve host object fields. Non-record state is retained in an envelope.
  const base =
    state !== null &&
    typeof state === 'object' &&
    !Array.isArray(state) &&
    Object.getPrototypeOf(state) === Object.prototype
      ? state
      : { __svet_router_state__: state };
  return { ...base, [routerHistoryKey]: position };
}

function routerMatcher(options: { routes: RouteDefinition[] }) {
  let liveOptions: { routes: RouteDefinition[] } | null = options;
  let routes: RouteDefinition[] = [];
  return {
    match: (rawUrl: string): ReturnType<RouterInstance['match']> => {
      // Live configuration can replace the array. Retirement keeps the last
      // observed table without retaining the whole options/guard environment.
      if (liveOptions) routes = liveOptions.routes;
      const { pathname, query, hash } = parseUrlParts(rawUrl);
      for (const route of routes) {
        const params = matchRoutePattern(route.path, pathname);
        if (params !== null) return { route, params, pathname, query, hash };
      }
      return { route: null, params: {}, pathname, query, hash };
    },
    retire: () => {
      liveOptions = null;
    },
  };
}

function retirableRouterSignal<T>(source: ReadonlySignal<T>) {
  let current: ReadonlySignal<T> | null = source;
  let value: T;
  const read = (() => (current ? current() : value)) as ReadonlySignal<T>;
  Object.defineProperties(read, { id: { value: source.id }, __svet_sig: { value: true } });
  read.peek = () => (current ? current.peek() : value);
  const retire = () => {
    if (current) {
      value = current.peek();
      current = null;
    }
  };
  read.dispose = () => {
    if (current) {
      current.dispose();
      retire();
    }
  };
  return { read, retire };
}

function retirableRouter(implementation: RouterInstance) {
  let current: RouterInstance | null = implementation;
  const path = retirableRouterSignal(implementation.path);
  const params = retirableRouterSignal(implementation.params);
  const query = retirableRouterSignal(implementation.query);
  const hash = retirableRouterSignal(implementation.hash);
  const removers = new Set<() => void>();
  const router: RouterInstance = {
    path: path.read,
    params: params.read,
    query: query.read,
    hash: hash.read,
    navigate: to => current?.navigate(to) ?? false,
    beforeEach: guard => {
      if (!current) return () => {};
      const remover = retirableCallback(current.beforeEach(guard));
      removers.add(remover.dispose);
      return () => {
        try {
          remover.invoke();
        } finally {
          remover.dispose();
          removers.delete(remover.dispose);
        }
      };
    },
    match: implementation.match,
    dispose: () => {
      current?.dispose();
    },
  };
  return {
    router,
    retire: () => {
      path.retire();
      params.retire();
      query.retire();
      hash.retire();
      current = null;
      for (const retire of removers) retire();
      removers.clear();
    },
  };
}

function createUrlNormalizer(browserWindow: (Window & typeof globalThis) | undefined) {
  const browserOrigin = browserWindow?.location.origin;
  const resolutionBase =
    browserOrigin && browserOrigin !== 'null' ? `${browserOrigin}/` : 'http://svet.invalid/';
  const origin = new URL(resolutionBase).origin;
  return (to: string, base: string): string | null => {
    if (typeof to !== 'string') return null;
    try {
      const destination = new URL(to, new URL(base, resolutionBase));
      if (!/^https?:$/.test(destination.protocol) || destination.origin !== origin) return null;
      return `${destination.pathname}${destination.search}${destination.hash}`;
    } catch {
      return null;
    }
  };
}

function createRouterHistoryController(
  browserWindow: (Window & typeof globalThis) | undefined,
  initialFull: string,
  isCurrent: () => boolean,
  syncPath: (to: string, pushHistory: boolean) => boolean,
) {
  let currentFull = initialFull;
  const history = browserWindow?.history;
  const historyChains = new Set<string>();
  let physicalPosition: RouterHistoryPosition | null = null;
  let acceptedPosition: RouterHistoryPosition | null = null;
  let acceptedCommit = {};
  let restoring = false;
  let restoreTimer: ReturnType<typeof setTimeout> | undefined;
  const clearRestoreTimer = () => {
    if (restoreTimer !== undefined) {
      clearTimeout(restoreTimer);
      restoreTimer = undefined;
    }
  };
  const fullLocation = () =>
    browserWindow
      ? `${browserWindow.location.pathname}${browserWindow.location.search}${browserWindow.location.hash}`
      : currentFull;
  const ownPosition = (state: unknown): RouterHistoryPosition | null => {
    const position = readRouterHistoryPosition(state);
    return position && historyChains.has(position.chain) ? position : null;
  };
  const stampPhysicalEntry = (): RouterHistoryPosition => {
    const chain = `svet:${Date.now()}:${++nextHistoryChain}`;
    historyChains.add(chain);
    const position = { chain, index: 0 };
    history!.replaceState(stampRouterHistory(history!.state, position), '', fullLocation());
    physicalPosition = position;
    return position;
  };
  const restoreAcceptedEntry = () => {
    clearRestoreTimer();
    if (!isCurrent() || !history) return;
    const target = acceptedPosition;
    if (
      target &&
      physicalPosition &&
      target.chain === physicalPosition.chain &&
      target.index !== physicalPosition.index
    ) {
      restoring = true;
      try {
        history.go(target.index - physicalPosition.index);
      } catch (failure) {
        restoring = false;
        throw failure;
      }
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
    if (
      restoring &&
      acceptedPosition &&
      physicalPosition &&
      acceptedPosition.chain === physicalPosition.chain &&
      acceptedPosition.index === physicalPosition.index &&
      fullLocation() === currentFull
    ) {
      clearRestoreTimer();
      restoring = false;
      return; // Compensation is not another guarded navigation.
    }
    if (restoring) {
      // Coalesce burst popstate events arriving during compensation into one retry
      // so rapid multi-step navigation bursts do not multiply History IPC calls.
      clearRestoreTimer();
      restoreTimer = setTimeout(() => {
        restoreTimer = undefined;
        if (restoring) restoreAcceptedEntry();
      }, 0);
      return;
    }
    const previousCommit = acceptedCommit;
    try {
      syncPath(fullLocation(), false);
    } catch (failure) {
      // Native restoration failure must not hide the original guard failure.
      if (acceptedCommit === previousCommit) {
        try {
          restoreAcceptedEntry();
        } catch {
          /* Keep the primary transition failure. */
        }
      }
      throw failure;
    }
    // A nested successful navigation owns its new URL/position. Rejected guards
    // recover only when no newer commit superseded their transition.
    if (acceptedCommit === previousCommit) restoreAcceptedEntry();
  };
  return {
    getCurrentFull: () => currentFull,
    isRestoring: () => restoring,
    syncBrowserUrl(normalized: string, pushHistory: boolean) {
      if (pushHistory && history) {
        physicalPosition = ownPosition(history.state);
        const previous = physicalPosition ?? stampPhysicalEntry();
        // Retain the initial entry as a rollback destination before the first push.
        if (!acceptedPosition) acceptedPosition = previous;
        const next = { chain: previous.chain, index: previous.index + 1 };
        history.pushState(stampRouterHistory({}, next), '', normalized);
        physicalPosition = next;
      } else if (history) {
        // A guard redirect during traversal belongs to the visited slot, not a new push.
        if (normalized !== fullLocation()) history.replaceState(history.state, '', normalized);
        if (!physicalPosition) stampPhysicalEntry();
      }
    },
    commitAccepted(normalized: string) {
      currentFull = normalized;
      acceptedPosition = physicalPosition;
      acceptedCommit = {};
    },
    attach: () => browserWindow?.addEventListener?.('popstate', onPopState),
    dispose() {
      clearRestoreTimer();
      browserWindow?.removeEventListener?.('popstate', onPopState);
      historyChains.clear();
    },
  };
}

export function createRouter(options: {
  routes: RouteDefinition[];
  initialPath?: string;
  beforeEach?: NavigationGuard;
}): RouterInstance {
  const owner = getCurrentOwner(),
    core = getWasmCore(),
    generation = getReactiveGeneration(core);
  if (isOwnerScopeDisposed(owner)) throw new Error('Router construction owner disposed');
  const browserWindow = typeof window !== 'undefined' ? window : undefined;
  const normalize = createUrlNormalizer(browserWindow);
  const initialGuard = options.beforeEach;
  let disposed = false,
    navigationVersion = 0;
  const isCurrent = () =>
    !disposed && !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);
  const initialBase = browserWindow
    ? `${browserWindow.location.pathname}${browserWindow.location.search ?? ''}${browserWindow.location.hash ?? ''}`
    : '/';
  const initialTarget = normalize(options.initialPath ?? initialBase, initialBase);
  if (initialTarget === null) throw new TypeError('Invalid or external initial router URL');

  const matcher = routerMatcher(options);
  const match = matcher.match;
  const initialMatch = match(initialTarget);
  if (!isCurrent()) throw new Error('Router context or owner invalidated during construction');

  const currentPath = signal(initialMatch.pathname);
  const currentParams = signal<Record<string, string>>(initialMatch.params);
  const currentQuery = signal<Record<string, string>>(initialMatch.query);
  const currentHash = signal<string>(initialMatch.hash);
  const guards = new Set<NavigationGuard>();
  if (initialGuard) guards.add(initialGuard);

  const historyCtrl = createRouterHistoryController(
    browserWindow,
    initialTarget,
    isCurrent,
    (to, push) => syncPathInternal(to, push),
  );

  const syncPathInternal = (to: string, pushHistory: boolean, redirectDepth = 0): boolean => {
    if (!isCurrent() || redirectDepth > 10) return false;
    const baseFull = historyCtrl.getCurrentFull();
    const normalized = normalize(to, baseFull);
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
      if (res === false) return false;
      if (typeof res === 'string') {
        const redirect = normalize(res, baseFull);
        if (redirect === null) return false;
        if (redirect !== normalized)
          return syncPathInternal(redirect, pushHistory, redirectDepth + 1);
      }
    }

    historyCtrl.syncBrowserUrl(normalized, pushHistory);
    if (!isCurrent() || attempt !== navigationVersion) return false;
    historyCtrl.commitAccepted(normalized);
    withWasmCore(core, () =>
      batch(() => {
        currentPath.set(nextMatch.pathname);
        currentParams.set(nextMatch.params);
        currentQuery.set(nextMatch.query);
        currentHash.set(nextMatch.hash);
      }),
    );
    return true;
  };

  historyCtrl.attach();
  const retirement: { retire?: () => void } = {};
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    navigationVersion++;
    if (owner && !owner.disposed) {
      const index = owner.disposables.indexOf(dispose);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
    historyCtrl.dispose();
    guards.clear();
    currentPath.dispose();
    currentParams.dispose();
    currentQuery.dispose();
    currentHash.dispose();
    matcher.retire();
    retirement.retire?.();
  };
  onCleanup(dispose);

  const handle = retirableRouter({
    path: currentPath,
    params: currentParams,
    query: currentQuery,
    hash: currentHash,
    navigate: to => (historyCtrl.isRestoring() ? false : syncPathInternal(to, true)),
    beforeEach: guard => {
      if (!isCurrent()) return () => {};
      guards.add(guard);
      return () => guards.delete(guard);
    },
    match,
    dispose,
  });
  retirement.retire = handle.retire;
  return handle.router;
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
      createRoot(dispose => {
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
  const unsafe = isUnsafeUrl(to);
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
        if (unsafe) {
          e.preventDefault();
          return;
        }
        if (!router || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return;
        const anchor = e.currentTarget as HTMLAnchorElement | null;
        if (!anchor || anchor.hasAttribute('download')) return;
        const doc = anchor.ownerDocument;
        const target =
          anchor.getAttribute('target') ||
          doc.querySelector('base[target]')?.getAttribute('target') ||
          '';
        if (target && target.toLowerCase() !== '_self') return;
        const location = doc.defaultView?.location;
        if (!location) return;
        let destination: URL;
        try {
          destination = new URL(anchor.href, location.href);
        } catch {
          return;
        }
        if (!/^https?:$/.test(destination.protocol) || destination.origin !== location.origin)
          return;
        e.preventDefault();
        router.navigate(`${destination.pathname}${destination.search}${destination.hash}`);
      },
    },
    props.children,
  );
}
