/**
 * Hierarchical context lookup (`createContext` / `useContext`), `Portal`, and
 * shared retirable callback helper for Svet application primitives.
 */

import {
  captureViewLifetime,
  type ChildNodeValue,
  insertChild,
  withDocumentAdoption,
} from './dom.ts';
import {
  getCurrentOwner,
  getReactiveGeneration,
  lookupOwnerContext,
  onCleanup,
  runWithOwnerScope,
} from './reactivity.ts';
import { getWasmCore } from './wasm-bridge.ts';

// Keep externally retained callbacks outside component/router closure environments.
export function retirableCallback(callback: () => void): {
  invoke: () => void;
  dispose: () => void;
} {
  let active: (() => void) | null = callback;
  return {
    invoke: () => {
      active?.();
    },
    dispose: () => {
      active = null;
    },
  };
}

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

  const Provider = (props: {
    value: T;
    children: ChildNodeValue | (() => Node);
  }): DocumentFragment => {
    const assertCallerCurrent = captureViewLifetime();
    assertCallerCurrent();
    return runWithOwnerScope([], () => {
      const assertConstructionCurrent = captureViewLifetime();
      const assertConstruction = () => {
        assertCallerCurrent();
        assertConstructionCurrent();
      };
      assertConstruction();
      const value = props.value;
      assertConstruction();
      return runWithOwnerScope([[id, value]], () => {
        const assertProviderCurrent = captureViewLifetime();
        const assertCurrent = () => {
          assertConstruction();
          assertProviderCurrent();
        };
        assertCurrent();
        const children = props.children;
        assertCurrent();
        const frag = document.createDocumentFragment();
        assertCurrent();
        const content = typeof children === 'function' ? (children as () => Node)() : children;
        assertCurrent();
        insertChild(frag, content, assertCurrent);
        assertCurrent();
        return frag;
      });
    });
  };

  return { id, defaultValue, Provider };
}

export function useContext<T>(context: Pick<SvetContext<T>, 'id' | 'defaultValue'>): T {
  return lookupOwnerContext(context.id, context.defaultValue);
}

// ============================================================================
// 2. Portals (`<Portal mount={...}>`)
// ============================================================================

export function Portal(props: {
  mount?: HTMLElement;
  children: ChildNodeValue | (() => Node);
}): Comment {
  return runWithOwnerScope([], () => {
    const owner = getCurrentOwner()!,
      core = getWasmCore(),
      generation = getReactiveGeneration(core);
    const assertCurrent = () => {
      if (owner.disposed) throw new Error('Portal construction owner disposed');
      if (generation !== getReactiveGeneration(core))
        throw new Error('Reactive graph reset during portal construction');
    };
    const placeholder = document.createComment('svet-portal');
    const container = document.createElement('div');
    container.setAttribute('data-svet-portal', '');
    // Register before user rendering/insertion so partial attachment can be undone.
    const removeContainer = () => {
      container.parentNode?.removeChild(container);
    };
    onCleanup(removeContainer);
    try {
      const target = props.mount ?? document.body;
      assertCurrent();
      const children = props.children;
      assertCurrent();
      const content = typeof children === 'function' ? (children as () => Node)() : children;
      assertCurrent();
      insertChild(container, content, assertCurrent);
      assertCurrent();
      withDocumentAdoption(
        target,
        container,
        () => {
          target.appendChild(container);
        },
        assertCurrent,
      );
      assertCurrent();
      return placeholder;
    } catch (failure) {
      // An insertion hook can attach after owner cleanup has already run.
      try {
        removeContainer();
      } catch {
        /* Preserve the construction failure. */
      }
      throw failure;
    }
  });
}
