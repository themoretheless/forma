/**
 * Two-way `model={signal}` binding and form reset coordination for Svet DOM controls.
 */

import {
  isMultipleSelect,
  modelSynchronizers,
  selectedSelectValues,
  synchronizeSelectValue,
} from './dom-attributes.ts';
import {
  adoptionRoutes,
  type AdoptionRoute,
  captureViewLifetime,
  domMutationState,
} from './dom-events.ts';
import {
  batch,
  effect,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  runWithOwnerScope,
  untrack,
} from './reactivity.ts';
import { getWasmCore, withWasmCore } from './wasm-bridge.ts';

export type WritableModelSignal = (() => unknown) & { set(value: unknown): void };

type ModelResetBinding = {
  core: ReturnType<typeof getWasmCore>;
  signal: WritableModelSignal;
  isCurrent: () => boolean;
  stop: () => void;
  read: () => { value: unknown; priority: number } | null;
};

const resetBindings = new WeakMap<Element, Set<ModelResetBinding>>();
const resetRoots = new WeakMap<EventTarget, { count: number; listener: EventListener }>();
const pendingResetEvents = new WeakSet<Event>();
export const pendingResetForms = new WeakMap<HTMLFormElement, number>();

function formControls(form: HTMLFormElement): Element[] {
  // Named controls can shadow form.elements just as they can shadow form.reset.
  const prototype =
    form.ownerDocument.defaultView?.HTMLFormElement.prototype ??
    globalThis.HTMLFormElement?.prototype;
  const getter = prototype && Object.getOwnPropertyDescriptor(prototype, 'elements')?.get;
  return Array.from(getter ? getter.call(form) : form.elements);
}

function handleModelReset(event: Event): void {
  const form = event.target as HTMLFormElement | null;
  if (!form || form.nodeType !== 1 || form.tagName !== 'FORM' || pendingResetEvents.has(event))
    return;
  pendingResetEvents.add(event);
  pendingResetForms.set(form, (pendingResetForms.get(form) ?? 0) + 1);
  queueMicrotask(() => {
    pendingResetEvents.delete(event);
    const remaining = (pendingResetForms.get(form) ?? 1) - 1;
    if (remaining) pendingResetForms.set(form, remaining);
    else pendingResetForms.delete(form);
    if (event.defaultPrevented) return;
    const groups = new Map<
      ReturnType<typeof getWasmCore>,
      Map<WritableModelSignal, { binding: ModelResetBinding; value: unknown; priority: number }>
    >();
    // Read all reset values before publishing anything: radio/model effects can
    // otherwise alter the native state of controls not yet visited.
    for (const el of formControls(form)) {
      if ((el as HTMLInputElement).form !== form) continue;
      for (const binding of resetBindings.get(el) ?? []) {
        if (!binding.isCurrent()) {
          binding.stop();
          continue;
        }
        const next = withWasmCore(binding.core, () => untrack(binding.read));
        if (!binding.isCurrent()) {
          binding.stop();
          continue;
        }
        if (!next) continue;
        let group = groups.get(binding.core);
        if (!group) {
          group = new Map();
          groups.set(binding.core, group);
        }
        const previous = group.get(binding.signal);
        if (!previous || next.priority >= previous.priority)
          group.set(binding.signal, { binding, ...next });
      }
    }
    for (const [core, group] of groups)
      withWasmCore(core, () =>
        batch(() => {
          for (const { binding, value } of group.values()) {
            if (binding.isCurrent()) binding.signal.set(value);
            else binding.stop();
          }
        }),
      );
  });
}

function retainResetRoot(root: EventTarget): () => void {
  let entry = resetRoots.get(root);
  if (entry) entry.count++;
  else {
    const listener: EventListener = event => {
      if (entry && entry.count > 0) handleModelReset(event);
    };
    entry = { count: 1, listener };
    resetRoots.set(root, entry);
    try {
      root.addEventListener('reset', listener, true);
    } catch (failure) {
      entry.count = 0;
      if (resetRoots.get(root) === entry) resetRoots.delete(root);
      try {
        root.removeEventListener('reset', listener, true);
      } catch {
        /* Preserve registration failure. */
      }
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
      try {
        root.removeEventListener('reset', retained.listener, true);
      } catch {
        /* Weak binding lookup still prevents stale publication. */
      }
    }
  };
}

function registerModelReset(el: HTMLElement, binding: ModelResetBinding): () => void {
  let entries = resetBindings.get(el);
  if (!entries) {
    entries = new Set();
    resetBindings.set(el, entries);
  }
  entries.add(binding);
  const roots = new Map<EventTarget, () => void>(),
    stagedRoots = new Map<EventTarget, number>();
  const routes = adoptionRoutes.get(el) ?? new Set<AdoptionRoute>();
  let active = true;
  const assertCurrent = () => {
    if (!active || !binding.isCurrent())
      throw new Error('Model binding disposed during document adoption');
  };
  const retain = (root: EventTarget) => {
    assertCurrent();
    if (roots.has(root)) return;
    const stop = retainResetRoot(root);
    if (!active || !binding.isCurrent()) {
      stop();
      assertCurrent();
    }
    roots.set(root, stop);
    assertCurrent();
  };
  const release = () => {
    active = false;
    entries!.delete(binding);
    if (!entries!.size && resetBindings.get(el) === entries) resetBindings.delete(el);
    routes.delete(route);
    if (!routes.size && adoptionRoutes.get(el) === routes) adoptionRoutes.delete(el);
    const leases = Array.from(roots.values());
    roots.clear();
    stagedRoots.clear();
    for (const stop of leases) stop();
  };
  const route: AdoptionRoute = {
    prepare(doc, parent) {
      const targets = new Set<EventTarget>([doc, parent.getRootNode()]);
      const unpin = () => {
        for (const root of targets) {
          const count = stagedRoots.get(root) ?? 0;
          if (count <= 1) stagedRoots.delete(root);
          else stagedRoots.set(root, count - 1);
        }
      };
      for (const root of targets) stagedRoots.set(root, (stagedRoots.get(root) ?? 0) + 1);
      try {
        for (const root of targets) retain(root);
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
      if (!active || !binding.isCurrent()) {
        release();
        return;
      }
      const desired = new Set<EventTarget>([el.ownerDocument]);
      const form = (el as HTMLInputElement).form,
        root = el.getRootNode();
      if (form) desired.add(form);
      if (root !== el && root.nodeType !== 9) desired.add(root);
      for (const target of desired) retain(target);
      for (const [target, stop] of roots) {
        if (!desired.has(target) && !stagedRoots.has(target)) {
          roots.delete(target);
          stop();
        }
      }
    },
  };
  routes.add(route);
  adoptionRoutes.set(el, routes);
  try {
    retain(el.ownerDocument);
    const form = (el as HTMLInputElement).form;
    if (form) retain(form);
    return release;
  } catch (failure) {
    release();
    throw failure;
  }
}

export function retainOwnedResetRoot(root: EventTarget): void {
  const assertCurrent = captureViewLifetime(),
    owner = getCurrentOwner();
  assertCurrent();
  let release: (() => void) | undefined,
    stopped = false;
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
  } catch (failure) {
    stop();
    throw failure;
  }
}

function isTextModelControl(el: HTMLElement, inputEl: HTMLInputElement): boolean {
  return (
    el.tagName === 'TEXTAREA' ||
    (el.tagName === 'INPUT' &&
      ['text', 'search', 'tel', 'url', 'email', 'password'].includes(inputEl.type))
  );
}

function syncModelElementToDom(
  el: HTMLElement,
  inputEl: HTMLInputElement,
  currentVal: unknown,
  assertCurrent: () => void,
): void {
  if (el.tagName === 'INPUT' && inputEl.type === 'file') {
    // File selection is a native FileList, never a writable path string.
    // null clears; the native setter validates non-null FileList values.
    if (currentVal == null) {
      if (inputEl.value !== '') {
        assertCurrent();
        inputEl.value = '';
        domMutationState.count++;
        assertCurrent();
      }
    } else if (inputEl.files !== currentVal) {
      assertCurrent();
      inputEl.files = currentVal as FileList;
      domMutationState.count++;
      assertCurrent();
    }
  } else if (el.tagName === 'SELECT' && isMultipleSelect(el)) {
    const select = el as unknown as HTMLSelectElement;
    const values = new Set(Array.isArray(currentVal) ? currentVal.map(String) : []);
    assertCurrent();
    for (const option of Array.from(select.options)) {
      const nextSelected = values.has(option.value);
      assertCurrent();
      if (option.selected !== nextSelected) {
        assertCurrent();
        const nativeReflection = typeof select.multiple === 'boolean';
        assertCurrent();
        if (nativeReflection) option.selected = nextSelected;
        else {
          // Attribute-backed DOM hosts may omit multiple reflection and
          // implement option.selected as single-select regardless of parent.
          if (nextSelected) option.setAttribute('selected', '');
          else option.removeAttribute('selected');
        }
        domMutationState.count++;
        assertCurrent();
      }
    }
  } else if (el.tagName === 'SELECT') {
    synchronizeSelectValue(el as unknown as HTMLSelectElement, currentVal, assertCurrent);
  } else if (inputEl.type === 'checkbox' || inputEl.type === 'radio') {
    const nextChecked =
      inputEl.type === 'radio'
        ? currentVal != null && String(currentVal) === inputEl.value
        : Boolean(currentVal);
    assertCurrent();
    if (inputEl.checked !== nextChecked) {
      assertCurrent();
      inputEl.checked = nextChecked;
      domMutationState.count++;
      assertCurrent();
    }
  } else {
    const nextStr = currentVal == null ? '' : String(currentVal);
    assertCurrent();
    if (inputEl.value !== nextStr) {
      assertCurrent();
      inputEl.value = nextStr;
      domMutationState.count++;
      assertCurrent();
    }
  }
}

function readModelResetState(
  el: HTMLElement,
  inputEl: HTMLInputElement,
  isCurrent: () => boolean,
  clearComposing: () => void,
): { value: unknown; priority: number } | null {
  if (!isCurrent()) return null;
  clearComposing();
  let value: unknown,
    priority = 1;
  if (el.tagName === 'SELECT' && isMultipleSelect(el)) {
    value = selectedSelectValues(el);
  } else if (inputEl.type === 'radio') {
    const checked = inputEl.checked;
    value = checked ? inputEl.value : null;
    priority = checked ? 2 : 0;
  } else if (inputEl.type === 'checkbox') value = inputEl.checked;
  else if (el.tagName === 'INPUT' && inputEl.type === 'file') value = null;
  else if (inputEl.type === 'number' || inputEl.type === 'range')
    value = inputEl.value === '' ? 0 : Number(inputEl.value);
  else value = inputEl.value;
  return isCurrent() ? { value, priority } : null;
}

function attachSelectModelObserver(
  el: HTMLElement,
  inputEl: HTMLInputElement,
  core: ReturnType<typeof getWasmCore>,
  isCurrent: () => boolean,
  synchronizeIfCurrent: () => void,
  cleanup: () => void,
  assertCurrent: () => void,
): MutationObserver | undefined {
  const Observer = el.ownerDocument.defaultView?.MutationObserver ?? globalThis.MutationObserver;
  if (!Observer) return undefined;
  const synchronizeOptions = () => {
    if (!isCurrent()) {
      cleanup();
      return;
    }
    if (inputEl.form && pendingResetForms.has(inputEl.form)) {
      // A queued option mutation must not restore the pre-reset model
      // before the reset publication has sampled native defaults.
      queueMicrotask(() => withWasmCore(core, () => untrack(synchronizeOptions)));
      return;
    }
    synchronizeIfCurrent();
  };
  const observer = new Observer(() => withWasmCore(core, () => untrack(synchronizeOptions)));
  assertCurrent();
  observer.observe(el, {
    childList: true,
    subtree: true,
    characterData: true,
    attributes: true,
    attributeFilter: ['value', 'multiple'],
  });
  assertCurrent();
  return observer;
}

export function bindModel(el: HTMLElement, sig: WritableModelSignal): void {
  return runWithOwnerScope([], () => {
    const owner = getCurrentOwner()!,
      core = getWasmCore(),
      generation = getReactiveGeneration(core);
    const assertLifetime = captureViewLifetime();
    const inputEl = el as HTMLInputElement;
    let active = true,
      composing = false,
      observer: MutationObserver | undefined;
    let synchronizers: Set<() => void> | undefined;
    let releaseReset: (() => void) | undefined;
    const isCurrent = () =>
      active && !isOwnerScopeDisposed(owner) && generation === getReactiveGeneration(core);
    const assertCurrent = () => {
      assertLifetime();
      if (!active) throw new Error('Model binding disposed during construction');
    };
    const isTextModel = () => isTextModelControl(el, inputEl);
    const synchronize = () => {
      assertCurrent();
      const currentVal = sig();
      assertCurrent();
      if (composing && isTextModel()) return;
      syncModelElementToDom(el, inputEl, currentVal, assertCurrent);
    };
    const synchronizeIfCurrent = () => {
      if (isCurrent()) synchronize();
      else cleanup();
    };
    const commitValue = (next: unknown) => {
      if (isCurrent()) sig.set(next);
      else cleanup();
    };
    const updateFromDom = (event: Event) => {
      if (!isCurrent()) return cleanup();
      const isCheckbox = inputEl.type === 'checkbox',
        isRadio = inputEl.type === 'radio',
        isFile = el.tagName === 'INPUT' && inputEl.type === 'file';
      const eventName =
        isCheckbox || isRadio || isFile || el.tagName === 'SELECT' ? 'change' : 'input';
      if (
        event.type !== eventName ||
        (isTextModel() && (composing || (event as InputEvent).isComposing))
      )
        return;
      if (el.tagName === 'SELECT' && isMultipleSelect(el)) commitValue(selectedSelectValues(el));
      else if (isFile) commitValue(inputEl.files);
      else if (isCheckbox) commitValue(inputEl.checked);
      else if (isRadio) {
        if (inputEl.checked) commitValue(inputEl.value);
      } else if (inputEl.type === 'number' || inputEl.type === 'range')
        commitValue(inputEl.value === '' ? 0 : Number(inputEl.value));
      else commitValue(inputEl.value);
    };
    const listeners: Array<[string, EventListener]> = [
      ['compositionstart', () => (isCurrent() ? isTextModel() && (composing = true) : cleanup())],
      [
        'compositionend',
        () => {
          if (!isCurrent()) return cleanup();
          const wasComposing = composing;
          composing = false;
          if (wasComposing && isTextModel()) commitValue(inputEl.value);
        },
      ],
      ['input', updateFromDom],
      ['change', updateFromDom],
    ];
    function cleanup() {
      active = false;
      composing = false;
      releaseReset?.();
      try {
        observer?.disconnect();
      } catch {
        /* Complete the remaining cleanup. */
      }
      for (const [name, listener] of listeners) {
        try {
          el.removeEventListener(name, listener);
        } catch {
          /* Inactive callbacks cannot publish even if native removal fails. */
        }
      }
      synchronizers?.delete(synchronizeIfCurrent);
      if (synchronizers?.size === 0 && modelSynchronizers.get(el) === synchronizers)
        modelSynchronizers.delete(el);
    }
    // Register before initial synchronization or any native registration can reenter.
    onCleanup(cleanup);
    try {
      effect(synchronize);
      assertCurrent();
      synchronizers = modelSynchronizers.get(el) ?? new Set();
      modelSynchronizers.set(el, synchronizers);
      synchronizers.add(synchronizeIfCurrent);
      if (el.tagName === 'SELECT') {
        observer = attachSelectModelObserver(
          el,
          inputEl,
          core,
          isCurrent,
          synchronizeIfCurrent,
          cleanup,
          assertCurrent,
        );
      }
      for (const [name, listener] of listeners) {
        assertCurrent();
        el.addEventListener(name, listener);
        assertCurrent();
      }
      releaseReset = registerModelReset(el, {
        core,
        signal: sig,
        isCurrent,
        stop: cleanup,
        read: () => readModelResetState(el, inputEl, isCurrent, () => (composing = false)),
      });
      assertCurrent();
    } catch (failure) {
      // Cleanup can have run before a synchronous hook attaches its late listener.
      cleanup();
      throw failure;
    }
  });
}
