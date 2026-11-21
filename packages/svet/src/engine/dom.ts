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

import { captureViewLifetime, delegationAreaKey, withDocumentAdoption } from './dom-events.ts';
import { retainOwnedResetRoot } from './dom-model.ts';
import { createRoot, getCurrentOwner, onCleanup } from './reactivity.ts';

export {
  type ReactiveValue,
  type ChildNodeValue,
  type Component,
  type AsyncComponent,
  getDomMutationCount,
  resetDomMutationCount,
  isUnsafeUrl,
  withDocumentAdoption,
  captureViewLifetime,
} from './dom-events.ts';
export { applyAttributeValue } from './dom-attributes.ts';
export {
  installAsyncComponentRenderer,
  bindProp,
  insertChild,
  h,
  createJsxElement,
  Fragment,
  mountOwnedViewNodes,
} from './dom-core.ts';
export { Show, For } from './dom-for.ts';

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
    assertCallerCurrent();
    assertRootCurrent();
    if (mountAttempts.get(container) !== attempt)
      throw new Error('Mount superseded by a newer mount');
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
      (mountOwner.context ??= new Map()).set(
        delegationAreaKey,
        treeRoot.nodeType === 11 && 'host' in treeRoot ? container : undefined,
      );
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
    withDocumentAdoption(
      container,
      node,
      () => {
        container.replaceChildren(node);
      },
      assertMountCurrent,
    );
    assertMountCurrent();
  } catch (error) {
    try {
      disposeRoot();
    } catch {
      /* Preserve the DOM/replacement failure. */
    }
    for (const prepared of preparedNodes) {
      try {
        if (prepared.parentNode === container) container.removeChild(prepared);
      } catch {
        /* Preserve the original failure if native rollback also fails. */
      }
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
      try {
        disposeRoot();
      } catch (error) {
        if (!domFailed) throw error;
      }
    }
  };
  mountedContainers.set(container, unmount);
  onCleanup(unmount);
  return { unmount };
}
