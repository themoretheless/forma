/**
 * What a Forma component instance is from the outside, and the lifetime rules around one: who
 * disposes the instances a factory creates, what a handle keeps after its instance is closed.
 */

import type { FormaDocumentAst } from './forma-ast.ts';
import { structuredCloneIfObject } from './forma-reactive.ts';
import {
  getCurrentOwner,
  isOwnerScopeDisposed,
  onCleanup,
  type ReadonlySignal,
  type Signal,
} from './reactivity.ts';

export interface FormaStateMutator {
  toggle(field: string): void;
  set(field: string, value: any): void;
  increment(field: string, delta?: number): void;
  decrement(field: string, delta?: number): void;
  push(field: string, item: any): void;
  remove(field: string, index: number): void;
  reset(field?: string): void;
}

export interface FormaHmrSnapshot {
  componentName: string;
  locale: string;
  state: Record<string, any>;
  props: Record<string, any>;
}

export interface FormaRuntimeInstance {
  readonly componentName: string;
  readonly state: Record<string, Signal<any>>;
  readonly props: Record<string, Signal<any>>;
  readonly computedLets: Record<string, ReadonlySignal<any>>;
  readonly locale: Signal<string>;
  readonly isValid: ReadonlySignal<boolean>;
  readonly actions: FormaStateMutator;
  /** Atomically updates multiple state fields inside a single WASM batch (creates signals for undeclared fields) */
  setState(partial: Record<string, any>): void;
  onEvent(eventName: string, handler: (...args: any[]) => void): () => void;
  emitEvent(eventName: string, ...args: any[]): void;
  getElementByKey(key: string): HTMLElement | undefined;
  /** Captures a serializable HMR snapshot of all signals (`state`, `props`, `locale`) */
  exportHmrSnapshot(): FormaHmrSnapshot;
  /** Restores signal values atomically from an HMR snapshot without losing user state */
  restoreHmrSnapshot(snapshot: FormaHmrSnapshot): void;
  /** Hot-reloads the `.ui` source code or pre-parsed `FormaDocumentAst` in-place, preserving all signal values, ViewModel bindings, and event listeners */
  hotReload(nextUiSourceOrAst: string | FormaDocumentAst): void;
  /** Renders the Forma component into a live DOM DocumentFragment for embedding inside Svet JSX (`<FormaHost instance={...} />`) */
  render(): DocumentFragment;
  mount(container: HTMLElement): { unmount(): void };
  /** Disposes all internal state/prop/computed signals owned by this Forma component instance */
  dispose(): void;
}

// Synchronous factory construction owns every completed Forma instance it creates,
// including instances whose handle is lost when the factory throws.
type FormaConstructionDomain = { active: Set<FormaRuntimeInstance> | null };
const formaConstructionHost = globalThis as unknown as Record<
  symbol,
  FormaConstructionDomain | undefined
>;
export const formaConstructionDomain = (formaConstructionHost[
  Symbol.for('svet.forma.construction.v1')
] ??= { active: null });
export function constructFormaChild(factory: () => FormaRuntimeInstance): FormaRuntimeInstance {
  const previous = formaConstructionDomain.active;
  const created = new Set<FormaRuntimeInstance>();
  const owner = getCurrentOwner();
  formaConstructionDomain.active = created;
  const release = () => {
    const instances = [...created].reverse();
    created.clear();
    for (const instance of instances) {
      try {
        instance.dispose();
      } catch {
        /* Drain siblings after a cleanup failure. */
      }
    }
  };
  try {
    const child = factory();
    // The renderer transfers the returned handle separately; avoid duplicate disposal.
    created.delete(child);
    if (created.size) {
      if (isOwnerScopeDisposed(owner)) release();
      else onCleanup(release);
    }
    return child;
  } catch (error) {
    release();
    throw error;
  } finally {
    formaConstructionDomain.active = previous;
  }
}

/** A retained terminal public handle must not retain the compiler's native context. */
export function createFormaPublicHandle(
  implementation: FormaRuntimeInstance,
): FormaRuntimeInstance {
  const cell: {
    current: FormaRuntimeInstance | null;
    closing: boolean;
    closed: any;
    leases: Set<{ stop: (() => void) | null }>;
  } = {
    current: implementation,
    closing: false,
    closed: null,
    leases: new Set(),
  };
  const componentName = implementation.componentName;
  implementation = null!;
  const handle = {} as FormaRuntimeInstance;
  const live = () => {
    if (!cell.current || cell.closing) throw new Error('Forma instance is disposed');
    return cell.current;
  };
  const actions = {} as FormaStateMutator;
  const actionSlots: Array<{ original: Function | null; override: Function | null }> = [];
  for (const key of [
    'toggle',
    'set',
    'increment',
    'decrement',
    'push',
    'remove',
    'reset',
  ] as const) {
    const slot = {
      original: cell.current!.actions[key] as Function | null,
      override: null as Function | null,
    };
    actionSlots.push(slot);
    const original = (...args: any[]) => {
      if (!cell.current || cell.closing) return;
      return slot.original?.apply(cell.current.actions, args);
    };
    Object.defineProperty(actions, key, {
      enumerable: true,
      get: () => slot.override ?? original,
      set: (replacement: Function) => {
        if (!cell.current || cell.closing) return;
        slot.override = replacement === original ? null : replacement;
        (cell.current.actions as any)[key] = slot.override ?? slot.original;
      },
    });
  }
  Object.defineProperty(handle, 'componentName', { enumerable: true, value: componentName });
  Object.defineProperty(handle, 'actions', { enumerable: true, value: actions });
  for (const key of ['state', 'props', 'computedLets', 'locale', 'isValid'] as const) {
    Object.defineProperty(handle, key, {
      enumerable: true,
      get: () => (cell.current ? cell.current[key] : cell.closed[key]),
    });
  }
  for (const key of [
    'setState',
    'emitEvent',
    'restoreHmrSnapshot',
    'hotReload',
    'render',
  ] as const) {
    (handle as any)[key] = (...args: any[]) => (live()[key] as Function).apply(handle, args);
  }
  handle.getElementByKey = key => cell.current?.getElementByKey(key);
  handle.exportHmrSnapshot = () => {
    if (cell.current) return cell.current.exportHmrSnapshot();
    return {
      componentName,
      locale: cell.closed.locale.peek(),
      state: Object.fromEntries(
        Object.entries(cell.closed.state).map(([key, value]) => [
          key,
          structuredCloneIfObject((value as Signal<any>).peek()),
        ]),
      ),
      props: Object.fromEntries(
        Object.entries(cell.closed.props).map(([key, value]) => [
          key,
          structuredCloneIfObject((value as Signal<any>).peek()),
        ]),
      ),
    };
  };
  handle.mount = container => {
    const mounted = live().mount.call(handle, container);
    return { unmount: leaseFormaCleanup(mounted.unmount, cell.leases) };
  };
  handle.onEvent = (name, callback) =>
    leaseFormaCleanup(live().onEvent(name, callback), cell.leases);
  handle.dispose = () => {
    if (!cell.current || cell.closing) return;
    cell.closing = true;
    const current = cell.current;
    let failed = false,
      failure: unknown;
    try {
      current.dispose.call(handle);
    } catch (error) {
      failed = true;
      failure = error;
    }
    try {
      cell.closed = snapshotClosedForma(current);
    } catch (error) {
      cell.closed = {
        state: Object.create(null),
        props: Object.create(null),
        computedLets: Object.create(null),
        locale: cachedFormaValue(''),
        isValid: cachedFormaValue(undefined),
      };
      if (!failed) {
        failed = true;
        failure = error;
      }
    } finally {
      cell.current = null;
      cell.closing = false;
      for (const slot of actionSlots) {
        slot.original = null;
        slot.override = null;
      }
      for (const lease of cell.leases) lease.stop = null;
      cell.leases.clear();
    }
    if (failed) throw failure;
  };
  return handle;
}
function leaseFormaCleanup(
  stop: () => void,
  leases: Set<{ stop: (() => void) | null }>,
): () => void {
  const lease = { stop: stop as (() => void) | null };
  leases.add(lease);
  stop = null!;
  return () => {
    const current = lease.stop;
    if (!current) return;
    lease.stop = null;
    leases.delete(lease);
    current();
  };
}
export function cachedFormaValue(value: any): Signal<any> {
  const getter = (() => value) as Signal<any>;
  Object.assign(getter, {
    id: -1,
    __svet_sig: true,
    peek: () => value,
    set: () => {},
    update: () => {},
    dispose: () => {},
  });
  return getter;
}
function snapshotClosedForma(instance: FormaRuntimeInstance) {
  const copies = new WeakMap<object, Signal<any>>();
  const copy = (resource: any, allowSignalPeek: boolean) => {
    let result = copies.get(resource);
    if (!result) {
      // Signal.peek is a cached read. Computed.peek can execute user code and
      // must never be used as a fallback while closing an instance.
      const value =
        typeof resource.__svet_cached_value === 'function'
          ? resource.__svet_cached_value()
          : allowSignalPeek
            ? resource.peek()
            : undefined;
      result = cachedFormaValue(value);
      copies.set(resource, result);
    }
    return result;
  };
  const map = (values: Record<string, any>, allowSignalPeek: boolean) =>
    Object.fromEntries(
      Object.entries(values).map(([key, value]) => [key, copy(value, allowSignalPeek)]),
    );
  return {
    state: map(instance.state, true),
    props: map(instance.props, true),
    computedLets: map(instance.computedLets, false),
    locale: copy(instance.locale, true),
    isValid: copy(instance.isValid, false),
  };
}
