/**
 * Core DOM element construction, JSX factories, dynamic child reconciliation, and
 * owned branch mounting for Svet.
 */

import { bindAttribute, bindStyleProp, modelSynchronizers } from './dom-attributes.ts';
import {
  type AsyncComponent,
  bindEventProp,
  captureViewLifetime,
  type Component,
  EVENT_PROP_PREFIX,
  type ChildNodeValue,
  domMutationState,
  isDomNode,
  isReactiveAccessor,
  withDocumentAdoption,
} from './dom-events.ts';
import { bindModel, retainOwnedResetRoot, type WritableModelSignal } from './dom-model.ts';
import {
  adoptCurrentOwnerIntoRoot,
  createRoot,
  currentOwnerIsEmpty,
  effect,
  getCurrentOwner,
  onCleanup,
  runWithOwnerScope,
  untrack,
} from './reactivity.ts';

type UntypedComponentFn = (props: Record<string | symbol, unknown>) => Node | PromiseLike<Node>;

type AsyncComponentRenderer = (
  component: UntypedComponentFn,
  props: Record<string | symbol, unknown>,
) => Node | undefined;

let asyncComponentRenderer: AsyncComponentRenderer | undefined;

/** @internal Installed by the Node SSR entry; inactive outside an async render request. */
export function installAsyncComponentRenderer(renderer: AsyncComponentRenderer): void {
  asyncComponentRenderer = renderer;
}

/**
 * Applies a static or reactive prop/attribute/event/model to a real DOM Element.
 */
export function bindProp(el: HTMLElement, key: string, value: unknown): void {
  if (key === 'children' || value === undefined || value === null) {
    return;
  }

  if (key === 'ref' && typeof value === 'function') {
    const assertCallerCurrent = captureViewLifetime();
    assertCallerCurrent();
    return runWithOwnerScope([], () => {
      const assertRefCurrent = captureViewLifetime();
      assertCallerCurrent();
      assertRefCurrent();
      (value as (node: HTMLElement) => void)(el);
      assertCallerCurrent();
      assertRefCurrent();
    });
  }

  // First-class two-way signal binding: `<input model={textSignal} />`
  if (key === 'model' && typeof value === 'function' && 'set' in value) {
    bindModel(el, value as unknown as WritableModelSignal);
    return;
  }

  if (key === 'type' || key === 'value') {
    bindAttribute(el, key, value, true);
    return;
  }

  // Event listeners: `onClick`, `onInput`, etc. (with automatic Root Event Delegation)
  if (key.startsWith('on') && key.length > 2 && typeof value === 'function') {
    bindEventProp(
      el,
      key.slice(2).toLowerCase(),
      value as (this: HTMLElement, event: Event) => unknown,
    );
    return;
  }

  // Style construction owns all provisional nested property subscriptions.
  if (key === 'style') {
    bindStyleProp(el, value);
    return;
  }

  bindAttribute(el, key, value, false);
}

function flattenChildValues(
  values: readonly ChildNodeValue[],
  cycleMessage: string,
): ChildNodeValue[] {
  const flattened: ChildNodeValue[] = [];
  const active = new Set<readonly ChildNodeValue[]>();
  const stack: Array<{ value: ChildNodeValue; exit?: boolean }> = [
    { value: values as ChildNodeValue[] },
  ];
  while (stack.length) {
    const frame = stack.pop()!,
      value = frame.value;
    if (Array.isArray(value)) {
      if (frame.exit) {
        active.delete(value);
        continue;
      }
      if (active.has(value)) throw new Error(cycleMessage);
      active.add(value);
      stack.push({ value, exit: true });
      const snapshot = Array.from(value);
      for (let i = snapshot.length - 1; i >= 0; i--) stack.push({ value: snapshot[i] });
    } else if (value != null && typeof value !== 'boolean') flattened.push(value);
  }
  return flattened;
}

function dynamicArrayNodes(
  values: readonly ChildNodeValue[],
  doc: Document,
  assertCurrent: () => void,
): Node[] {
  const flattened = flattenChildValues(values, 'Cyclic dynamic child array');
  const nodes: Node[] = [],
    seen = new Set<Node>();
  const add = (node: Node) => {
    if (seen.has(node)) throw new Error('Duplicate dynamic child node');
    seen.add(node);
    nodes.push(node);
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

type DynamicChildState = {
  currentNode: Node | null;
  textNodeRef: Text | null;
  marker: Comment | null;
  lastTextStr: string | null;
  currentFragment: Node | null;
  grouped: boolean;
  disposeContent: (() => void) | undefined;
};

type DynamicNodeOrigin = {
  node: Node;
  parent: Node | null;
  next: Node | null;
};

function restoreFailedDynamicInsert(
  oldNodes: Node[],
  nextNodes: Node[],
  origins: DynamicNodeOrigin[],
  fragment: Node | null,
  target: Node,
  nextMarker: Comment | null,
  originalAnchor: Node | null,
  needsMarker: boolean,
  assertCurrent: () => void,
): void {
  let alive = true;
  try {
    assertCurrent();
  } catch {
    alive = false;
  }
  // Include nodes attached after a synchronous hook invalidated the owner.
  for (const node of nextNodes) {
    try {
      if (node.parentNode) {
        node.parentNode.removeChild(node);
        domMutationState.count++;
      }
    } catch {
      /* Best effort. */
    }
  }
  const restore = (action: () => void) => {
    if (!alive) return;
    try {
      assertCurrent();
      action();
      assertCurrent();
    } catch {
      try {
        assertCurrent();
      } catch {
        alive = false;
      }
    }
  };
  if (alive) {
    // Restore candidate origins (including a consumed fragment) for retry.
    if (fragment) {
      for (const node of nextNodes.slice(1))
        restore(() => {
          withDocumentAdoption(
            fragment,
            node,
            () => {
              fragment.appendChild(node);
            },
            assertCurrent,
          );
        });
    } else {
      for (const origin of origins.slice().reverse()) {
        restore(() => {
          if (origin.parent)
            withDocumentAdoption(
              origin.parent,
              origin.node,
              () => {
                origin.parent!.insertBefore(
                  origin.node,
                  origin.next?.parentNode === origin.parent ? origin.next : null,
                );
              },
              assertCurrent,
            );
        });
      }
    }
    const anchor =
      nextMarker?.parentNode === target
        ? nextMarker
        : originalAnchor?.parentNode === target
          ? originalAnchor
          : null;
    for (const node of oldNodes)
      restore(() => {
        withDocumentAdoption(
          target,
          node,
          () => {
            target.insertBefore(node, anchor);
          },
          assertCurrent,
        );
        domMutationState.count++;
      });
  }
  if (needsMarker) {
    try {
      nextMarker?.parentNode?.removeChild(nextMarker);
    } catch {
      /* Best effort. */
    }
  }
  try {
    assertCurrent();
  } catch {
    alive = false;
  }
  if (!alive) {
    for (const node of [...oldNodes, ...nextNodes]) {
      try {
        node.parentNode?.removeChild(node);
      } catch {
        /* Never revive invalidated content. */
      }
    }
  }
}

function renderDynamicChildContent(
  state: DynamicChildState,
  parent: Node,
  child: () => unknown,
  doc: Document,
  assertParentCurrent: () => void,
  disposeCandidate: () => void,
  preset?: { value: unknown },
): void {
  const publishContent = () => {
    const previous = state.disposeContent;
    state.disposeContent = disposeCandidate;
    previous?.();
    assertCurrent();
  };
  const assertEffectCurrent = captureViewLifetime();
  const assertCurrent = () => {
    assertParentCurrent();
    assertEffectCurrent();
  };
  assertCurrent();
  const resolved = preset ? preset.value : child();
  assertCurrent();
  if (state.currentFragment && resolved === state.currentFragment) {
    disposeCandidate();
    assertParentCurrent();
    return;
  }
  const nativeNode = isDomNode(resolved),
    array = Array.isArray(resolved);
  const fragment = nativeNode && resolved.nodeType === 11 ? resolved : null;
  const text =
    nativeNode || array
      ? null
      : resolved == null || typeof resolved === 'boolean'
        ? ''
        : String(resolved);
  assertCurrent();
  if (state.textNodeRef && (typeof resolved === 'string' || typeof resolved === 'number')) {
    if (state.lastTextStr !== text) {
      state.textNodeRef.nodeValue = text;
      domMutationState.count++;
      assertCurrent();
      state.lastTextStr = text;
    }
    publishContent();
    return;
  }
  if (!state.grouped && nativeNode && state.currentNode === resolved) {
    disposeCandidate();
    assertParentCurrent();
    return;
  }

  const oldNodes: Node[] = [];
  if (state.grouped) {
    let node = state.currentNode;
    while (node && node !== state.marker) {
      oldNodes.push(node);
      node = node.nextSibling;
    }
    if (!node || node !== state.marker) throw new Error('Broken dynamic fragment range');
  } else if (state.currentNode) oldNodes.push(state.currentNode);
  if (state.marker && !state.marker.parentNode) throw new Error('Broken dynamic child anchor');
  const target = state.marker?.parentNode ?? state.currentNode?.parentNode ?? parent;
  const originalAnchor = state.marker ?? state.currentNode?.nextSibling ?? null;
  const groupNodes = array
    ? dynamicArrayNodes(resolved, doc, assertCurrent)
    : fragment
      ? Array.from(fragment.childNodes)
      : null;
  const nextNode = groupNodes
    ? doc.createComment('svet-dyn-start')
    : nativeNode
      ? resolved
      : doc.createTextNode(text!);
  const nextNodes = groupNodes ? [nextNode, ...groupNodes] : [nextNode];
  const origins = nextNodes.map(node => ({
    node,
    parent: node.parentNode,
    next: node.nextSibling,
  }));
  const nextSet = new Set(nextNodes);
  let nextMarker = state.marker;
  const needsMarker = (nativeNode || array) && !nextMarker;
  if (needsMarker) nextMarker = doc.createComment('svet-dyn');
  const insert = (node: Node, anchor: Node | null) => {
    assertCurrent();
    withDocumentAdoption(
      target,
      node,
      () => {
        target.insertBefore(node, anchor);
      },
      assertCurrent,
    );
    domMutationState.count++;
    assertCurrent();
  };
  try {
    if (needsMarker) insert(nextMarker!, originalAnchor);
    const anchor = nextMarker ?? originalAnchor;
    for (const node of nextNodes) insert(node, anchor);
    for (const node of oldNodes) {
      if (!nextSet.has(node) && node.parentNode) {
        assertCurrent();
        node.parentNode.removeChild(node);
        domMutationState.count++;
        assertCurrent();
      }
    }
    assertCurrent();
    state.currentNode = nextNode;
    state.currentFragment = fragment;
    state.grouped = groupNodes !== null;
    state.marker = nextMarker;
    state.textNodeRef = nativeNode || array ? null : (nextNode as Text);
    state.lastTextStr = text;
  } catch (failure) {
    restoreFailedDynamicInsert(
      oldNodes,
      nextNodes,
      origins,
      fragment,
      target,
      nextMarker,
      originalAnchor,
      needsMarker,
      assertCurrent,
    );
    throw failure;
  }
  publishContent();
}

function insertReactiveChild(
  parent: Node,
  child: () => unknown,
  assertParentCurrent: () => void,
): void {
  const state: DynamicChildState = {
    currentNode: null,
    textNodeRef: null,
    marker: null,
    lastTextStr: null,
    currentFragment: null,
    grouped: false,
    disposeContent: undefined,
  };
  const doc = parent.ownerDocument ?? document;
  onCleanup(() => {
    const dispose = state.disposeContent;
    state.disposeContent = undefined;
    dispose?.();
  });

  // Most reactive children resolve to text. Until one resolves to DOM, run
  // it in the effect's own scope and patch a text node, skipping the
  // per-run content root, its disposer and the publish bookkeeping.
  let textOnly = true;
  effect(() => {
    if (!textOnly)
      return createRoot(disposeCandidate =>
        renderDynamicChildContent(state, parent, child, doc, assertParentCurrent, disposeCandidate),
      );
    const assertEffectCurrent = captureViewLifetime();
    assertParentCurrent();
    const resolved = child();
    assertParentCurrent();
    assertEffectCurrent();
    const kind = typeof resolved;
    // Text that left nothing to clean up. A run that registered cleanups
    // must keep them until its replacement commits, which needs the root.
    if (
      (resolved == null || kind === 'string' || kind === 'number' || kind === 'boolean') &&
      currentOwnerIsEmpty()
    ) {
      const text = resolved == null || kind === 'boolean' ? '' : String(resolved);
      if (state.textNodeRef) {
        if (state.lastTextStr !== text) {
          state.textNodeRef.nodeValue = text;
          domMutationState.count++;
          state.lastTextStr = text;
        }
        return;
      }
      const node = doc.createTextNode(text);
      withDocumentAdoption(
        parent,
        node,
        () => {
          parent.insertBefore(node, null);
        },
        assertParentCurrent,
      );
      domMutationState.count++;
      assertParentCurrent();
      state.currentNode = state.textNodeRef = node;
      state.lastTextStr = text;
      return;
    }
    // First DOM result or owned content: hand what this run created to a
    // content root, as the general path would have, and stay on that path.
    textOnly = false;
    adoptCurrentOwnerIntoRoot(disposeCandidate =>
      renderDynamicChildContent(state, parent, child, doc, assertParentCurrent, disposeCandidate, {
        value: resolved,
      }),
    );
  });
}

/**
 * Appends a static or reactive child into a parent DOM node or DocumentFragment.
 */
export function insertChild(
  parent: Node,
  child: ChildNodeValue,
  assertInsertionCurrent?: () => void,
): void {
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
    assertCurrent();
    withDocumentAdoption(
      parent,
      child,
      () => {
        parent.appendChild(child);
      },
      assertCurrent,
    );
    domMutationState.count++;
    assertCurrent();
    return;
  }

  if (isReactiveAccessor(child)) {
    insertReactiveChild(parent, child, assertCurrent);
    assertCurrent();
    return;
  }

  const text = String(child);
  assertCurrent();
  const textNode = document.createTextNode(text);
  assertCurrent();
  parent.appendChild(textNode);
  domMutationState.count++;
  assertCurrent();
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
  const element = childNode.nodeType === 1 ? (childNode as Element) : null;
  if (
    element &&
    element.namespaceURI === HTML_NAMESPACE &&
    AMBIGUOUS_SVG_HTML_TAGS.has(element.tagName.toLowerCase())
  ) {
    const svgClone = document.createElementNS(SVG_NAMESPACE, element.tagName.toLowerCase());
    for (let i = 0; i < element.attributes.length; i++) {
      const attr = element.attributes[i]!;
      if (attr.namespaceURI) {
        svgClone.setAttributeNS(attr.namespaceURI, attr.name, attr.value);
      } else {
        svgClone.setAttribute(attr.name, attr.value);
      }
    }
    // Preserve delegated event handlers (`__svet_evt_*`) attached to `childNode`
    const childRecord = childNode as unknown as Record<string, unknown>;
    const cloneRecord = svgClone as unknown as Record<string, unknown>;
    for (const propKey of Object.keys(childNode)) {
      if (propKey.startsWith(EVENT_PROP_PREFIX)) {
        cloneRecord[propKey] = childRecord[propKey];
      }
    }
    while (childNode.firstChild) {
      svgClone.appendChild(
        upgradeAmbiguousSvgChild(svgClone as unknown as HTMLElement, childNode.firstChild),
      );
    }
    return svgClone;
  }
  return childNode;
}

type ElementTypeInput = string | ((props: never) => Node | PromiseLike<Node>);
type ElementPropsInput = Record<string, unknown> | null;

function constructElement(
  type: ElementTypeInput,
  props: ElementPropsInput,
  hasExplicitChildren: boolean,
  explicitChildren: ChildNodeValue[],
  hasExplicitKey: boolean,
  explicitKey: string | number | undefined,
): Node {
  const assertCallerCurrent = captureViewLifetime();
  assertCallerCurrent();
  return runWithOwnerScope([], () => {
    const assertConstructionCurrent = captureViewLifetime();
    const assertCurrent = () => {
      assertCallerCurrent();
      assertConstructionCurrent();
    };
    assertCurrent();
    const normalizedProps: Record<string | symbol, unknown> = {};
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
        Object.defineProperty(normalizedProps, key, {
          value,
          enumerable: true,
          configurable: true,
          writable: true,
        });
      }
    }
    if (hasExplicitKey) {
      Object.defineProperty(normalizedProps, 'key', {
        value: explicitKey,
        enumerable: true,
        configurable: true,
        writable: true,
      });
    }
    if (hasExplicitChildren && explicitChildren.length > 0) {
      Object.defineProperty(normalizedProps, 'children', {
        value: explicitChildren.length === 1 ? explicitChildren[0] : explicitChildren,
        enumerable: true,
        configurable: true,
        writable: true,
      });
    }

    if (typeof type === 'function') {
      const componentFn = type as unknown as UntypedComponentFn;
      const rendered = asyncComponentRenderer?.(componentFn, normalizedProps);
      assertCurrent();
      if (rendered !== undefined) return rendered;
      const result = untrack(() => componentFn(normalizedProps));
      assertCurrent();
      const then = untrack(() => result && (result as unknown as { then?: unknown }).then);
      assertCurrent();
      if (typeof then === 'function') {
        // Observe an unsupported promise too, so a rejected component does not
        // escape as an unhandled rejection alongside the synchronous diagnostic.
        void untrack(() => Promise.resolve(result)).catch(() => {});
        throw new TypeError('Async components require renderToStringAsync from svet-engine/server');
      }
      return result as Node;
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
      const children = normalizedProps.children as ChildNodeValue;
      if (el.namespaceURI === SVG_NAMESPACE && type !== 'foreignObject') {
        const upgraded = Array.isArray(children)
          ? children.map((c: unknown) => (isDomNode(c) ? upgradeAmbiguousSvgChild(el, c) : c))
          : isDomNode(children)
            ? upgradeAmbiguousSvgChild(el, children)
            : children;
        insertChild(el, upgraded as ChildNodeValue, assertCurrent);
      } else {
        insertChild(el, children, assertCurrent);
      }
    }
    if (el.tagName === 'SELECT') {
      untrack(() => {
        for (const synchronize of modelSynchronizers.get(el) ?? []) {
          assertCurrent();
          synchronize();
          assertCurrent();
        }
      });
    }
    // A live form owns routing even before it has any modeled controls. Native
    // later insertions/moves must not depend on a mutation-observer checkpoint.
    if (el.tagName === 'FORM') retainOwnedResetRoot(el);
    assertCurrent();
    return el;
  });
}

/**
 * Core hyperscript / JSX factory that builds real DOM & SVG nodes directly (Zero VDOM).
 */
export function h(
  type: string,
  props: Record<string, unknown> | null,
  ...children: ChildNodeValue[]
): Node;
export function h<P = Record<PropertyKey, unknown>>(
  type: Component<P> | AsyncComponent<P>,
  props: P | Omit<P, 'children'> | null,
  ...children: ChildNodeValue[]
): Node;
export function h(
  type: ElementTypeInput,
  props: ElementPropsInput,
  ...children: ChildNodeValue[]
): Node;
export function h(
  type: ElementTypeInput,
  props: ElementPropsInput,
  ...children: ChildNodeValue[]
): Node {
  return constructElement(type, props, true, children, false, undefined);
}

/** @internal Automatic JSX runtime entry without intermediate prop cloning or child rest-spreading. */
export function createJsxElement(
  type: string,
  props: Record<string, unknown> | null,
  key?: string | number,
): Node;
export function createJsxElement<P = Record<PropertyKey, unknown>>(
  type: Component<P> | AsyncComponent<P>,
  props: P | null,
  key?: string | number,
): Node;
export function createJsxElement(
  type: ElementTypeInput,
  props: ElementPropsInput,
  key?: string | number,
): Node;
export function createJsxElement(
  type: ElementTypeInput,
  props: ElementPropsInput,
  key?: string | number,
): Node {
  return constructElement(type, props, false, [], key !== undefined, key);
}

export function Fragment(props: { children?: ChildNodeValue }): DocumentFragment {
  const assertCallerCurrent = captureViewLifetime();
  assertCallerCurrent();
  return runWithOwnerScope([], () => {
    const assertFragmentCurrent = captureViewLifetime();
    const assertCurrent = () => {
      assertCallerCurrent();
      assertFragmentCurrent();
    };
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

/** @internal Commits a branch with partial-insertion cleanup. */
export function mountOwnedViewNodes(
  fragment: DocumentFragment,
  anchor: Comment,
  content: ChildNodeValue,
  assertViewCurrent: () => void,
  onMutation?: () => void,
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
      withDocumentAdoption(
        target,
        node,
        () => {
          target.insertBefore(node, anchor);
        },
        assertCurrent,
      );
      onMutation?.();
      assertCurrent();
    }
    return nodes;
  } catch (failure) {
    for (const node of nodes) {
      try {
        if (node.parentNode) {
          node.parentNode.removeChild(node);
          onMutation?.();
        }
      } catch {
        /* Preserve the insertion failure. */
      }
    }
    throw failure;
  }
}
