/**
 * Attribute, style, and `<select>` synchronization helpers for Svet DOM bindings.
 */

import {
  captureViewLifetime,
  domMutationState,
  isReactiveAccessor,
  isUnsafeUrl,
} from './dom-events.ts';
import {
  effect,
  getCurrentOwner,
  getReactiveGeneration,
  isOwnerScopeDisposed,
  onCleanup,
  runWithCapturedOwner,
  runWithOwnerScope,
  untrack,
} from './reactivity.ts';
import { getWasmCore } from './wasm-bridge.ts';

export const modelSynchronizers = new WeakMap<HTMLElement, Set<() => void>>();

export function isMultipleSelect(el: HTMLElement): boolean {
  const reflected = (el as unknown as HTMLSelectElement).multiple;
  return typeof reflected === 'boolean' ? reflected : el.hasAttribute('multiple');
}

export function selectedSelectValues(el: HTMLElement): string[] {
  return Array.from((el as unknown as HTMLSelectElement).options)
    .filter(option => option.selected)
    .map(option => option.value);
}

export function synchronizeSelectValue(
  select: HTMLSelectElement,
  value: unknown,
  assertCurrent: () => void,
): void {
  assertCurrent();
  const next = value == null ? '' : String(value);
  assertCurrent();
  const options = Array.from(select.options);
  assertCurrent();
  let selected = -1;
  for (let index = 0; index < options.length; index++) {
    assertCurrent();
    if (selected < 0 && options[index]!.value === next) selected = index;
    assertCurrent();
  }
  // Avoid rewriting an already coherent selection (including default state).
  let coherent = true;
  for (let index = 0; index < options.length; index++) {
    assertCurrent();
    if (options[index]!.selected !== (index === selected)) coherent = false;
    assertCurrent();
  }
  if (coherent) return;
  // Clear first: some DOM hosts enforce single selection even when deselecting.
  for (const option of options) {
    assertCurrent();
    if (option.selected) {
      option.selected = false;
      domMutationState.count++;
      assertCurrent();
    }
  }
  if (selected >= 0) {
    assertCurrent();
    options[selected]!.selected = true;
    domMutationState.count++;
    assertCurrent();
  } else if (select.selectedIndex !== -1) {
    assertCurrent();
    select.selectedIndex = -1;
    domMutationState.count++;
    assertCurrent();
  }
}

function attachSelectValueObserver(
  el: HTMLElement,
  readCachedValue: () => string,
  assertBindingCurrent: () => void,
): void {
  const core = getWasmCore(),
    owner = getCurrentOwner(),
    generation = getReactiveGeneration(core);
  const synchronizers = modelSynchronizers.get(el) ?? new Set<() => void>();
  let active = true,
    observer: MutationObserver | undefined;
  const refresh = () => {
    if (!active || isOwnerScopeDisposed(owner) || generation !== getReactiveGeneration(core))
      return;
    runWithCapturedOwner(core, owner, () =>
      untrack(() => {
        assertBindingCurrent();
        synchronizeSelectValue(
          el as unknown as HTMLSelectElement,
          readCachedValue(),
          assertBindingCurrent,
        );
      }),
    );
  };
  const cleanup = () => {
    active = false;
    try {
      observer?.disconnect();
    } catch {
      /* Still unlink selection refresh. */
    }
    synchronizers.delete(refresh);
    if (!synchronizers.size && modelSynchronizers.get(el) === synchronizers)
      modelSynchronizers.delete(el);
  };
  modelSynchronizers.set(el, synchronizers);
  synchronizers.add(refresh);
  onCleanup(cleanup);
  try {
    const Observer = el.ownerDocument.defaultView?.MutationObserver ?? globalThis.MutationObserver;
    assertBindingCurrent();
    if (Observer) {
      observer = new Observer(refresh);
      assertBindingCurrent();
      observer.observe(el, {
        childList: true,
        subtree: true,
        characterData: true,
        attributes: true,
        attributeFilter: ['value'],
      });
      assertBindingCurrent();
    }
  } catch (error) {
    cleanup();
    throw error;
  }
}

export function bindAttribute(
  el: HTMLElement,
  key: string,
  value: unknown,
  notifyModel: boolean,
): void {
  const assertBindingCurrent = captureViewLifetime();
  assertBindingCurrent();
  const isSelectValue = el.tagName === 'SELECT' && key === 'value';
  let cachedSelectValue = '';
  const apply = () => {
    const assertRunCurrent = captureViewLifetime();
    const assertCurrent = () => {
      assertBindingCurrent();
      assertRunCurrent();
    };
    assertCurrent();
    const resolved = isReactiveAccessor(value) ? value() : value;
    assertCurrent();
    const attributeValue = isSelectValue ? (resolved == null ? '' : String(resolved)) : resolved;
    assertCurrent();
    if (isSelectValue) cachedSelectValue = attributeValue as string;
    if (notifyModel) applyAttributeValue(el, key, attributeValue, assertCurrent);
    else applyStaticAttribute(el, key, attributeValue, assertCurrent);
  };
  if (isSelectValue) {
    attachSelectValueObserver(el, () => cachedSelectValue, assertBindingCurrent);
  }
  if (isReactiveAccessor(value)) {
    runWithOwnerScope([], () => effect(() => runWithOwnerScope([], apply)));
  } else if (typeof value === 'object' && value !== null) {
    // User coercion can create provisional subscriptions before failing.
    runWithOwnerScope([], apply);
  } else apply();
}

const managedStyles = new WeakMap<HTMLElement, Set<string>>();
const cssPropertyCache = new Map<string, string>();

function cssPropertyName(name: string): string {
  if (name.startsWith('--')) return name;
  let cached = cssPropertyCache.get(name);
  if (cached !== undefined) return cached;
  cached =
    name === 'cssFloat'
      ? 'float'
      : name.replace(/[A-Z]/g, char => `-${char.toLowerCase()}`).replace(/^ms-/, '-ms-');
  if (cssPropertyCache.size < 1024) cssPropertyCache.set(name, cached);
  return cached;
}

export function bindStyleProp(el: HTMLElement, value: unknown): void {
  runWithOwnerScope([], () => {
    const assertBindingCurrent = captureViewLifetime();
    const apply = () => {
      const assertRunCurrent = captureViewLifetime();
      const assertCurrent = () => {
        assertBindingCurrent();
        assertRunCurrent();
      };
      assertCurrent();
      const resolved = isReactiveAccessor(value) ? value() : value;
      assertCurrent();
      applyStyle(el, resolved as string | Record<string, unknown> | undefined, assertCurrent);
    };
    if (isReactiveAccessor(value)) effect(() => runWithOwnerScope([], apply));
    else apply();
  });
}

export function applyStyle(
  el: HTMLElement,
  styleVal: string | Record<string, unknown> | undefined,
  assertCurrent: () => void,
): void {
  assertCurrent();
  if (typeof styleVal === 'string') {
    if (el.style.cssText !== styleVal) {
      assertCurrent();
      el.style.cssText = styleVal;
      domMutationState.count++;
      assertCurrent();
    }
    const properties = new Set(
      Array.from({ length: el.style.length }, (_, i) => {
        assertCurrent();
        const name = el.style.item(i);
        assertCurrent();
        return name;
      }),
    );
    assertCurrent();
    managedStyles.set(el, properties);
    return;
  }
  const keys = styleVal ? Object.keys(styleVal) : [];
  assertCurrent();
  const entries: Array<[string, unknown]> = new Array(keys.length);
  const nextProperties = new Set<string>();
  for (let i = 0; i < keys.length; i++) {
    const prop = keys[i]!;
    assertCurrent();
    const raw = (styleVal as Record<string, unknown>)[prop];
    assertCurrent();
    entries[i] = [prop, raw];
    nextProperties.add(cssPropertyName(prop));
  }
  for (const prop of managedStyles.get(el) ?? []) {
    assertCurrent();
    if (!nextProperties.has(prop) && el.style.getPropertyValue(prop)) {
      assertCurrent();
      el.style.removeProperty(prop);
      domMutationState.count++;
      assertCurrent();
    }
  }
  assertCurrent();
  managedStyles.set(el, nextProperties);
  for (const [prop, rawVal] of entries) {
    assertCurrent();
    if (isReactiveAccessor(rawVal)) {
      effect(() => {
        const assertPropertyCurrent = captureViewLifetime();
        const assertValueCurrent = () => {
          assertCurrent();
          assertPropertyCurrent();
        };
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

export function setStyleProperty(
  el: HTMLElement,
  prop: string,
  val: unknown,
  assertCurrent: () => void,
): void {
  assertCurrent();
  const str = val == null ? '' : String(val);
  assertCurrent();
  const styleRecord = el.style as unknown as Record<string, string>;
  if (prop.includes('-')) {
    if (el.style.getPropertyValue(prop) !== str) {
      assertCurrent();
      el.style.setProperty(prop, str);
      domMutationState.count++;
      assertCurrent();
    }
  } else if (styleRecord[prop] !== str) {
    assertCurrent();
    styleRecord[prop] = str;
    domMutationState.count++;
    assertCurrent();
  }
  assertCurrent();
}

const URL_ATTRIBUTES = new Set(['href', 'src', 'xlink:href', 'action', 'formaction']);

const XLINK_NAMESPACE = 'http://www.w3.org/1999/xlink';
const BOOLEAN_TOKEN_ATTRIBUTES = new Set(['contenteditable', 'draggable', 'spellcheck']);

/** @internal Applies an attribute value without interpreting model/ref/event props. */
export function applyAttributeValue(
  el: HTMLElement,
  key: string,
  value: unknown,
  assertCurrent = captureViewLifetime(),
): void {
  assertCurrent();
  applyStaticAttribute(el, key, value, assertCurrent);
  assertCurrent();
  const name = key.toLowerCase();
  // Type and radio-option changes can affect checked without changing the model.
  if (name === 'type' || (name === 'value' && (el as HTMLInputElement).type === 'radio')) {
    untrack(() => {
      for (const synchronize of modelSynchronizers.get(el) ?? []) {
        assertCurrent();
        synchronize();
        assertCurrent();
      }
    });
  }
  assertCurrent();
}

export function applyStaticAttribute(
  el: HTMLElement,
  key: string,
  val: unknown,
  assertCurrent = captureViewLifetime(),
): void {
  assertCurrent();
  const attrName = key === 'className' ? 'class' : key === 'xlinkHref' ? 'xlink:href' : key;
  const lowerAttr = attrName.toLowerCase();
  if (typeof val === 'boolean') {
    if (lowerAttr === 'translate') val = val ? 'yes' : 'no';
    else if (
      BOOLEAN_TOKEN_ATTRIBUTES.has(lowerAttr) ||
      lowerAttr.startsWith('aria-') ||
      lowerAttr.startsWith('data-')
    )
      val = String(val);
  }
  // Raw string/non-function inline event attributes remain blocked.
  if (lowerAttr.startsWith('on')) return;
  if (attrName === 'value' && el.tagName === 'SELECT') {
    synchronizeSelectValue(el as unknown as HTMLSelectElement, val, assertCurrent);
    return;
  }
  if (attrName === 'value' && 'value' in el) {
    assertCurrent();
    const str = val == null ? '' : String(val);
    assertCurrent();
    if ((el as HTMLInputElement).value !== str) {
      assertCurrent();
      (el as HTMLInputElement).value = str;
      domMutationState.count++;
      assertCurrent();
    }
    assertCurrent();
    return;
  }
  if (attrName === 'checked' && 'checked' in el) {
    const bool = Boolean(val);
    if ((el as HTMLInputElement).checked !== bool) {
      assertCurrent();
      (el as HTMLInputElement).checked = bool;
      domMutationState.count++;
      assertCurrent();
    }
    assertCurrent();
    return;
  }
  if (val === false || val === null || val === undefined) {
    if (attrName === 'xlink:href' && typeof el.removeAttributeNS === 'function') {
      assertCurrent();
      el.removeAttributeNS(XLINK_NAMESPACE, 'href');
      domMutationState.count++;
      assertCurrent();
    } else if (el.hasAttribute(attrName)) {
      assertCurrent();
      el.removeAttribute(attrName);
      domMutationState.count++;
      assertCurrent();
    }
  } else if (val === true) {
    if (!el.hasAttribute(attrName)) {
      assertCurrent();
      el.setAttribute(attrName, '');
      domMutationState.count++;
      assertCurrent();
    }
  } else {
    const str = String(val);
    assertCurrent();
    if ((URL_ATTRIBUTES.has(lowerAttr) || lowerAttr === 'xlink:href') && isUnsafeUrl(str)) return;
    if (attrName === 'xlink:href' && typeof el.setAttributeNS === 'function') {
      if (el.getAttributeNS(XLINK_NAMESPACE, 'href') !== str) {
        assertCurrent();
        el.setAttributeNS(XLINK_NAMESPACE, 'xlink:href', str);
        domMutationState.count++;
        assertCurrent();
      }
      assertCurrent();
      return;
    }
    if (el.getAttribute(attrName) !== str) {
      assertCurrent();
      el.setAttribute(attrName, str);
      domMutationState.count++;
      assertCurrent();
    }
  }
  assertCurrent();
}
