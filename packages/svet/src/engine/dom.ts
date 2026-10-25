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

import { createRoot, effect, onCleanup, ReadonlySignal, signal, Signal, untrack } from './reactivity';
import { computeLisWasm } from './wasm-bridge';

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

const registeredDelegatedEvents = new Set<string>();
const EVENT_PROP_PREFIX = '__svet_evt_';

function ensureDelegatedListener(eventName: string): void {
  if (typeof document === 'undefined' || registeredDelegatedEvents.has(eventName)) {
    return;
  }
  registeredDelegatedEvents.add(eventName);
  document.addEventListener(eventName, (event: Event) => {
    let node = event.target as (Node & Record<string, any>) | null;
    const propKey = EVENT_PROP_PREFIX + eventName;
    while (node) {
      const handler = node[propKey] as ((e: Event) => void) | undefined;
      if (typeof handler === 'function') {
        if ((node as HTMLButtonElement).disabled) return;
        handler.call(node, event);
        if (event.cancelBubble) break;
      }
      node = node.parentNode as (Node & Record<string, any>) | null;
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
    value(el);
    return;
  }

  // First-class two-way signal binding: `<input model={textSignal} />`
  if (key === 'model' && typeof value === 'function' && 'set' in value) {
    const sig = value as Signal<any>;
    const inputEl = el as HTMLInputElement;
    const isCheckbox = inputEl.type === 'checkbox';
    const isNumber = inputEl.type === 'number' || inputEl.type === 'range';

    effect(() => {
      const currentVal = sig();
      if (isCheckbox) {
        const nextChecked = Boolean(currentVal);
        if (inputEl.checked !== nextChecked) {
          inputEl.checked = nextChecked;
          domMutationCounter++;
        }
      } else {
        const nextStr = currentVal == null ? '' : String(currentVal);
        if (inputEl.value !== nextStr) {
          inputEl.value = nextStr;
          domMutationCounter++;
        }
      }
    });

    const eventName = isCheckbox || el.tagName === 'SELECT' ? 'change' : 'input';
    const updateFromDom = (e: Event) => {
      const target = e.target as HTMLInputElement;
      if (isCheckbox) {
        sig.set(target.checked);
      } else if (isNumber) {
        sig.set(target.value === '' ? 0 : Number(target.value));
      } else {
        sig.set(target.value);
      }
    };
    el.addEventListener(eventName, updateFromDom);
    return;
  }

  // Event listeners: `onClick`, `onInput`, etc. (with automatic Root Event Delegation)
  if (key.startsWith('on') && key.length > 2 && typeof value === 'function') {
    const eventName = key.slice(2).toLowerCase();
    if (DELEGATED_EVENTS.has(eventName)) {
      ensureDelegatedListener(eventName);
      const propKey = EVENT_PROP_PREFIX + eventName;
      const existing = (el as any)[propKey];
      if (typeof existing === 'function') {
        (el as any)[propKey] = (e: Event) => {
          existing(e);
          value(e);
        };
      } else {
        (el as any)[propKey] = value;
      }
    } else {
      const handler = value as EventListener;
      el.addEventListener(eventName, handler);
      onCleanup(() => el.removeEventListener(eventName, handler));
    }
    return;
  }

  // Style object or reactive style
  if (key === 'style') {
    if (isReactiveAccessor(value)) {
      effect(() => {
        const resolved = value();
        applyStyle(el, resolved);
      });
    } else {
      applyStyle(el, value);
    }
    return;
  }

  // Reactive attribute or DOM property
  if (isReactiveAccessor(value)) {
    effect(() => {
      const resolved = value();
      applyStaticAttribute(el, key, resolved);
    });
    return;
  }

  applyStaticAttribute(el, key, value);
}

function applyStyle(el: HTMLElement, styleVal: string | Record<string, any> | undefined): void {
  if (!styleVal) return;
  if (typeof styleVal === 'string') {
    if (el.style.cssText !== styleVal) {
      el.style.cssText = styleVal;
      domMutationCounter++;
    }
    return;
  }
  for (const [prop, rawVal] of Object.entries(styleVal)) {
    if (isReactiveAccessor(rawVal)) {
      effect(() => {
        const v = rawVal();
        setStyleProperty(el, prop, v);
      });
    } else {
      setStyleProperty(el, prop, rawVal);
    }
  }
}

function setStyleProperty(el: HTMLElement, prop: string, val: any): void {
  const str = val == null ? '' : String(val);
  if (prop.includes('-')) {
    if (el.style.getPropertyValue(prop) !== str) {
      el.style.setProperty(prop, str);
      domMutationCounter++;
    }
  } else if ((el.style as any)[prop] !== str) {
    (el.style as any)[prop] = str;
    domMutationCounter++;
  }
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

function applyStaticAttribute(el: HTMLElement, key: string, val: any): void {
  const attrName = key === 'className' ? 'class' : key;
  const lowerAttr = attrName.toLowerCase();

  // Block raw string/non-function inline event attributes (`onclick="alert(1)"`)
  if (lowerAttr.startsWith('on')) {
    return;
  }

  if (attrName === 'value' && 'value' in el) {
    const str = val == null ? '' : String(val);
    if ((el as HTMLInputElement).value !== str) {
      (el as HTMLInputElement).value = str;
      domMutationCounter++;
    }
    return;
  }

  if (attrName === 'checked' && 'checked' in el) {
    const bool = Boolean(val);
    if ((el as HTMLInputElement).checked !== bool) {
      (el as HTMLInputElement).checked = bool;
      domMutationCounter++;
    }
    return;
  }

  if (val === false || val === null || val === undefined) {
    if (el.hasAttribute(attrName)) {
      el.removeAttribute(attrName);
      domMutationCounter++;
    }
  } else if (val === true) {
    if (!el.hasAttribute(attrName)) {
      el.setAttribute(attrName, '');
      domMutationCounter++;
    }
  } else {
    const str = String(val);
    if (URL_ATTRIBUTES.has(lowerAttr) && isUnsafeUrl(str)) {
      return;
    }
    if (el.getAttribute(attrName) !== str) {
      el.setAttribute(attrName, str);
      domMutationCounter++;
    }
  }
}

/**
 * Appends a static or reactive child into a parent DOM node or DocumentFragment.
 */
export function insertChild(parent: Node, child: ChildNodeValue): void {
  if (child === null || child === undefined || typeof child === 'boolean') {
    return;
  }

  if (Array.isArray(child)) {
    for (let i = 0; i < child.length; i++) {
      insertChild(parent, child[i]);
    }
    return;
  }

  if (child instanceof Node) {
    parent.appendChild(child);
    domMutationCounter++;
    return;
  }

  if (isReactiveAccessor(child)) {
    // Evaluate initial value inside effect:
    let currentNode: Node | null = null;
    let textNodeRef: Text | null = null;
    let marker: Comment | null = null;
    let lastTextStr: string | null = null;

    effect(() => {
      const resolved = child();
      if (textNodeRef !== null) {
        if (typeof resolved === 'string') {
          if (lastTextStr !== resolved) {
            lastTextStr = resolved;
            textNodeRef.nodeValue = resolved;
            domMutationCounter++;
          }
          return;
        }
        if (typeof resolved === 'number') {
          const textStr = '' + resolved;
          if (lastTextStr !== textStr) {
            lastTextStr = textStr;
            textNodeRef.nodeValue = textStr;
            domMutationCounter++;
          }
          return;
        }
      }

      if (typeof resolved === 'string' || typeof resolved === 'number') {
        const textStr = typeof resolved === 'string' ? resolved : '' + resolved;
        lastTextStr = textStr;
        const textNode = document.createTextNode(textStr);
        if (currentNode && currentNode.parentNode) {
          currentNode.parentNode.replaceChild(textNode, currentNode);
        } else if (marker && marker.parentNode) {
          marker.parentNode.insertBefore(textNode, marker);
        } else {
          parent.appendChild(textNode);
        }
        currentNode = textNode;
        textNodeRef = textNode;
        domMutationCounter++;
        return;
      }

      textNodeRef = null;
      if (resolved instanceof Node) {
        lastTextStr = null;
        if (!marker) {
          marker = document.createComment('svet-dyn');
          if (currentNode && currentNode.parentNode) {
            currentNode.parentNode.insertBefore(marker, currentNode.nextSibling);
          } else {
            parent.appendChild(marker);
          }
        }
        if (currentNode !== resolved) {
          if (currentNode && currentNode.parentNode) {
            currentNode.parentNode.replaceChild(resolved, currentNode);
          } else if (marker.parentNode) {
            marker.parentNode.insertBefore(resolved, marker);
          }
          currentNode = resolved;
          domMutationCounter++;
        }
      } else {
        const textStr =
          resolved === null || resolved === undefined || typeof resolved === 'boolean'
            ? ''
            : String(resolved);
        lastTextStr = textStr;
        const textNode = document.createTextNode(textStr);
        if (currentNode && currentNode.parentNode) {
          currentNode.parentNode.replaceChild(textNode, currentNode);
        } else if (marker && marker.parentNode) {
          marker.parentNode.insertBefore(textNode, marker);
        } else {
          parent.appendChild(textNode);
        }
        currentNode = textNode;
        textNodeRef = textNode;
        domMutationCounter++;
      }
    });
    return;
  }

  const textNode = document.createTextNode(String(child));
  parent.appendChild(textNode);
  domMutationCounter++;
}

const SVG_NAMESPACE = 'http://www.w3.org/2000/svg';
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
]);

/**
 * Core hyperscript / JSX factory that builds real DOM & SVG nodes directly (Zero VDOM).
 */
export function h(
  type: string | Component<any>,
  props: Record<string, any> | null,
  ...children: ChildNodeValue[]
): Node {
  const normalizedProps = props ? { ...props } : {};
  if (children.length > 0) {
    normalizedProps.children = children.length === 1 ? children[0] : children;
  }

  if (typeof type === 'function') {
    return untrack(() => type(normalizedProps));
  }

  const el = SVG_TAGS.has(type)
    ? (document.createElementNS(SVG_NAMESPACE, type) as unknown as HTMLElement)
    : document.createElement(type);
  // Ensure `type` attribute is applied BEFORE `model` or `value` so `<input type="checkbox" model={...}>` knows its type
  if ('type' in normalizedProps) {
    applyStaticAttribute(el, 'type', normalizedProps.type);
  }
  for (const [k, v] of Object.entries(normalizedProps)) {
    if (k !== 'children' && k !== 'type') {
      bindProp(el, k, v);
    }
  }
  if (normalizedProps.children !== undefined) {
    insertChild(el, normalizedProps.children);
  }
  return el;
}

export function Fragment(props: { children?: ChildNodeValue }): DocumentFragment {
  const frag = document.createDocumentFragment();
  if (props.children !== undefined) {
    insertChild(frag, props.children);
  }
  return frag;
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
  const frag = document.createDocumentFragment();
  const anchor = document.createComment('svet-show');
  frag.appendChild(anchor);

  let currentNodes: Node[] = [];
  let disposeBranchScope: (() => void) | null = null;
  let activeState: 'none' | 'truthy' | 'falsy' = 'none';
  let lastTruthyVal: any = undefined;
  const isCallbackChild = typeof props.children === 'function' && props.children.length > 0;

  const clearCurrent = () => {
    if (disposeBranchScope) {
      disposeBranchScope();
      disposeBranchScope = null;
    }
    for (const n of currentNodes) {
      if (n.parentNode) {
        n.parentNode.removeChild(n);
        domMutationCounter++;
      }
    }
    currentNodes = [];
  };

  const mountBranch = (factory: () => any) => {
    createRoot((dispose) => {
      disposeBranchScope = dispose;
      const branchContent = untrack(factory);
      const tempFrag = document.createDocumentFragment();
      insertChild(tempFrag, branchContent);
      const nodes = Array.from(tempFrag.childNodes);
      if (anchor.parentNode) {
        for (const n of nodes) {
          anchor.parentNode.insertBefore(n, anchor);
          domMutationCounter++;
        }
      } else {
        for (const n of nodes) {
          frag.insertBefore(n, anchor);
          domMutationCounter++;
        }
      }
      currentNodes = nodes;
    });
  };

  effect(() => {
    const cond = isReactiveAccessor(props.when) ? props.when() : props.when;
    const isTruthy = Boolean(cond);

    if (isTruthy) {
      if (activeState === 'truthy' && (!isCallbackChild || Object.is(lastTruthyVal, cond))) {
        return;
      }
      activeState = 'truthy';
      lastTruthyVal = cond;
      clearCurrent();
      mountBranch(() =>
        typeof props.children === 'function'
          ? (props.children as (val: NonNullable<T>) => Node)(cond as NonNullable<T>)
          : props.children
      );
    } else {
      if (activeState === 'falsy') {
        return;
      }
      activeState = 'falsy';
      lastTruthyVal = undefined;
      clearCurrent();
      if (props.fallback !== undefined) {
        mountBranch(() =>
          typeof props.fallback === 'function'
            ? (props.fallback as () => Node)()
            : props.fallback
        );
      }
    }
  });

  onCleanup(() => {
    activeState = 'none';
    clearCurrent();
  });

  return frag;
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
  children: (item: T, index: () => number) => Node;
}): DocumentFragment {
  const frag = document.createDocumentFragment();
  const endMarker = document.createComment('svet-for-end');
  frag.appendChild(endMarker);

  type MappedRecord = {
    key: K;
    node: Node;
    index: number;
    indexSig?: Signal<number>;
    disposeScope: () => void;
  };

  let currentRecords: MappedRecord[] = [];
  let recordMap = new Map<K, MappedRecord>();
  // Fast integer-indexed lookup table when keys are non-negative SMI integers (`0 <= key < 65536`)
  let intKeyTable: Array<MappedRecord | undefined> = [];
  let scratchIndices = new Uint32Array(64);
  let scratchRecords: MappedRecord[] = [];

  // If the enclosing scope of <For> is disposed, dispose all active row scopes
  onCleanup(() => {
    for (const rec of currentRecords) {
      rec.disposeScope();
    }
    currentRecords = [];
    scratchRecords = [];
    recordMap.clear();
    intKeyTable.length = 0;
  });

  effect(() => {
    const list = isReactiveAccessor(props.each) ? props.each() : props.each;
    const parent = endMarker.parentNode ?? frag;

    const getKey = props.keyFn ?? ((item: T) => item as unknown as K);
    const nextLen = list.length;

    if (nextLen === 0) {
      for (const rec of currentRecords) {
        rec.disposeScope();
        if (rec.node.parentNode) {
          rec.node.parentNode.removeChild(rec.node);
          domMutationCounter++;
        }
      }
      currentRecords = [];
      recordMap.clear();
      intKeyTable.length = 0;
      return;
    }

    const oldLen = currentRecords.length;
    if (scratchIndices.length < nextLen) {
      scratchIndices = new Uint32Array(Math.max(nextLen, 64));
    }
    const oldIndices = scratchIndices;
    let nextRecords = scratchRecords;
    if (nextRecords.length !== nextLen) {
      nextRecords = new Array(nextLen);
    }

    let anyMovedOrNew = false;
    let prevOldIdx = -1;
    let displacedCount = 0;
    let firstDisplacedIdx = -1;
    let secondDisplacedIdx = -1;
    let reusedCount = 0;

    const usesReactiveIndex = props.children.length >= 2;

    for (let i = 0; i < nextLen; i++) {
      const item = list[i];
      const key = getKey(item, i);
      const isIntKey = typeof key === 'number' && (key as number) >= 0 && (key as number) < 65536 && ((key as number) | 0) === (key as number);
      const existing = isIntKey ? intKeyTable[key as number] : recordMap.get(key);

      if (existing) {
        reusedCount++;
        const oldIdx = existing.index;
        oldIndices[i] = oldIdx;
        if (oldIdx < prevOldIdx) {
          anyMovedOrNew = true;
          const prevWasJump =
            i >= 2 ? oldIndices[i - 1] > oldIdx && oldIdx > oldIndices[i - 2] : oldIndices[0] > oldIdx && i + 1 < nextLen;
          const displacedPos = prevWasJump ? i - 1 : i;
          displacedCount++;
          if (displacedCount === 1) firstDisplacedIdx = displacedPos;
          else if (displacedCount === 2) secondDisplacedIdx = displacedPos;
          if (prevWasJump) {
            prevOldIdx = oldIdx;
          }
        } else {
          prevOldIdx = oldIdx;
        }
        existing.index = i;
        if (existing.indexSig) {
          existing.indexSig.set(i);
        }
        nextRecords[i] = existing;
      } else {
        oldIndices[i] = NEW_ITEM_MARKER;
        anyMovedOrNew = true;
        displacedCount = 999; // force full LIS pass when new items are inserted
        let rowDispose!: () => void;
        const newRec: MappedRecord = {
          key,
          node: null as unknown as Node,
          index: i,
          disposeScope: () => {},
        };
        const getReactiveIndex = usesReactiveIndex
          ? (): number => {
              if (!newRec.indexSig) {
                newRec.indexSig = signal(newRec.index);
              }
              return newRec.indexSig();
            }
          : (() => i);
        const createdNode = untrack(() =>
          createRoot((dispose) => {
            rowDispose = dispose;
            return props.children(item, getReactiveIndex);
          })
        );
        newRec.node = createdNode;
        newRec.disposeScope = rowDispose;
        nextRecords[i] = newRec;
        if (isIntKey) {
          intKeyTable[key as number] = newRec;
        } else {
          recordMap.set(key, newRec);
        }
      }
    }

    // Now that all new and reused records are finalized, remove only the duplicate loop
    // If any old items were not reused, remove their DOM nodes, dispose their scopes, and delete from lookup tables
    if (reusedCount < oldLen) {
      const nextSet = new Set<MappedRecord>(nextRecords);
      for (let i = 0; i < oldLen; i++) {
        const oldRec = currentRecords[i];
        if (!nextSet.has(oldRec)) {
          oldRec.disposeScope();
          const k = oldRec.key;
          if (typeof k === 'number' && k >= 0 && k < 65536 && (k | 0) === k) {
            intKeyTable[k] = undefined;
          } else {
            recordMap.delete(k);
          }
          if (oldRec.node.parentNode) {
            oldRec.node.parentNode.removeChild(oldRec.node);
            domMutationCounter++;
          }
        }
      }
    }

    if (anyMovedOrNew) {
      if (displacedCount > 0 && displacedCount <= 2 && reusedCount === nextLen && oldLen === nextLen) {
        // Fast path: 1 or 2 items moved forward in an existing list! Move only those exact nodes in O(1)!
        const moveIdx = (idx: number) => {
          const rec = nextRecords[idx];
          const anchor = idx + 1 < nextLen ? nextRecords[idx + 1].node : endMarker;
          parent.insertBefore(rec.node, anchor);
          domMutationCounter++;
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
            parent.insertBefore(rec.node, anchorNode);
            domMutationCounter++;
          }
          anchorNode = rec.node;
        }
      }
    }

    scratchRecords = currentRecords;
    currentRecords = nextRecords;
  });

  return frag;
}

/**
 * Mounts a Svet component or DOM subtree into a target container inside a root `OwnerScope`.
 * Calling `unmount()` disposes the entire reactive tree and recycles all WASM node slots!
 */
export function mount(view: () => Node, container: HTMLElement): { unmount(): void } {
  container.innerHTML = '';
  let disposeRoot!: () => void;
  const node = createRoot((dispose) => {
    disposeRoot = dispose;
    return view();
  });
  container.appendChild(node);
  return {
    unmount() {
      disposeRoot();
      container.innerHTML = '';
    },
  };
}
