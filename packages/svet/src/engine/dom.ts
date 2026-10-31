/**
 * # Svet Surgical DOM Engine (`dom.ts`)
 *
 * Includes:
 * 1. **Zero Virtual DOM**: Components run ONCE and return real DOM nodes.
 * 2. **Hierarchical Scope Disposal (`createRoot`)**: Every `<Show>` branch and `<For>` list item
 *    runs inside its own `OwnerScope`. When a branch switches or an item is removed from a list,
 *    all nested `effect` and `computed` nodes inside it are automatically disposed and their WASM
 *    slot IDs are recycled back into `free_list`!
 * 3. **Global Event Delegation**: Common events (`click`, `input`, `change`, `keydown`, `keyup`,
 *    `submit`, `pointerdown`, `pointerup`) use a single root listener on `document`, so 10,000 list
 *    rows attach 1 browser listener instead of 10,000.
 * 4. **First-Class Two-Way Binding (`model={signal}`)**: Automatically binds `<input>`, `<textarea>`,
 *    and `<select>` values (including `type="checkbox"` and `type="number"`) to a Signal in both directions.
 */

import { batch, createRoot, effect, getCurrentOwner, isOwnerScopeDisposed, getReactiveGeneration, lookupOwnerContext, runWithCapturedOwner, runUntrackedInCore, runWithOwnerScope, onCleanup, type ReadonlySignal, signal, type Signal, untrack } from './reactivity.ts';
import { computeLisWasm, getWasmCore, withWasmCore } from './wasm-bridge.ts';

export type ReactiveValue<T> = T | ReadonlySignal<T> | (() => T);

export type ChildNodeValue =
  | Node
  | string
  | number
  | boolean
  | null
  | undefined
  | ReadonlySignal<any>
  | (() => any)
  | ChildNodeValue[];

export type Component<P = Record<string, any>> = (props: P) => Node;

let domMutationCounter = 0;

export function getDomMutationCount(): number {
  return domMutationCounter;
}

export function resetDomMutationCount(): void {
  domMutationCounter = 0;
}

function isReactiveAccessor(val: unknown): val is () => any {
  return typeof val === 'function';
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

type DelegationArea = HTMLElement | ShadowRoot;
const delegationAreaKey = Symbol('svet delegation area');
type DelegatedBinding = { handler: (this: HTMLElement, event: Event) => void; active: boolean; root?: DelegationArea };
const EVENT_PROP_PREFIX = '__svet_evt_';
// A versioned target-owned registry coordinates independent copies and realms.
// The native dispatcher invokes each binding's own lifetime/core-aware callback.
const delegationRegistryKey = Symbol.for('svet.event-delegation.registry.v1');
type DelegationRegistry = Map<string, { count: number; listener: EventListener }>;
type DelegationTarget = (Document | DelegationArea) & { [delegationRegistryKey]?: DelegationRegistry };
function delegationRegistry(target: Document | DelegationArea): DelegationRegistry {
  const host = target as DelegationTarget;
  let registry = host[delegationRegistryKey];
  if (!registry) {
    registry = new Map();
    Object.defineProperty(host, delegationRegistryKey, {value: registry, configurable: true});
  }
  return registry;
}
function retireDelegationRegistry(target: Document | DelegationArea, registry: DelegationRegistry): void {
  const host = target as DelegationTarget;
  if (!registry.size && host[delegationRegistryKey] === registry) delete host[delegationRegistryKey];
}

type AdoptionRoute = { prepare(doc: Document, parent: Node): () => void; settle(): void };
const adoptionRoutes = new WeakMap<Node, Set<AdoptionRoute>>();

/** @internal Stage document-dependent bindings before native adoption, then reconcile actual placement. */
export function withDocumentAdoption(parent: Node, child: Node, write: () => void, assertCurrent: () => void): void {
  const doc = parent.nodeType === 9 ? parent as Document : parent.ownerDocument;
  if (!doc || child.ownerDocument === doc) { write(); return; }
  const routes: AdoptionRoute[] = [], pending = [child];
  while (pending.length) {
    assertCurrent();
    const node = pending.pop()!;
    const bindings = adoptionRoutes.get(node);
    if (bindings) for (const binding of bindings) routes.push(binding);
    const first = node.firstChild;
    assertCurrent();
    for (let descendant = first; descendant; descendant = descendant.nextSibling) pending.push(descendant);
  }
  const staged: Array<() => void> = [];
  let failed = false;
  try {
    for (const route of routes) { assertCurrent(); staged.push(route.prepare(doc, parent)); assertCurrent(); }
    assertCurrent(); write(); assertCurrent();
  } catch (failure) { failed = true; throw failure; }
  finally {
    let cleanupFailed = false, cleanupFailure: unknown;
    for (const finish of staged) {
      try { finish(); }
      catch (failure) { if (!cleanupFailed) { cleanupFailed = true; cleanupFailure = failure; } }
    }
    if (!failed && cleanupFailed) throw cleanupFailure;
  }
}

function retainDelegatedListener(doc: Document | DelegationArea, eventName: string): () => void {
  const events = delegationRegistry(doc);
  let entry = events.get(eventName);
  if (!entry) {
    const listener: EventListener = event => {
      if (!entry || entry.count <= 0) return;
      const path = event.composedPath();
      const boundary = path.indexOf(doc);
      for (const target of path.slice(0, boundary < 0 ? path.length : boundary + 1)) {
        if (entry.count <= 0) return;
        const node = target as HTMLElement & Record<string, any>;
        const bindings = node[EVENT_PROP_PREFIX + eventName] as Set<DelegatedBinding> | undefined;
        if (bindings && !node.disabled) {
          let immediateStopped = false;
          // Delegation must expose the logical bound element, while native Event
          // methods still run with the original Event as their receiver.
          const localEvent = new Proxy(event, {
            get(original, key) {
              if (key === 'currentTarget') return original.currentTarget ? node : null;
              if (key === 'target' && typeof node.getRootNode === 'function') {
                const root = node.getRootNode();
                return path.find(candidate => typeof (candidate as Node).getRootNode === 'function' &&
                  (candidate as Node).getRootNode() === root) ?? original.target;
              }
              if (key === 'stopImmediatePropagation') return () => {
                immediateStopped = true;
                original.stopImmediatePropagation();
              };
              const value = Reflect.get(original, key, original);
              return typeof value === 'function' ? value.bind(original) : value;
            },
            set(original, key, value) { return Reflect.set(original, key, value, original); },
          });
          for (const binding of Array.from(bindings)) {
            if (entry.count <= 0) return;
            if (!binding.active) continue;
            const nodeRoot = typeof node.getRootNode === 'function' ? node.getRootNode() : null;
            if (doc.nodeType !== 9) {
              if (binding.root !== doc || nodeRoot !== doc.getRootNode()) continue;
            } else if (binding.root && path.includes(binding.root) && nodeRoot === binding.root.getRootNode()) {
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
    };
    entry = { count: 1, listener };
    events.set(eventName, entry);
    try { doc.addEventListener(eventName, listener); }
    catch (failure) {
      entry.count = 0;
      if (events.get(eventName) === entry) events.delete(eventName);
      retireDelegationRegistry(doc, events);
      try { doc.removeEventListener(eventName, listener); } catch { /* Preserve registration failure. */ }
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
      if (events!.get(eventName) === retained) events!.delete(eventName);
      retireDelegationRegistry(doc, events);
      doc.removeEventListener(eventName, retained.listener);
    }
  };
}

const modelSynchronizers = new WeakMap<HTMLElement, Set<() => void>>();

type ModelResetBinding = {
  core: ReturnType<typeof getWasmCore>;
  signal: Signal<any>;
  isCurrent: () => boolean;
  stop: () => void;
  read: () => {value: unknown; priority: number} | null;
};
const resetBindings = new WeakMap<Element, Set<ModelResetBinding>>();
const resetRoots = new WeakMap<EventTarget, {count: number; listener: EventListener}>();
const pendingResetEvents = new WeakSet<Event>();
const pendingResetForms = new WeakMap<HTMLFormElement, number>();
function formControls(form: HTMLFormElement): Element[] {
  // Named controls can shadow form.elements just as they can shadow form.reset.
  const prototype = form.ownerDocument.defaultView?.HTMLFormElement.prototype ?? globalThis.HTMLFormElement?.prototype;
  const getter = prototype && Object.getOwnPropertyDescriptor(prototype, 'elements')?.get;
  return Array.from(getter ? getter.call(form) : form.elements);
}
function handleModelReset(event: Event): void {
  const form = event.target as HTMLFormElement | null;
  if (!form || form.nodeType !== 1 || form.tagName !== 'FORM' || pendingResetEvents.has(event)) return;
  pendingResetEvents.add(event);
  pendingResetForms.set(form, (pendingResetForms.get(form) ?? 0) + 1);
  queueMicrotask(() => {
    pendingResetEvents.delete(event);
    const remaining = (pendingResetForms.get(form) ?? 1) - 1;
    if (remaining) pendingResetForms.set(form, remaining);
    else pendingResetForms.delete(form);
    if (event.defaultPrevented) return;
    const groups = new Map<ReturnType<typeof getWasmCore>, Map<Signal<any>, {binding: ModelResetBinding; value: unknown; priority: number}>>();
    // Read all reset values before publishing anything: radio/model effects can
    // otherwise alter the native state of controls not yet visited.
    for (const el of formControls(form)) {
      if ((el as HTMLInputElement).form !== form) continue;
      for (const binding of resetBindings.get(el) ?? []) {
        if (!binding.isCurrent()) {binding.stop();continue;}
        const next = withWasmCore(binding.core, () => untrack(binding.read));
        if (!binding.isCurrent()) {binding.stop();continue;}
        if (!next) continue;
        let group = groups.get(binding.core);
        if (!group) {group = new Map(); groups.set(binding.core, group);}
        const previous = group.get(binding.signal);
        if (!previous || next.priority >= previous.priority) group.set(binding.signal, {binding, ...next});
      }
    }
    for (const [core, group] of groups) withWasmCore(core, () => batch(() => {
      for (const {binding, value} of group.values()) {
        if (binding.isCurrent()) binding.signal.set(value);
        else binding.stop();
      }
    }));
  });
}
function retainResetRoot(root: EventTarget): () => void {
  let entry = resetRoots.get(root);
  if (entry) entry.count++;
  else {
    const listener: EventListener = event => { if (entry && entry.count > 0) handleModelReset(event); };
    entry = {count: 1, listener}; resetRoots.set(root, entry);
    try { root.addEventListener('reset', listener, true); }
    catch (failure) {
      entry.count = 0;
      if (resetRoots.get(root) === entry) resetRoots.delete(root);
      try { root.removeEventListener('reset', listener, true); } catch { /* Preserve registration failure. */ }
      throw failure;
    }
  }
  const retained = entry;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    if (retained.count <= 0) return;
    if (--retained.count === 0) {
      if (resetRoots.get(root) === retained) resetRoots.delete(root);
      try { root.removeEventListener('reset', retained.listener, true); } catch { /* Weak binding lookup still prevents stale publication. */ }
    }
  };
}
function registerModelReset(el: HTMLElement, binding: ModelResetBinding): () => void {
  let entries = resetBindings.get(el);
  if (!entries) {entries = new Set();resetBindings.set(el, entries);}
  entries.add(binding);
  const roots = new Map<EventTarget, () => void>(), stagedRoots = new Map<EventTarget, number>();
  const routes = adoptionRoutes.get(el) ?? new Set<AdoptionRoute>();
  let active = true;
  const assertCurrent = () => {
    if (!active || !binding.isCurrent()) throw new Error('Model binding disposed during document adoption');
  };
  const retain = (root: EventTarget) => {
    assertCurrent();
    if (roots.has(root)) return;
    const stop = retainResetRoot(root);
    if (!active || !binding.isCurrent()) { stop(); assertCurrent(); }
    roots.set(root, stop);
    assertCurrent();
  };
  const release = () => {
    active = false;
    entries!.delete(binding);
    if (!entries!.size && resetBindings.get(el) === entries) resetBindings.delete(el);
    routes.delete(route);
    if (!routes.size && adoptionRoutes.get(el) === routes) adoptionRoutes.delete(el);
    const leases = Array.from(roots.values()); roots.clear(); stagedRoots.clear();
    for (const stop of leases) stop();
  };
  const route: AdoptionRoute = {
    prepare(doc, parent) {
      const targets = new Set<EventTarget>([doc, parent.getRootNode()]);
      const unpin = () => {
        for (const root of targets) {
          const count = stagedRoots.get(root) ?? 0;
          if (count <= 1) stagedRoots.delete(root); else stagedRoots.set(root, count - 1);
        }
      };
      for (const root of targets) stagedRoots.set(root, (stagedRoots.get(root) ?? 0) + 1);
      try { for (const root of targets) retain(root); }
      catch (failure) {
        unpin();
        try { route.settle(); } catch { /* Preserve registration failure. */ }
        throw failure;
      }
      let finished = false;
      return () => { if (finished) return; finished = true; unpin(); route.settle(); };
    },
    settle() {
      if (!active || !binding.isCurrent()) { release(); return; }
      const desired = new Set<EventTarget>([el.ownerDocument]);
      const form = (el as HTMLInputElement).form, root = el.getRootNode();
      if (form) desired.add(form);
      if (root !== el && root.nodeType !== 9) desired.add(root);
      for (const target of desired) retain(target);
      for (const [target, stop] of roots) {
        if (!desired.has(target) && !stagedRoots.has(target)) { roots.delete(target); stop(); }
      }
    },
  };
  routes.add(route); adoptionRoutes.set(el, routes);
  try {
    retain(el.ownerDocument);
    const form = (el as HTMLInputElement).form;
    if (form) retain(form);
    return release;
  } catch (failure) { release(); throw failure; }
}
function retainOwnedResetRoot(root: EventTarget): void {
  const assertCurrent = captureViewLifetime(), owner = getCurrentOwner();
  assertCurrent();
  let release: (() => void) | undefined, stopped = false;
  const stop = () => {
    stopped = true;
    // Registration can synchronously dispose its owner before returning the lease.
    // Repeat release after such reentry; the underlying lease is idempotent.
    release?.();
    if (owner && !owner.disposed) {
      const index = owner.disposables.indexOf(stop);
      if (index >= 0) owner.disposables.splice(index, 1);
    }
  };
  onCleanup(stop);
  try {
    release = retainResetRoot(root);
    assertCurrent();
    if (stopped) throw new Error('Reset root owner disposed during registration');
  } catch (failure) {stop();throw failure;}
}

function bindModel(el: HTMLElement, sig: Signal<any>): void {
  return runWithOwnerScope([], () => {
    const owner = getCurrentOwner()!, core = getWasmCore(), generation = getReactiveGeneration(core);
    const assertLifetime = captureViewLifetime();
    const inputEl = el as HTMLInputElement;
    let active = true, composing = false, observer: MutationObserver | undefined;
    let synchronizers: Set<() => void> | undefined;
    let releaseReset: (() => void) | undefined;
    const isCurrent = () => active && !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);
    const assertCurrent = () => {
      assertLifetime();
      if (!active) throw new Error('Model binding disposed during construction');
    };
    const isTextModel = () => el.tagName === 'TEXTAREA' ||
      (el.tagName === 'INPUT' && ['text', 'search', 'tel', 'url', 'email', 'password'].includes(inputEl.type));
    const synchronize = () => {
      assertCurrent();
      const currentVal = sig();
      assertCurrent();
      if (composing && isTextModel()) return;
      if (el.tagName === 'INPUT' && inputEl.type === 'file') {
        // File selection is a native FileList, never a writable path string.
        // null clears; the native setter validates non-null FileList values.
        if (currentVal == null) {
          if (inputEl.value !== '') {
            assertCurrent(); inputEl.value = ''; domMutationCounter++; assertCurrent();
          }
        } else if (inputEl.files !== currentVal) {
          assertCurrent(); inputEl.files = currentVal; domMutationCounter++; assertCurrent();
        }
      } else if (el.tagName === 'SELECT' && (el as unknown as HTMLSelectElement).multiple) {
        const select = el as unknown as HTMLSelectElement;
        const values = new Set(Array.isArray(currentVal) ? currentVal.map(String) : []);
        assertCurrent();
        for (const option of Array.from(select.options)) {
          const nextSelected = values.has(option.value);
          assertCurrent();
          if (option.selected !== nextSelected) {
            assertCurrent(); option.selected = nextSelected; domMutationCounter++; assertCurrent();
          }
        }
      } else if (inputEl.type === 'checkbox' || inputEl.type === 'radio') {
        const nextChecked = inputEl.type === 'radio'
          ? currentVal != null && String(currentVal) === inputEl.value : Boolean(currentVal);
        assertCurrent();
        if (inputEl.checked !== nextChecked) {
          assertCurrent(); inputEl.checked = nextChecked; domMutationCounter++; assertCurrent();
        }
      } else {
        const nextStr = currentVal == null ? '' : String(currentVal);
        assertCurrent();
        if (inputEl.value !== nextStr) {
          assertCurrent(); inputEl.value = nextStr; domMutationCounter++; assertCurrent();
        }
      }
    };
    const synchronizeIfCurrent = () => { if (isCurrent()) synchronize(); else cleanup(); };
    const commitValue = (next: any) => { if (isCurrent()) sig.set(next); else cleanup(); };
    const updateFromDom = (event: Event) => {
      if (!isCurrent()) { cleanup(); return; }
      const isCheckbox = inputEl.type === 'checkbox', isRadio = inputEl.type === 'radio';
      const isFile = el.tagName === 'INPUT' && inputEl.type === 'file';
      const eventName = isCheckbox || isRadio || isFile || el.tagName === 'SELECT' ? 'change' : 'input';
      if (event.type !== eventName || (isTextModel() && (composing || (event as InputEvent).isComposing))) return;
      if (el.tagName === 'SELECT' && (el as unknown as HTMLSelectElement).multiple) {
        commitValue(Array.from((el as unknown as HTMLSelectElement).selectedOptions, option => option.value));
      } else if (isFile) commitValue(inputEl.files);
      else if (isCheckbox) commitValue(inputEl.checked);
      else if (isRadio) { if (inputEl.checked) commitValue(inputEl.value); }
      else if (inputEl.type === 'number' || inputEl.type === 'range') commitValue(inputEl.value === '' ? 0 : Number(inputEl.value));
      else commitValue(inputEl.value);
    };
    const startComposition = () => {
      if (!isCurrent()) { cleanup(); return; }
      if (isTextModel()) composing = true;
    };
    const endComposition = () => {
      if (!isCurrent()) { cleanup(); return; }
      const wasComposing = composing;
      composing = false;
      if (wasComposing && isTextModel()) commitValue(inputEl.value);
    };
    const listeners: Array<[string, EventListener]> = [
      ['compositionstart', startComposition], ['compositionend', endComposition],
      ['input', updateFromDom], ['change', updateFromDom],
    ];
    function cleanup() {
      active = false; composing = false;
      releaseReset?.();
      try { observer?.disconnect(); } catch { /* Complete the remaining cleanup. */ }
      for (const [name, listener] of listeners) {
        try { el.removeEventListener(name, listener); } catch { /* Inactive callbacks cannot publish even if native removal fails. */ }
      }
      synchronizers?.delete(synchronizeIfCurrent);
      if (synchronizers?.size === 0 && modelSynchronizers.get(el) === synchronizers) modelSynchronizers.delete(el);
    }
    // Register before initial synchronization or any native registration can reenter.
    onCleanup(cleanup);
    try {
      effect(synchronize);
      assertCurrent();
      synchronizers = modelSynchronizers.get(el);
      if (!synchronizers) { synchronizers = new Set(); modelSynchronizers.set(el, synchronizers); }
      synchronizers.add(synchronizeIfCurrent);
      if (el.tagName === 'SELECT') {
        const Observer = el.ownerDocument.defaultView?.MutationObserver ?? globalThis.MutationObserver;
        if (Observer) {
          const synchronizeOptions = () => {
            if (!isCurrent()) {cleanup();return;}
            if (inputEl.form && pendingResetForms.has(inputEl.form)) {
              // A queued option mutation must not restore the pre-reset model
              // before the reset publication has sampled native defaults.
              queueMicrotask(() => withWasmCore(core, () => untrack(synchronizeOptions)));
              return;
            }
            synchronizeIfCurrent();
          };
          observer = new Observer(() => withWasmCore(core, () => untrack(synchronizeOptions)));
          assertCurrent();
          observer.observe(el, {childList: true, subtree: true, characterData: true,
            attributes: true, attributeFilter: ['value', 'multiple']});
          assertCurrent();
        }
      }
      for (const [name, listener] of listeners) {
        assertCurrent(); el.addEventListener(name, listener); assertCurrent();
      }
      releaseReset = registerModelReset(el, {
        core, signal: sig, isCurrent, stop: cleanup,
        read: () => {
          if (!isCurrent()) return null;
          composing = false;
          let value: unknown, priority = 1;
          if (el.tagName === 'SELECT' && (el as unknown as HTMLSelectElement).multiple) {
            value = Array.from((el as unknown as HTMLSelectElement).selectedOptions, option => option.value);
          } else if (inputEl.type === 'radio') {
            const checked = inputEl.checked;
            value = checked ? inputEl.value : null; priority = checked ? 2 : 0;
          } else if (inputEl.type === 'checkbox') value = inputEl.checked;
          else if (el.tagName === 'INPUT' && inputEl.type === 'file') value = null;
          else if (inputEl.type === 'number' || inputEl.type === 'range') value = inputEl.value === '' ? 0 : Number(inputEl.value);
          else value = inputEl.value;
          return isCurrent() ? {value, priority} : null;
        },
      });
      assertCurrent();
    } catch (failure) {
      // Cleanup can have run before a synchronous hook attaches its late listener.
      cleanup();
      throw failure;
    }
  });
}

/**
 * Applies a static or reactive prop/attribute/event/model to a real DOM Element.
 */
export function bindProp(el: HTMLElement, key: string, value: any): void {
  if (key === 'children' || value === undefined || value === null) {
    return;
  }

  if (key === 'ref' && typeof value === 'function') {
    const assertCallerCurrent = captureViewLifetime();
    assertCallerCurrent();
    return runWithOwnerScope([], () => {
      const assertRefCurrent = captureViewLifetime();
      assertCallerCurrent(); assertRefCurrent();
      value(el);
      assertCallerCurrent(); assertRefCurrent();
    });
  }

  // First-class two-way signal binding: `<input model={textSignal} />`
  if (key === 'model' && typeof value === 'function' && 'set' in value) {
    bindModel(el, value as Signal<any>);
    return;
  }

  if (key === 'type' || key === 'value') {
    bindAttribute(el, key, value, true);
    return;
  }

  // Event listeners: `onClick`, `onInput`, etc. (with automatic Root Event Delegation)
  if (key.startsWith('on') && key.length > 2 && typeof value === 'function') {
    const eventName = key.slice(2).toLowerCase();
    const assertCurrent = captureViewLifetime(), owner = getCurrentOwner(), core = getWasmCore();
    const generation = getReactiveGeneration(core);
    const isCurrent = () => !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);
    const invoke = (target: HTMLElement, event: Event) => runUntrackedInCore(core, () =>
      runWithCapturedOwner(core, owner, () => value.call(target, event)));
    assertCurrent();
    if (DELEGATED_EVENTS.has(eventName)) {
      const area = lookupOwnerContext<DelegationArea | undefined>(delegationAreaKey, undefined);
      const actualRoot = el.getRootNode();
      const localRoot = actualRoot.nodeType === 11 && 'host' in actualRoot && (!area || !area.contains(el))
        ? actualRoot as ShadowRoot : area;
      const propKey = EVENT_PROP_PREFIX + eventName;
      const bindings: Set<DelegatedBinding> = (el as any)[propKey] ?? new Set();
      (el as any)[propKey] = bindings;
      const binding: DelegatedBinding = {
        handler(this: HTMLElement, event) {
          if (!binding.active || !isCurrent()) {cleanup();return;}
          return invoke(this, event);
        },
        active: true, root: localRoot,
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
        if (!binding.active || !isCurrent()) { release(); assertCurrent(); throw new Error('Delegated binding disposed'); }
        documentLeases.set(doc, release);
        assertCurrent();
      };
      const route: AdoptionRoute = {
        prepare(doc) {
          stagedDocuments.set(doc, (stagedDocuments.get(doc) ?? 0) + 1);
          const unpin = () => {
            const count = stagedDocuments.get(doc) ?? 0;
            if (count <= 1) stagedDocuments.delete(doc); else stagedDocuments.set(doc, count - 1);
          };
          try { retainDocument(doc); }
          catch (failure) {
            unpin();
            try { route.settle(); } catch { /* Preserve registration failure. */ }
            throw failure;
          }
          let finished = false;
          return () => { if (finished) return; finished = true; unpin(); route.settle(); };
        },
        settle() {
          if (!binding.active || !isCurrent()) { cleanup(); return; }
          const doc = el.ownerDocument;
          retainDocument(doc);
          for (const [other, release] of documentLeases) {
            if (other !== doc && !stagedDocuments.has(other)) { documentLeases.delete(other); release(); }
          }
        },
      };
      routes.add(route); adoptionRoutes.set(el, routes);
      const cleanup = () => {
        binding.active = false;
        bindings.delete(binding);
        if (bindings.size === 0 && (el as any)[propKey] === bindings) delete (el as any)[propKey];
        try { releaseLocal?.(); } catch { /* Complete document cleanup too. */ }
        routes.delete(route);
        if (!routes.size && adoptionRoutes.get(el) === routes) adoptionRoutes.delete(el);
        const leases = Array.from(documentLeases.values()); documentLeases.clear(); stagedDocuments.clear();
        for (const release of leases) { try { release(); } catch { /* Inactive bindings cannot publish. */ } }
        if (owner && !owner.disposed) {
          const index = owner.disposables.indexOf(cleanup);
          if (index >= 0) owner.disposables.splice(index, 1);
        }
      };
      onCleanup(cleanup);
      try {
        retainDocument(el.ownerDocument);
        assertCurrent();
        if (localRoot) releaseLocal = retainDelegatedListener(localRoot, eventName);
        assertCurrent();
        if (!binding.active) throw new Error('Delegated event owner disposed during registration');
      } catch (failure) {cleanup();throw failure;}
    } else {
      let active = true;
      const handler = function(this: HTMLElement, event: Event) {
        if (!active || !isCurrent()) {cleanup();return;}
        return invoke(this, event);
      };
      const cleanup = () => {
        active = false;
        // Repeat removal after registration reentry attaches a late listener.
        try { el.removeEventListener(eventName, handler); } catch { /* Inactive callbacks cannot publish. */ }
        if (owner && !owner.disposed) {
          const index = owner.disposables.indexOf(cleanup);
          if (index >= 0) owner.disposables.splice(index, 1);
        }
      };
      onCleanup(cleanup);
      try {
        el.addEventListener(eventName, handler);
        assertCurrent();
        if (!active) throw new Error('Event owner disposed during registration');
      } catch (failure) {cleanup();throw failure;}
    }
    return;
  }

  // Style construction owns all provisional nested property subscriptions.
  if (key === 'style') {
    runWithOwnerScope([], () => {
      const assertBindingCurrent = captureViewLifetime();
      const apply = () => {
        const assertRunCurrent = captureViewLifetime();
        const assertCurrent = () => {assertBindingCurrent();assertRunCurrent();};
        assertCurrent();
        const resolved = isReactiveAccessor(value) ? value() : value;
        assertCurrent();
        applyStyle(el, resolved, assertCurrent);
      };
      if (isReactiveAccessor(value)) effect(() => runWithOwnerScope([], apply));
      else apply();
    });
    return;
  }

  bindAttribute(el, key, value, false);
}

function bindAttribute(el: HTMLElement, key: string, value: any, notifyModel: boolean): void {
  const assertBindingCurrent = captureViewLifetime();
  assertBindingCurrent();
  const apply = () => {
    const assertRunCurrent = captureViewLifetime();
    const assertCurrent = () => {assertBindingCurrent();assertRunCurrent();};
    assertCurrent();
    const resolved = isReactiveAccessor(value) ? value() : value;
    assertCurrent();
    if (notifyModel) applyAttributeValue(el, key, resolved, assertCurrent);
    else applyStaticAttribute(el, key, resolved, assertCurrent);
  };
  if (isReactiveAccessor(value)) {
    runWithOwnerScope([], () => effect(() => runWithOwnerScope([], apply)));
  } else if (typeof value === 'object' && value !== null) {
    // User coercion can create provisional subscriptions before failing.
    runWithOwnerScope([], apply);
  } else apply();
}

const managedStyles = new WeakMap<HTMLElement, Set<string>>();
function cssPropertyName(name: string): string {
  if (name.startsWith('--')) return name;
  if (name === 'cssFloat') return 'float';
  return name.replace(/[A-Z]/g, char => `-${char.toLowerCase()}`).replace(/^ms-/, '-ms-');
}

function applyStyle(el: HTMLElement, styleVal: string | Record<string, any> | undefined, assertCurrent: () => void): void {
  assertCurrent();
  if (typeof styleVal === 'string') {
    if (el.style.cssText !== styleVal) {
      assertCurrent(); el.style.cssText = styleVal; domMutationCounter++; assertCurrent();
    }
    const properties = new Set(Array.from({ length: el.style.length }, (_, i) => {
      assertCurrent(); const name = el.style.item(i); assertCurrent(); return name;
    }));
    assertCurrent(); managedStyles.set(el, properties);
    return;
  }
  const keys = styleVal ? Object.keys(styleVal) : [];
  assertCurrent();
  const entries: Array<[string, any]> = [];
  for (const prop of keys) {
    assertCurrent(); const raw = (styleVal as Record<string, any>)[prop]; assertCurrent();
    entries.push([prop, raw]);
  }
  const nextProperties = new Set(entries.map(([prop]) => cssPropertyName(prop)));
  for (const prop of managedStyles.get(el) ?? []) {
    assertCurrent();
    if (!nextProperties.has(prop) && el.style.getPropertyValue(prop)) {
      assertCurrent(); el.style.removeProperty(prop); domMutationCounter++; assertCurrent();
    }
  }
  assertCurrent(); managedStyles.set(el, nextProperties);
  for (const [prop, rawVal] of entries) {
    assertCurrent();
    if (isReactiveAccessor(rawVal)) {
      effect(() => {
        const assertPropertyCurrent = captureViewLifetime();
        const assertValueCurrent = () => {assertCurrent();assertPropertyCurrent();};
        assertValueCurrent();
        const resolved = rawVal();
        assertValueCurrent();
        setStyleProperty(el, prop, resolved, assertValueCurrent);
      });
    } else {
      setStyleProperty(el, prop, rawVal, assertCurrent);
    }
  }
  assertCurrent();
}

function setStyleProperty(el: HTMLElement, prop: string, val: any, assertCurrent: () => void): void {
  assertCurrent();
  const str = val == null ? '' : String(val);
  assertCurrent();
  if (prop.includes('-')) {
    if (el.style.getPropertyValue(prop) !== str) {
      assertCurrent(); el.style.setProperty(prop, str); domMutationCounter++; assertCurrent();
    }
  } else if ((el.style as any)[prop] !== str) {
    assertCurrent(); (el.style as any)[prop] = str; domMutationCounter++; assertCurrent();
  }
  assertCurrent();
}

const URL_ATTRIBUTES = new Set(['href', 'src', 'xlink:href', 'action', 'formaction']);

function isUnsafeUrl(val: string): boolean {
  const clean = val.replace(/[\u0000-\u0020]+/g, '').toLowerCase();
  return (
    clean.startsWith('javascript:') ||
    clean.startsWith('vbscript:') ||
    clean.startsWith('data:text/html')
  );
}

const XLINK_NAMESPACE = 'http://www.w3.org/1999/xlink';
const BOOLEAN_TOKEN_ATTRIBUTES = new Set(['contenteditable', 'draggable', 'spellcheck']);

/** @internal Applies an attribute value without interpreting model/ref/event props. */
export function applyAttributeValue(el: HTMLElement, key: string, value: unknown, assertCurrent = captureViewLifetime()): void {
  assertCurrent();
  applyStaticAttribute(el, key, value, assertCurrent);
  assertCurrent();
  const name = key.toLowerCase();
  // Type and radio-option changes can affect checked without changing the model.
  if (name === 'type' || (name === 'value' && (el as HTMLInputElement).type === 'radio')) {
    untrack(() => {
      for (const synchronize of modelSynchronizers.get(el) ?? []) {
        assertCurrent(); synchronize(); assertCurrent();
      }
    });
  }
  assertCurrent();
}

function applyStaticAttribute(el: HTMLElement, key: string, val: any, assertCurrent = captureViewLifetime()): void {
  assertCurrent();
  const attrName = key === 'className' ? 'class' : key === 'xlinkHref' ? 'xlink:href' : key;
  const lowerAttr = attrName.toLowerCase();
  if (typeof val === 'boolean') {
    if (lowerAttr === 'translate') val = val ? 'yes' : 'no';
    else if (BOOLEAN_TOKEN_ATTRIBUTES.has(lowerAttr) || lowerAttr.startsWith('aria-') || lowerAttr.startsWith('data-')) val = String(val);
  }
  // Raw string/non-function inline event attributes remain blocked.
  if (lowerAttr.startsWith('on')) return;
  if (attrName === 'value' && 'value' in el) {
    assertCurrent(); const str = val == null ? '' : String(val); assertCurrent();
    if ((el as HTMLInputElement).value !== str) {
      assertCurrent(); (el as HTMLInputElement).value = str; domMutationCounter++; assertCurrent();
    }
    assertCurrent(); return;
  }
  if (attrName === 'checked' && 'checked' in el) {
    const bool = Boolean(val);
    if ((el as HTMLInputElement).checked !== bool) {
      assertCurrent(); (el as HTMLInputElement).checked = bool; domMutationCounter++; assertCurrent();
    }
    assertCurrent(); return;
  }
  if (val === false || val === null || val === undefined) {
    if (attrName === 'xlink:href' && typeof el.removeAttributeNS === 'function') {
      assertCurrent(); el.removeAttributeNS(XLINK_NAMESPACE, 'href'); domMutationCounter++; assertCurrent();
    } else if (el.hasAttribute(attrName)) {
      assertCurrent(); el.removeAttribute(attrName); domMutationCounter++; assertCurrent();
    }
  } else if (val === true) {
    if (!el.hasAttribute(attrName)) {
      assertCurrent(); el.setAttribute(attrName, ''); domMutationCounter++; assertCurrent();
    }
  } else {
    const str = String(val); assertCurrent();
    if ((URL_ATTRIBUTES.has(lowerAttr) || lowerAttr === 'xlink:href') && isUnsafeUrl(str)) return;
    if (attrName === 'xlink:href' && typeof el.setAttributeNS === 'function') {
      if (el.getAttributeNS(XLINK_NAMESPACE, 'href') !== str) {
        assertCurrent(); el.setAttributeNS(XLINK_NAMESPACE, 'xlink:href', str); domMutationCounter++; assertCurrent();
      }
      assertCurrent(); return;
    }
    if (el.getAttribute(attrName) !== str) {
      assertCurrent(); el.setAttribute(attrName, str); domMutationCounter++; assertCurrent();
    }
  }
  assertCurrent();
}

function isDomNode(value: unknown): value is Node {
  if (typeof Node === 'undefined' || value === null || typeof value !== 'object') return false;
  if (value instanceof Node) return true;
  // Native brand checking works across realms, including nodes adopted into a
  // document whose constructor no longer matches their original prototype.
  try { Node.prototype.hasChildNodes.call(value); return true; } catch { return false; }
}

function flattenChildValues(values: readonly ChildNodeValue[], cycleMessage: string): ChildNodeValue[] {
  const flattened: ChildNodeValue[] = [];
  const active = new Set<readonly ChildNodeValue[]>();
  const stack: Array<{ value: ChildNodeValue; exit?: boolean }> = [{ value: values as ChildNodeValue[] }];
  while (stack.length) {
    const frame = stack.pop()!, value = frame.value;
    if (Array.isArray(value)) {
      if (frame.exit) { active.delete(value); continue; }
      if (active.has(value)) throw new Error(cycleMessage);
      active.add(value); stack.push({ value, exit: true });
      const snapshot = Array.from(value);
      for (let i = snapshot.length - 1; i >= 0; i--) stack.push({ value: snapshot[i] });
    } else if (value != null && typeof value !== 'boolean') flattened.push(value);
  }
  return flattened;
}

function dynamicArrayNodes(values: readonly ChildNodeValue[], doc: Document, assertCurrent: () => void): Node[] {
  const flattened = flattenChildValues(values, 'Cyclic dynamic child array');
  const nodes: Node[] = [], seen = new Set<Node>();
  const add = (node: Node) => {
    if (seen.has(node)) throw new Error('Duplicate dynamic child node');
    seen.add(node); nodes.push(node);
  };
  for (const value of flattened) {
    assertCurrent();
    if (isDomNode(value)) {
      if (value.nodeType === 11) for (const node of Array.from(value.childNodes)) add(node);
      else add(value);
    } else if (isReactiveAccessor(value)) {
      const holder = doc.createDocumentFragment();
      insertChild(holder, value);
      for (const node of Array.from(holder.childNodes)) add(node);
    } else add(doc.createTextNode(String(value)));
    assertCurrent();
  }
  return nodes;
}

/**
 * Appends a static or reactive child into a parent DOM node or DocumentFragment.
 */
export function insertChild(parent: Node, child: ChildNodeValue, assertInsertionCurrent?: () => void): void {
  if (child === null || child === undefined || typeof child === 'boolean') {
    return;
  }

  const assertCurrent = assertInsertionCurrent ?? captureViewLifetime();
  assertCurrent();
  if (Array.isArray(child)) {
    // Snapshot/validate the entire tree before bindings or DOM mutation, then
    // recheck between nodes so one insertion cannot invalidate the remaining work.
    const values = flattenChildValues(child, 'Cyclic child array');
    assertCurrent();
    for (const value of values) insertChild(parent, value, assertCurrent);
    assertCurrent();
    return;
  }

  if (isDomNode(child)) {
    assertCurrent(); withDocumentAdoption(parent, child, () => { parent.appendChild(child); }, assertCurrent); domMutationCounter++; assertCurrent();
    return;
  }

  if (isReactiveAccessor(child)) {
    const assertParentCurrent = assertCurrent;
    let currentNode: Node | null = null, textNodeRef: Text | null = null;
    let marker: Comment | null = null, lastTextStr: string | null = null;
    let currentFragment: Node | null = null, grouped = false;
    const doc = parent.ownerDocument ?? document;
    let disposeContent: (() => void) | undefined;
    onCleanup(() => { const dispose = disposeContent; disposeContent = undefined; dispose?.(); });

    effect(() => createRoot(disposeCandidate => {
      const publishContent = () => {
        const previous = disposeContent;
        disposeContent = disposeCandidate;
        previous?.();
        assertCurrent();
      };
      const assertEffectCurrent = captureViewLifetime();
      const assertCurrent = () => { assertParentCurrent(); assertEffectCurrent(); };
      assertCurrent();
      const resolved = child();
      assertCurrent();
      if (currentFragment && resolved === currentFragment) {
        disposeCandidate(); assertParentCurrent(); return;
      }
      const nativeNode = isDomNode(resolved), array = Array.isArray(resolved);
      const fragment = nativeNode && resolved.nodeType === 11 ? resolved : null;
      const text = nativeNode || array ? null : resolved == null || typeof resolved === 'boolean' ? '' : String(resolved);
      assertCurrent();
      if (textNodeRef && (typeof resolved === 'string' || typeof resolved === 'number')) {
        if (lastTextStr !== text) {
          textNodeRef.nodeValue = text; domMutationCounter++;
          assertCurrent(); lastTextStr = text;
        }
        publishContent(); return;
      }
      if (!grouped && nativeNode && currentNode === resolved) {
        disposeCandidate(); assertParentCurrent(); return;
      }

      const oldNodes: Node[] = [];
      if (grouped) {
        let node = currentNode;
        while (node && node !== marker) { oldNodes.push(node); node = node.nextSibling; }
        if (!node || node !== marker) throw new Error('Broken dynamic fragment range');
      } else if (currentNode) oldNodes.push(currentNode);
      if (marker && !marker.parentNode) throw new Error('Broken dynamic child anchor');
      const target = marker?.parentNode ?? currentNode?.parentNode ?? parent;
      const originalAnchor = marker ?? currentNode?.nextSibling ?? null;
      const groupNodes = array ? dynamicArrayNodes(resolved, doc, assertCurrent) : fragment ? Array.from(fragment.childNodes) : null;
      const nextNode = groupNodes ? doc.createComment('svet-dyn-start') : nativeNode ? resolved : doc.createTextNode(text!);
      const nextNodes = groupNodes ? [nextNode, ...groupNodes] : [nextNode];
      const origins = nextNodes.map(node => ({ node, parent: node.parentNode, next: node.nextSibling }));
      const nextSet = new Set(nextNodes);
      let nextMarker = marker;
      const needsMarker = (nativeNode || array) && !nextMarker;
      if (needsMarker) nextMarker = doc.createComment('svet-dyn');
      const insert = (node: Node, anchor: Node | null) => {
        assertCurrent(); withDocumentAdoption(target, node, () => { target.insertBefore(node, anchor); }, assertCurrent); domMutationCounter++; assertCurrent();
      };
      try {
        if (needsMarker) insert(nextMarker!, originalAnchor);
        const anchor = nextMarker ?? originalAnchor;
        for (const node of nextNodes) insert(node, anchor);
        for (const node of oldNodes) {
          if (!nextSet.has(node) && node.parentNode) {
            assertCurrent(); node.parentNode.removeChild(node); domMutationCounter++; assertCurrent();
          }
        }
        assertCurrent();
        currentNode = nextNode; currentFragment = fragment; grouped = groupNodes !== null; marker = nextMarker;
        textNodeRef = nativeNode || array ? null : nextNode as Text; lastTextStr = text;
      } catch (failure) {
        let alive = true;
        try { assertCurrent(); } catch { alive = false; }
        // Include nodes attached after a synchronous hook invalidated the owner.
        for (const node of nextNodes) {
          try { if (node.parentNode) { node.parentNode.removeChild(node); domMutationCounter++; } } catch { /* Best effort. */ }
        }
        const restore = (action: () => void) => {
          if (!alive) return;
          try { assertCurrent(); action(); assertCurrent(); } catch {
            try { assertCurrent(); } catch { alive = false; }
          }
        };
        if (alive) {
          // Restore candidate origins (including a consumed fragment) for retry.
          if (fragment) {
            for (const node of nextNodes.slice(1)) restore(() => { withDocumentAdoption(fragment, node, () => { fragment.appendChild(node); }, assertCurrent); });
          } else {
            for (const origin of origins.slice().reverse()) {
              restore(() => { if (origin.parent) withDocumentAdoption(origin.parent, origin.node, () => { origin.parent!.insertBefore(origin.node, origin.next?.parentNode === origin.parent ? origin.next : null); }, assertCurrent); });
            }
          }
          const anchor = nextMarker?.parentNode === target ? nextMarker : originalAnchor?.parentNode === target ? originalAnchor : null;
          for (const node of oldNodes) restore(() => { withDocumentAdoption(target, node, () => { target.insertBefore(node, anchor); }, assertCurrent); domMutationCounter++; });
        }
        if (needsMarker) { try { nextMarker?.parentNode?.removeChild(nextMarker); } catch { /* Best effort. */ } }
        try { assertCurrent(); } catch { alive = false; }
        if (!alive) {
          for (const node of [...oldNodes, ...nextNodes]) {
            try { node.parentNode?.removeChild(node); } catch { /* Never revive invalidated content. */ }
          }
        }
        throw failure;
      }
      publishContent();
    }));
    assertCurrent();
    return;
  }

  const text = String(child);
  assertCurrent();
  const textNode = document.createTextNode(text);
  assertCurrent(); parent.appendChild(textNode); domMutationCounter++; assertCurrent();
}

const SVG_NAMESPACE = 'http://www.w3.org/2000/svg';
const HTML_NAMESPACE = 'http://www.w3.org/1999/xhtml';
const SVG_TAGS = new Set([
  'svg',
  'path',
  'circle',
  'rect',
  'line',
  'polyline',
  'polygon',
  'ellipse',
  'g',
  'defs',
  'linearGradient',
  'radialGradient',
  'stop',
  'use',
  'text',
  'tspan',
  'clipPath',
  'mask',
  'pattern',
  'symbol',
  'marker',
  'foreignObject',
  'filter',
  'feGaussianBlur',
  'feOffset',
  'feBlend',
  'feColorMatrix',
  'desc',
  'metadata',
  'switch',
  'view',
]);

const AMBIGUOUS_SVG_HTML_TAGS = new Set(['a', 'title', 'style', 'script']);

function upgradeAmbiguousSvgChild(parent: HTMLElement, childNode: Node): Node {
  if (parent.namespaceURI !== SVG_NAMESPACE || parent.tagName.toLowerCase() === 'foreignobject') {
    return childNode;
  }
  const element = childNode.nodeType === 1 ? childNode as Element : null;
  if (
    element &&
    element.namespaceURI === HTML_NAMESPACE &&
    AMBIGUOUS_SVG_HTML_TAGS.has(element.tagName.toLowerCase())
  ) {
    const svgClone = document.createElementNS(SVG_NAMESPACE, element.tagName.toLowerCase());
    for (let i = 0; i < element.attributes.length; i++) {
      const attr = element.attributes[i];
      if (attr.namespaceURI) {
        svgClone.setAttributeNS(attr.namespaceURI, attr.name, attr.value);
      } else {
        svgClone.setAttribute(attr.name, attr.value);
      }
    }
    // Preserve delegated event handlers (`__svet_evt_*`) attached to `childNode`
    for (const propKey of Object.keys(childNode)) {
      if (propKey.startsWith(EVENT_PROP_PREFIX)) {
        (svgClone as any)[propKey] = (childNode as any)[propKey];
      }
    }
    while (childNode.firstChild) {
      svgClone.appendChild(
        upgradeAmbiguousSvgChild(svgClone as unknown as HTMLElement, childNode.firstChild)
      );
    }
    return svgClone;
  }
  return childNode;
}

/**
 * Core hyperscript / JSX factory that builds real DOM & SVG nodes directly (Zero VDOM).
 */
export function h(
  type: string | Component<any>,
  props: Record<string, any> | null,
  ...children: ChildNodeValue[]
): Node {
  const assertCallerCurrent = captureViewLifetime();
  assertCallerCurrent();
  return runWithOwnerScope([], () => {
    const assertConstructionCurrent = captureViewLifetime();
    const assertCurrent = () => {assertCallerCurrent();assertConstructionCurrent();};
    assertCurrent();
    const normalizedProps: Record<string | symbol, any> = {};
    if (props) {
      const keys = Reflect.ownKeys(props);
      assertCurrent();
      for (const key of keys) {
        assertCurrent();
        const descriptor = Object.getOwnPropertyDescriptor(props, key);
        assertCurrent();
        if (!descriptor?.enumerable) continue;
        const value = Reflect.get(props, key);
        assertCurrent();
        Object.defineProperty(normalizedProps, key, {value, enumerable: true, configurable: true, writable: true});
      }
    }
    if (children.length > 0) {
      normalizedProps.children = children.length === 1 ? children[0] : children;
    }

    if (typeof type === 'function') {
      const result = untrack(() => type(normalizedProps));
      assertCurrent();
      return result;
    }

    const forceSvg =
      SVG_TAGS.has(type) ||
      normalizedProps.xmlns === SVG_NAMESPACE ||
      (AMBIGUOUS_SVG_HTML_TAGS.has(type) &&
        ('xlink:href' in normalizedProps || 'xlinkHref' in normalizedProps));

    const el = forceSvg
      ? (document.createElementNS(SVG_NAMESPACE, type) as unknown as HTMLElement)
      : document.createElement(type);
    assertCurrent();
    // Ensure `type` attribute is applied BEFORE `model` or `value` so `<input type="checkbox" model={...}>` knows its type
    if ('type' in normalizedProps) {
      bindProp(el, 'type', normalizedProps.type);
      assertCurrent();
    }
    for (const [k, v] of Object.entries(normalizedProps)) {
      if (k !== 'children' && k !== 'type') {
        assertCurrent();
        bindProp(el, k, v);
        assertCurrent();
      }
    }
    if (normalizedProps.children !== undefined) {
      if (el.namespaceURI === SVG_NAMESPACE && type !== 'foreignObject') {
        const upgraded = Array.isArray(normalizedProps.children)
          ? normalizedProps.children.map((c: any) =>
              isDomNode(c) ? upgradeAmbiguousSvgChild(el, c) : c
            )
          : isDomNode(normalizedProps.children)
          ? upgradeAmbiguousSvgChild(el, normalizedProps.children)
          : normalizedProps.children;
        insertChild(el, upgraded, assertCurrent);
      } else {
        insertChild(el, normalizedProps.children, assertCurrent);
      }
    }
    if (el.tagName === 'SELECT') {
      untrack(() => { for (const synchronize of modelSynchronizers.get(el) ?? []) { assertCurrent(); synchronize(); assertCurrent(); } });
    }
    // A live form owns routing even before it has any modeled controls. Native
    // later insertions/moves must not depend on a mutation-observer checkpoint.
    if (el.tagName === 'FORM') retainOwnedResetRoot(el);
    assertCurrent();
    return el;
  });
}

export function Fragment(props: { children?: ChildNodeValue }): DocumentFragment {
  const assertCallerCurrent = captureViewLifetime();
  assertCallerCurrent();
  return runWithOwnerScope([], () => {
    const assertFragmentCurrent = captureViewLifetime();
    const assertCurrent = () => {assertCallerCurrent();assertFragmentCurrent();};
    assertCurrent();
    const frag = document.createDocumentFragment();
    assertCurrent();
    const children = props.children;
    assertCurrent();
    insertChild(frag, children, assertCurrent);
    assertCurrent();
    return frag;
  });
}

/** @internal Captures a view owner and graph generation before user rendering. */
export function captureViewLifetime(): () => void {
  const owner = getCurrentOwner(), core = getWasmCore(), generation = getReactiveGeneration(core);
  return () => {
    if (isOwnerScopeDisposed(owner)) throw new Error('View owner disposed during rendering');
    if (generation !== getReactiveGeneration(core)) throw new Error('Reactive graph reset during view rendering');
  };
}

/** @internal Commits a branch with partial-insertion cleanup. */
export function mountOwnedViewNodes(
  fragment: DocumentFragment, anchor: Comment, content: ChildNodeValue, assertViewCurrent: () => void,
  onMutation?: () => void
): Node[] {
  const branch = getCurrentOwner();
  const assertCurrent = () => {
    assertViewCurrent();
    if (branch?.disposed) throw new Error('View branch owner disposed during rendering');
  };
  let nodes: Node[] = [];
  try {
    assertCurrent();
    const temp = anchor.ownerDocument.createDocumentFragment();
    insertChild(temp, content, assertCurrent);
    assertCurrent();
    nodes = Array.from(temp.childNodes);
    for (const node of nodes) {
      assertCurrent();
      const target = anchor.parentNode ?? fragment;
      withDocumentAdoption(target, node, () => { target.insertBefore(node, anchor); }, assertCurrent);
      onMutation?.();
      assertCurrent();
    }
    return nodes;
  } catch (failure) {
    for (const node of nodes) {
      try { if (node.parentNode) { node.parentNode.removeChild(node); onMutation?.(); } } catch { /* Preserve the insertion failure. */ }
    }
    throw failure;
  }
}

/**
 * Reactive conditional flow component (`<Show when={cond} fallback={...}>`).
 * Creates an isolated `OwnerScope` for the active branch so when the condition flips,
 * all nested effects/computeds inside the old branch are disposed and recycled in WASM!
 * Preserves existing DOM nodes when `Boolean(when)` stays truthy unless `children` is a keyed value callback `(val) => Node`.
 */
export function Show<T>(props: {
  when: ReactiveValue<T>;
  fallback?: ChildNodeValue | (() => Node);
  children: ChildNodeValue | ((item: NonNullable<T>) => Node);
}): DocumentFragment {
  return runWithOwnerScope([], () => {
    const assertViewCurrent = captureViewLifetime();
    const frag = document.createDocumentFragment();
    const anchor = document.createComment('svet-show');
    frag.appendChild(anchor);
    let currentNodes: Node[] = [];
    let disposeBranchScope: (() => void) | null = null;
    let activeState: 'none' | 'truthy' | 'falsy' = 'none';
    let lastTruthyVal: unknown = undefined;
    const initialChildren = props.children;
    assertViewCurrent();
    const isCallbackChild = typeof initialChildren === 'function' && initialChildren.length > 0;

    const clearCurrent = () => {
      activeState = 'none'; lastTruthyVal = undefined;
      const dispose = disposeBranchScope; disposeBranchScope = null;
      dispose?.();
      const nodes = currentNodes; currentNodes = [];
      for (const node of nodes) {
        if (node.parentNode) { node.parentNode.removeChild(node); domMutationCounter++; }
      }
    };
    onCleanup(clearCurrent);

    const mountBranch = (factory: () => ChildNodeValue) => {
      createRoot(dispose => {
        disposeBranchScope = dispose;
        const content = untrack(factory);
        currentNodes = mountOwnedViewNodes(frag, anchor, content, assertViewCurrent, () => { domMutationCounter++; });
      });
    };

    effect(() => {
      assertViewCurrent();
      const when = props.when;
      assertViewCurrent();
      const cond = isReactiveAccessor(when) ? when() : when;
      assertViewCurrent();
      const truthy = Boolean(cond);
      if (truthy && activeState === 'truthy' && (!isCallbackChild || Object.is(lastTruthyVal, cond))) return;
      if (!truthy && activeState === 'falsy') return;
      clearCurrent();
      assertViewCurrent();
      if (truthy) {
        mountBranch(() => {
          const children = props.children;
          assertViewCurrent();
          return typeof children === 'function' ? (children as (value: NonNullable<T>) => Node)(cond as NonNullable<T>) : children;
        });
      } else {
        const fallback = props.fallback;
        assertViewCurrent();
        if (fallback !== undefined) mountBranch(() => typeof fallback === 'function' ? (fallback as () => Node)() : fallback);
      }
      assertViewCurrent();
      // Only a successfully committed branch may suppress a later same-state run.
      activeState = truthy ? 'truthy' : 'falsy';
      lastTruthyVal = truthy ? cond : undefined;
    });
    assertViewCurrent();
    return frag;
  });
}

const NEW_ITEM_MARKER = 0xffffffff;

/**
 * WASM-Accelerated Keyed List Reconciler (`<For each={items} keyFn={...}>`).
 *
 * - Uses `computeLisWasm` in Rust WASM linear memory ($O(n \log n)$) to find the
 *   Longest Increasing Subsequence and move ONLY the out-of-order DOM nodes.
 * - Wraps each row in its own `createRoot` ownership scope so removed rows automatically
 *   dispose all their inner effects and return their WASM node IDs to `free_list`!
 * - Provides a lazy reactive `index()` signal accessor so rows tracking their index update in $O(1)$ on reorder!
 */
export function For<T, K = T>(props: {
  each: ReactiveValue<readonly T[]>;
  keyFn?: (item: T, index: number) => K;
  children: (item: T, index: () => number, current: () => T) => Node;
}): DocumentFragment {
  return runWithOwnerScope([], () => {
    const assertViewCurrent = captureViewLifetime(), core = getWasmCore();
    const frag = document.createDocumentFragment();
    const endMarker = document.createComment('svet-for-end');
    frag.appendChild(endMarker);
    type MappedRecord = { key: K; item: T; itemSig?: Signal<T>; node: Node; end?: Node; preparedNodes?: Node[]; movingNodes?: Node[]; index: number; indexSig?: Signal<number>; disposeScope: () => void };
    let currentRecords: MappedRecord[] = [], scratchRecords: MappedRecord[] = [];
    const recordMap = new Map<K, MappedRecord>();
    const intKeyTable: Array<MappedRecord | undefined> = [];
    let scratchIndices = new Uint32Array(64);
    const isIntKey = (key: K): boolean => typeof key === 'number' && key >= 0 && key < 65536 && (key | 0) === key;
    const removeNode = (node: Node) => { if (node.parentNode) { node.parentNode.removeChild(node); domMutationCounter++; } };
    let updating = false;
    const rowNodes = (rec: MappedRecord): Node[] => {
      if (rec.movingNodes) return rec.movingNodes;
      if (rec.preparedNodes) return rec.preparedNodes;
      if (!rec.node) return [];
      if (!rec.end) return [rec.node];
      const nodes: Node[] = [];
      let node: Node | null = rec.node;
      while (node) {
        nodes.push(node);
        if (node === rec.end) return nodes;
        node = node.nextSibling;
      }
      throw new Error('Broken For row range');
    };
    const clearSnapshots = (records: readonly MappedRecord[]) => {
      for (const rec of records) { rec.preparedNodes = undefined; rec.movingNodes = undefined; }
    };
    const discardRows = (records: readonly MappedRecord[]) => {
      for (const rec of records) {
        let nodes: Node[];
        try { nodes = rowNodes(rec); } catch { nodes = [rec.node, rec.end].filter((node): node is Node => !!node); }
        try { rec.disposeScope(); } catch { /* Complete remaining row teardown. */ }
        for (const node of nodes) { try { removeNode(node); } catch { /* Teardown remains best effort. */ } }
      }
      // A synchronous insertion hook may still attach a node after owner cleanup.
      if (!updating) clearSnapshots(records);
    };
    const moveRow = (rec: MappedRecord, parent: Node, anchor: Node) => {
      if (!rec.end) {
        assertViewCurrent(); withDocumentAdoption(parent, rec.node, () => { parent.insertBefore(rec.node, anchor); }, assertViewCurrent); domMutationCounter++; assertViewCurrent();
        return;
      }
      const nodes = rowNodes(rec);
      rec.movingNodes = nodes;
      for (const node of nodes) {
        assertViewCurrent(); withDocumentAdoption(parent, node, () => { parent.insertBefore(node, anchor); }, assertViewCurrent); domMutationCounter++; assertViewCurrent();
      }
      rec.movingNodes = undefined; rec.preparedNodes = undefined;
    };
    onCleanup(() => {
      const records = currentRecords;
      currentRecords = []; scratchRecords = []; recordMap.clear(); intKeyTable.length = 0;
      discardRows(records);
      try { removeNode(endMarker); } catch { /* Preserve owner teardown. */ }
    });

    effect(() => {
      assertViewCurrent();
      const each = props.each;
      assertViewCurrent();
      const list = isReactiveAccessor(each) ? each() : each;
      const items = Array.from(list);
      assertViewCurrent();
      const getKey = props.keyFn ?? ((item: T) => item as unknown as K);
      const render = props.children;
      assertViewCurrent();
      const keys: K[] = [], seen = new Set<K>();
      for (let i = 0; i < items.length; i++) {
        const key = getKey(items[i], i);
        assertViewCurrent();
        if (seen.has(key)) throw new Error('Duplicate For key');
        seen.add(key); keys.push(key);
      }
      const oldRecords = currentRecords, oldLen = oldRecords.length, nextLen = items.length;
      if (scratchIndices.length < nextLen) scratchIndices = new Uint32Array(Math.max(nextLen, 64));
      const oldIndices = scratchIndices;
      const nextRecords = scratchRecords.length === nextLen ? scratchRecords : new Array<MappedRecord>(nextLen);
      const created: MappedRecord[] = [], removed: MappedRecord[] = [];
      const parent = endMarker.parentNode ?? frag;
      let anyMovedOrNew = false, prevOldIdx = -1, displacedCount = 0;
      let firstDisplacedIdx = -1, secondDisplacedIdx = -1, reusedCount = 0, committed = false;
      updating = true;
      try {
        // Prepare rows without publishing lookup entries or changing existing indexes.
        for (let i = 0; i < nextLen; i++) {
          const item = items[i], key = keys[i];
          const existing = isIntKey(key) ? intKeyTable[key as number] : recordMap.get(key);
          if (existing) {
            reusedCount++;
            const oldIdx = existing.index;
            oldIndices[i] = oldIdx;
            if (oldIdx < prevOldIdx) {
              anyMovedOrNew = true;
              const prevWasJump = i >= 2 ? oldIndices[i - 1] > oldIdx && oldIdx > oldIndices[i - 2] : oldIndices[0] > oldIdx && i + 1 < nextLen;
              const displacedPos = prevWasJump ? i - 1 : i;
              displacedCount++;
              if (displacedCount === 1) firstDisplacedIdx = displacedPos;
              else if (displacedCount === 2) secondDisplacedIdx = displacedPos;
              if (prevWasJump) prevOldIdx = oldIdx;
            } else prevOldIdx = oldIdx;
            nextRecords[i] = existing;
          } else {
            oldIndices[i] = NEW_ITEM_MARKER; anyMovedOrNew = true; displacedCount = 999;
            let rowDispose: (() => void) | undefined, rowDisposed = false;
            const rec: MappedRecord = {
              key, item, node: null as unknown as Node, index: i,
              disposeScope() {
                if (rowDisposed) return;
                rowDisposed = true;
                try { rowDispose?.(); } finally { try { rec.indexSig?.dispose(); } finally { rec.itemSig?.dispose(); } }
              },
            };
            created.push(rec);
            const index = () => {
              if (!rec.indexSig) {
                if (rowDisposed) return rec.index;
                assertViewCurrent();
                rec.indexSig = withWasmCore(core, () => signal(rec.index));
              }
              return rec.indexSig();
            };
            const current = (): T => {
              if (!rec.itemSig) {
                if (rowDisposed) return rec.item;
                assertViewCurrent();
                rec.itemSig = withWasmCore(core, () => signal(rec.item));
              }
              return rec.itemSig();
            };
            rec.node = untrack(() => createRoot(dispose => {
              rowDispose = dispose;
              const node = render(item, index, current);
              assertViewCurrent();
              if (node.nodeType === 11) {
                const start = node.ownerDocument!.createComment('svet-for-row');
                const end = node.ownerDocument!.createComment('svet-for-row-end');
                rec.node = start; rec.end = end;
                rec.preparedNodes = [start, ...Array.from(node.childNodes), end];
                node.insertBefore(start, node.firstChild);
                assertViewCurrent();
                node.appendChild(end);
                assertViewCurrent();
                return start;
              }
              return node;
            }));
            assertViewCurrent();
            nextRecords[i] = rec;
          }
        }
        if (reusedCount < oldLen) {
          const nextSet = new Set(nextRecords);
          for (const rec of oldRecords) if (!nextSet.has(rec)) removed.push(rec);
        }
        if (anyMovedOrNew) {
          if (displacedCount > 0 && displacedCount <= 2 && reusedCount === nextLen && oldLen === nextLen) {
            // Move only displaced rows; fragment rows move their complete live ranges.
            const moveIdx = (idx: number) => {
              const rec = nextRecords[idx];
              const anchor = idx + 1 < nextLen ? nextRecords[idx + 1].node : endMarker;
              moveRow(rec, parent, anchor);
            };
            if (displacedCount === 2) {
              moveIdx(secondDisplacedIdx);
            }
            moveIdx(firstDisplacedIdx);
          } else {
            const lisPositions = computeLisWasm(oldIndices.subarray(0, nextLen));
            let lisCursor = lisPositions.length - 1;

            let anchorNode: Node = endMarker;
            for (let i = nextLen - 1; i >= 0; i--) {
              const rec = nextRecords[i];
              const isNew = oldIndices[i] === NEW_ITEM_MARKER;
              let isStationary = false;
              if (!isNew && lisCursor >= 0 && lisPositions[lisCursor] === i) {
                isStationary = true;
                lisCursor--;
              }

              if (!isStationary) {
                moveRow(rec, parent, anchorNode);
              }
              anchorNode = rec.node;
            }
          }
        }

        assertViewCurrent();
        // DOM preparation succeeded. Publish records before any teardown callbacks.
        for (let i = 0; i < nextLen; i++) {
          const rec = nextRecords[i];
          rec.index = i; rec.item = items[i];
          rec.indexSig?.set(i);
          // A function-valued item is data, never a signal updater.
          rec.itemSig?.set(() => rec.item);
        }
        for (const rec of removed) {
          if (isIntKey(rec.key)) intKeyTable[rec.key as number] = undefined;
          else recordMap.delete(rec.key);
        }
        for (const rec of created) {
          if (isIntKey(rec.key)) intKeyTable[rec.key as number] = rec;
          else recordMap.set(rec.key, rec);
        }
        if (nextLen === 0) { recordMap.clear(); intKeyTable.length = 0; }
        scratchRecords = oldRecords; currentRecords = nextRecords; committed = true;
        for (const rec of removed) {
          const nodes = rowNodes(rec); rec.disposeScope();
          for (const node of nodes) removeNode(node);
        }
        clearSnapshots(removed);
        assertViewCurrent();
      } catch (failure) {
        let alive = true;
        try { assertViewCurrent(); } catch { alive = false; }
        if (!alive) {
          currentRecords = []; scratchRecords = []; recordMap.clear(); intKeyTable.length = 0;
          discardRows(oldRecords); discardRows(created);
          clearSnapshots(oldRecords); clearSnapshots(created);
          try { removeNode(endMarker); } catch { /* Preserve lifetime failure. */ }
        } else if (!committed) {
          scratchRecords = [];
          discardRows(created); clearSnapshots(created);
          // Old rows are still live: restore their exact committed order.
          for (const rec of oldRecords) {
            try { moveRow(rec, parent, endMarker); } catch { /* Preserve original failure. */ }
          }
        } else { discardRows(removed); clearSnapshots(removed); }
        throw failure;
      } finally { updating = false; }
    });
    assertViewCurrent();
    return frag;
  });
}

const mountedContainers = new WeakMap<HTMLElement | ShadowRoot, () => void>();
const mountAttempts = new WeakMap<HTMLElement | ShadowRoot, object>();

/**
 * Mounts a Svet component or DOM subtree into a target container inside a root `OwnerScope`.
 * Calling `unmount()` disposes the entire reactive tree and recycles all WASM node slots!
 */
export function mount(view: () => Node, container: HTMLElement | ShadowRoot): { unmount(): void } {
  const assertCallerCurrent = captureViewLifetime();
  assertCallerCurrent();
  const attempt = {};
  mountAttempts.set(container, attempt);
  let assertRootCurrent!: () => void;
  const assertMountCurrent = () => {
    assertCallerCurrent(); assertRootCurrent();
    if (mountAttempts.get(container) !== attempt) throw new Error('Mount superseded by a newer mount');
  };
  let disposeRoot!: () => void;
  let node: Node;
  try {
    node = createRoot(dispose => {
      disposeRoot = dispose;
      assertRootCurrent = captureViewLifetime();
      // The stable container covers descendant forms after native subtree moves;
      // its tree root also covers an enclosing form outside the container.
      retainOwnedResetRoot(container);
      assertMountCurrent();
      const treeRoot = container.getRootNode();
      if (treeRoot !== container) retainOwnedResetRoot(treeRoot);
      const mountOwner = getCurrentOwner()!;
      (mountOwner.context ??= new Map()).set(delegationAreaKey, treeRoot.nodeType === 11 && 'host' in treeRoot ? container : undefined);
      assertMountCurrent();
      const result = view();
      assertMountCurrent();
      return result;
    });
  } catch (error) {
    disposeRoot?.();
    throw error;
  }
  // Preserve fragment contents before insertion consumes the fragment.
  const preparedNodes = node.nodeType === 11 ? Array.from(node.childNodes) : [node];
  // Prepare first: a failed view must not tear down the currently mounted app.
  try {
    assertMountCurrent();
    mountedContainers.get(container)?.();
    assertMountCurrent();
    withDocumentAdoption(container, node, () => { container.replaceChildren(node); }, assertMountCurrent);
    assertMountCurrent();
  } catch (error) {
    try { disposeRoot(); } catch { /* Preserve the DOM/replacement failure. */ }
    for (const prepared of preparedNodes) {
      try { if (prepared.parentNode === container) container.removeChild(prepared); }
      catch { /* Preserve the original failure if native rollback also fails. */ }
    }
    throw error;
  }
  let disposed = false;
  const unmount = () => {
    if (disposed) return;
    disposed = true;
    let domFailed = false;
    try {
      if (mountedContainers.get(container) === unmount) {
        mountedContainers.delete(container);
        container.replaceChildren();
      }
    } catch (error) {
      domFailed = true;
      throw error;
    } finally {
      try { disposeRoot(); } catch (error) {
        if (!domFailed) throw error;
      }
    }
  };
  mountedContainers.set(container, unmount);
  onCleanup(unmount);
  return { unmount };
}
