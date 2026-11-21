/**
 * View lifetimes, document adoption routing, DOM mutation telemetry, and the
 * global delegated event engine for Svet's DOM layer.
 */

import {
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  lookupOwnerContext,
  onCleanup,
  type ReadonlySignal,
  runUntrackedInCore,
  runWithCapturedOwner,
} from './reactivity.ts';
import { getWasmCore } from './wasm-bridge.ts';

export type ReactiveValue<T> = T | ReadonlySignal<T> | (() => T);

export type ChildNodeValue =
  | Node
  | string
  | number
  | boolean
  | null
  | undefined
  | ReadonlySignal<unknown>
  | (() => unknown)
  | ChildNodeValue[];

export type Component<P = Record<string, unknown>> = (props: P) => Node;

/** Async function components require Node renderToStringAsync. */
export type AsyncComponent<P = Record<string, unknown>> = (props: P) => PromiseLike<Node>;

/** Strips ASCII control chars and spaces (`<= 0x20`) without triggering control-regex rules. */
export function stripControlAndSpaces(value: string): string {
  let clean = '';
  for (let i = 0; i < value.length; i++) {
    if (value.charCodeAt(i) > 0x20) clean += value[i]!;
  }
  return clean;
}

export function isUnsafeUrl(val: string): boolean {
  const clean = stripControlAndSpaces(val).toLowerCase();
  return (
    clean.startsWith('javascript:') ||
    clean.startsWith('vbscript:') ||
    clean.startsWith('data:text/html')
  );
}

/** Shared counter tracking concrete DOM mutations performed by the engine. */
export const domMutationState = { count: 0 };

export function getDomMutationCount(): number {
  return domMutationState.count;
}

export function resetDomMutationCount(): void {
  domMutationState.count = 0;
}

export function isReactiveAccessor(val: unknown): val is () => unknown {
  return typeof val === 'function';
}

export function isDomNode(value: unknown): value is Node {
  if (typeof Node === 'undefined' || value === null || typeof value !== 'object') return false;
  if (value instanceof Node) return true;
  // Native brand checking works across realms, including nodes adopted into a
  // document whose constructor no longer matches their original prototype.
  try {
    Node.prototype.hasChildNodes.call(value);
    return true;
  } catch {
    return false;
  }
}

/** @internal Captures a view owner and graph generation before user rendering. */
export function captureViewLifetime(): () => void {
  const owner = getCurrentOwner(),
    core = getWasmCore(),
    generation = getReactiveGeneration(core);
  return () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('View owner disposed during rendering');
    if (generation !== getReactiveGeneration(core))
      throw new Error('Reactive graph reset during view rendering');
  };
}

// ============================================================================
// Global Event Delegation Engine (1 Document Listener instead of N Node Listeners)
// ============================================================================

const DELEGATED_EVENTS = new Set([
  'click',
  'dblclick',
  'input',
  'change',
  'keydown',
  'keyup',
  'mousedown',
  'mouseup',
  'pointerdown',
  'pointerup',
]);

export type DelegationArea = HTMLElement | ShadowRoot;
export const delegationAreaKey = Symbol('svet delegation area');

type DelegatedBinding = {
  handler: (this: HTMLElement, event: Event) => void;
  active: boolean;
  root?: DelegationArea | undefined;
};

export const EVENT_PROP_PREFIX = '__svet_evt_';
// A versioned target-owned registry coordinates independent copies and realms.
// The native dispatcher invokes each binding's own lifetime/core-aware callback.
const delegationRegistryKey = Symbol.for('svet.event-delegation.registry.v1');
type DelegationRegistry = Map<string, { count: number; listener: EventListener }>;
type DelegationTarget = (Document | DelegationArea) & {
  [delegationRegistryKey]?: DelegationRegistry;
};

function delegationRegistry(target: Document | DelegationArea): DelegationRegistry {
  const host = target as DelegationTarget;
  let registry = host[delegationRegistryKey];
  if (!registry) {
    registry = new Map();
    Object.defineProperty(host, delegationRegistryKey, { value: registry, configurable: true });
  }
  return registry;
}

function retireDelegationRegistry(
  target: Document | DelegationArea,
  registry: DelegationRegistry,
): void {
  const host = target as DelegationTarget;
  if (!registry.size && host[delegationRegistryKey] === registry)
    delete host[delegationRegistryKey];
}

export type AdoptionRoute = { prepare(doc: Document, parent: Node): () => void; settle(): void };
export const adoptionRoutes = new WeakMap<Node, Set<AdoptionRoute>>();

/** @internal Stage document-dependent bindings before native adoption, then reconcile actual placement. */
export function withDocumentAdoption(
  parent: Node,
  child: Node,
  write: () => void,
  assertCurrent: () => void,
): void {
  const doc = parent.nodeType === 9 ? (parent as Document) : parent.ownerDocument;
  if (!doc || child.ownerDocument === doc) {
    write();
    return;
  }
  const routes: AdoptionRoute[] = [],
    pending = [child];
  while (pending.length) {
    assertCurrent();
    const node = pending.pop()!;
    const bindings = adoptionRoutes.get(node);
    if (bindings) for (const binding of bindings) routes.push(binding);
    const first = node.firstChild;
    assertCurrent();
    for (let descendant = first; descendant; descendant = descendant.nextSibling)
      pending.push(descendant);
  }
  const staged: Array<() => void> = [];
  let failed = false;
  let cleanupFailed = false,
    cleanupFailure: unknown;
  try {
    for (const route of routes) {
      assertCurrent();
      staged.push(route.prepare(doc, parent));
      assertCurrent();
    }
    assertCurrent();
    write();
    assertCurrent();
  } catch (failure) {
    failed = true;
    throw failure;
  } finally {
    for (const finish of staged) {
      try {
        finish();
      } catch (failure) {
        if (!cleanupFailed) {
          cleanupFailed = true;
          cleanupFailure = failure;
        }
      }
    }
  }
  if (!failed && cleanupFailed) throw cleanupFailure;
}

function createDelegatedEventProxy(
  event: Event,
  node: HTMLElement,
  path: EventTarget[],
  markImmediateStopped: () => void,
): Event {
  // Delegation must expose the logical bound element, while native Event
  // methods still run with the original Event as their receiver.
  return new Proxy(event, {
    get(original, key) {
      if (key === 'currentTarget') return original.currentTarget ? node : null;
      if (key === 'target' && typeof node.getRootNode === 'function') {
        const root = node.getRootNode();
        return (
          path.find(
            candidate =>
              typeof (candidate as Node).getRootNode === 'function' &&
              (candidate as Node).getRootNode() === root,
          ) ?? original.target
        );
      }
      if (key === 'stopImmediatePropagation')
        return () => {
          markImmediateStopped();
          original.stopImmediatePropagation();
        };
      const value = Reflect.get(original, key, original);
      return typeof value === 'function' ? value.bind(original) : value;
    },
    set(original, key, value) {
      return Reflect.set(original, key, value, original);
    },
  });
}

function dispatchDelegatedEvent(
  doc: Document | DelegationArea,
  entry: { count: number; listener: EventListener },
  eventName: string,
  event: Event,
): void {
  if (entry.count <= 0) return;
  const path = event.composedPath();
  const boundary = path.indexOf(doc);
  for (const target of path.slice(0, boundary < 0 ? path.length : boundary + 1)) {
    if (entry.count <= 0) return;
    const node = target as HTMLElement & { disabled?: boolean } & Record<string, unknown>;
    const bindings = node[EVENT_PROP_PREFIX + eventName] as Set<DelegatedBinding> | undefined;
    if (bindings && !node.disabled) {
      let immediateStopped = false;
      const localEvent = createDelegatedEventProxy(event, node, path, () => {
        immediateStopped = true;
      });
      for (const binding of Array.from(bindings)) {
        if (entry.count <= 0) return;
        if (!binding.active) continue;
        const nodeRoot = typeof node.getRootNode === 'function' ? node.getRootNode() : null;
        if (doc.nodeType !== 9) {
          if (binding.root !== doc || nodeRoot !== doc.getRootNode()) continue;
        } else if (
          binding.root &&
          path.includes(binding.root) &&
          nodeRoot === binding.root.getRootNode()
        ) {
          // Its local area already handled this binding. This is based on the
          // current event route, so redispatching the same Event still works.
          continue;
        }
        binding.handler.call(node, localEvent);
        if (immediateStopped) break;
      }
    }
    if (event.cancelBubble) break;
  }
}

function retainDelegatedListener(doc: Document | DelegationArea, eventName: string): () => void {
  const events = delegationRegistry(doc);
  let entry = events.get(eventName);
  if (!entry) {
    const listener: EventListener = event => {
      if (entry) dispatchDelegatedEvent(doc, entry, eventName, event);
    };
    entry = { count: 1, listener };
    events.set(eventName, entry);
    try {
      doc.addEventListener(eventName, listener);
    } catch (failure) {
      entry.count = 0;
      if (events.get(eventName) === entry) events.delete(eventName);
      retireDelegationRegistry(doc, events);
      try {
        doc.removeEventListener(eventName, listener);
      } catch {
        /* Preserve registration failure. */
      }
      throw failure;
    }
  } else entry.count++;
  const retained = entry;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    if (retained.count <= 0) return;
    if (--retained.count === 0) {
      // Retire registry identity before native removal can reenter registration.
      if (events.get(eventName) === retained) events.delete(eventName);
      retireDelegationRegistry(doc, events);
      doc.removeEventListener(eventName, retained.listener);
    }
  };
}

function removeDisposableFromOwner(
  owner: ReturnType<typeof getCurrentOwner>,
  cleanup: () => void,
): void {
  if (owner && !owner.disposed) {
    const index = owner.disposables.indexOf(cleanup);
    if (index >= 0) owner.disposables.splice(index, 1);
  }
}

function attachDelegatedEvent(
  el: HTMLElement,
  eventName: string,
  assertCurrent: () => void,
  owner: ReturnType<typeof getCurrentOwner>,
  isCurrent: () => boolean,
  invoke: (target: HTMLElement, event: Event) => unknown,
): void {
  const area = lookupOwnerContext<DelegationArea | undefined>(delegationAreaKey, undefined);
  const actualRoot = el.getRootNode();
  const localRoot =
    actualRoot.nodeType === 11 && 'host' in actualRoot && (!area || !area.contains(el))
      ? (actualRoot as ShadowRoot)
      : area;
  const propKey = EVENT_PROP_PREFIX + eventName;
  const nodeStore = el as unknown as Record<string, Set<DelegatedBinding> | undefined>;
  const bindings: Set<DelegatedBinding> = nodeStore[propKey] ?? new Set();
  nodeStore[propKey] = bindings;
  const binding: DelegatedBinding = {
    handler(this: HTMLElement, event) {
      if (!binding.active || !isCurrent()) {
        cleanup();
        return;
      }
      return invoke(this, event);
    },
    active: true,
    root: localRoot,
  };
  bindings.add(binding);
  const documentLeases = new Map<Document, () => void>();
  const stagedDocuments = new Map<Document, number>();
  let releaseLocal: (() => void) | undefined;
  const routes = adoptionRoutes.get(el) ?? new Set<AdoptionRoute>();
  const retainDocument = (doc: Document) => {
    assertCurrent();
    if (documentLeases.has(doc)) return;
    const release = retainDelegatedListener(doc, eventName);
    if (!binding.active || !isCurrent()) {
      release();
      assertCurrent();
      throw new Error('Delegated binding disposed');
    }
    documentLeases.set(doc, release);
    assertCurrent();
  };
  const route: AdoptionRoute = {
    prepare(doc) {
      stagedDocuments.set(doc, (stagedDocuments.get(doc) ?? 0) + 1);
      const unpin = () => {
        const count = stagedDocuments.get(doc) ?? 0;
        if (count <= 1) stagedDocuments.delete(doc);
        else stagedDocuments.set(doc, count - 1);
      };
      try {
        retainDocument(doc);
      } catch (failure) {
        unpin();
        try {
          route.settle();
        } catch {
          /* Preserve registration failure. */
        }
        throw failure;
      }
      let finished = false;
      return () => {
        if (finished) return;
        finished = true;
        unpin();
        route.settle();
      };
    },
    settle() {
      if (!binding.active || !isCurrent()) {
        cleanup();
        return;
      }
      const doc = el.ownerDocument;
      retainDocument(doc);
      for (const [other, release] of documentLeases) {
        if (other !== doc && !stagedDocuments.has(other)) {
          documentLeases.delete(other);
          release();
        }
      }
    },
  };
  routes.add(route);
  adoptionRoutes.set(el, routes);
  const cleanup = () => {
    binding.active = false;
    bindings.delete(binding);
    if (bindings.size === 0 && nodeStore[propKey] === bindings) delete nodeStore[propKey];
    try {
      releaseLocal?.();
    } catch {
      /* Complete document cleanup too. */
    }
    routes.delete(route);
    if (!routes.size && adoptionRoutes.get(el) === routes) adoptionRoutes.delete(el);
    const leases = Array.from(documentLeases.values());
    documentLeases.clear();
    stagedDocuments.clear();
    for (const release of leases) {
      try {
        release();
      } catch {
        /* Inactive bindings cannot publish. */
      }
    }
    removeDisposableFromOwner(owner, cleanup);
  };
  onCleanup(cleanup);
  try {
    retainDocument(el.ownerDocument);
    assertCurrent();
    if (localRoot) releaseLocal = retainDelegatedListener(localRoot, eventName);
    assertCurrent();
    if (!binding.active) throw new Error('Delegated event owner disposed during registration');
  } catch (failure) {
    cleanup();
    throw failure;
  }
}

function attachDirectEvent(
  el: HTMLElement,
  eventName: string,
  assertCurrent: () => void,
  owner: ReturnType<typeof getCurrentOwner>,
  isCurrent: () => boolean,
  invoke: (target: HTMLElement, event: Event) => unknown,
): void {
  let active = true;
  const handler = function (this: HTMLElement, event: Event) {
    if (!active || !isCurrent()) {
      cleanup();
      return;
    }
    return invoke(this, event);
  };
  const cleanup = () => {
    active = false;
    // Repeat removal after registration reentry attaches a late listener.
    try {
      el.removeEventListener(eventName, handler);
    } catch {
      /* Inactive callbacks cannot publish. */
    }
    removeDisposableFromOwner(owner, cleanup);
  };
  onCleanup(cleanup);
  try {
    el.addEventListener(eventName, handler);
    assertCurrent();
    if (!active) throw new Error('Event owner disposed during registration');
  } catch (failure) {
    cleanup();
    throw failure;
  }
}

/** Binds a delegated or direct DOM event listener respecting view lifetime and realm ownership. */
export function bindEventProp(
  el: HTMLElement,
  eventName: string,
  value: (this: HTMLElement, event: Event) => unknown,
): void {
  const assertCurrent = captureViewLifetime(),
    owner = getCurrentOwner(),
    core = getWasmCore();
  const generation = getReactiveGeneration(core);
  const isCurrent = () =>
    !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);
  const invoke = (target: HTMLElement, event: Event) =>
    runUntrackedInCore(core, () =>
      runWithCapturedOwner(core, owner, () => value.call(target, event)),
    );
  assertCurrent();
  if (DELEGATED_EVENTS.has(eventName)) {
    attachDelegatedEvent(el, eventName, assertCurrent, owner, isCurrent, invoke);
  } else {
    attachDirectEvent(el, eventName, assertCurrent, owner, isCurrent, invoke);
  }
}
