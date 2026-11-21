/**
 * Reactive conditional (`Show`) and WASM-accelerated keyed list (`For`)
 * control-flow components for Svet.
 */

import { mountOwnedViewNodes } from './dom-core.ts';
import {
  captureViewLifetime,
  type ChildNodeValue,
  domMutationState,
  isReactiveAccessor,
  type ReactiveValue,
  withDocumentAdoption,
} from './dom-events.ts';
import {
  createRoot,
  disposeDetachedScope,
  disposeSignalNow,
  effect,
  onCleanup,
  type OwnerScope,
  runInDetachedScope,
  runWithOwnerScope,
  setSignalNow,
  signal,
  type Signal,
  untrack,
} from './reactivity.ts';
import { computeLisWasm, getWasmCore, withWasmCore } from './wasm-bridge.ts';

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
      activeState = 'none';
      lastTruthyVal = undefined;
      const dispose = disposeBranchScope;
      disposeBranchScope = null;
      dispose?.();
      const nodes = currentNodes;
      currentNodes = [];
      for (const node of nodes) {
        if (node.parentNode) {
          node.parentNode.removeChild(node);
          domMutationState.count++;
        }
      }
    };
    onCleanup(clearCurrent);

    const mountBranch = (factory: () => ChildNodeValue) => {
      createRoot(dispose => {
        disposeBranchScope = dispose;
        const content = untrack(factory);
        currentNodes = mountOwnedViewNodes(frag, anchor, content, assertViewCurrent, () => {
          domMutationState.count++;
        });
      });
    };

    effect(() => {
      assertViewCurrent();
      const when = props.when;
      assertViewCurrent();
      const cond = isReactiveAccessor(when) ? when() : when;
      assertViewCurrent();
      const truthy = Boolean(cond);
      if (
        truthy &&
        activeState === 'truthy' &&
        (!isCallbackChild || Object.is(lastTruthyVal, cond))
      )
        return;
      if (!truthy && activeState === 'falsy') return;
      clearCurrent();
      assertViewCurrent();
      if (truthy) {
        mountBranch(() => {
          const children = props.children;
          assertViewCurrent();
          return typeof children === 'function'
            ? (children as (value: NonNullable<T>) => Node)(cond as NonNullable<T>)
            : children;
        });
      } else {
        const fallback = props.fallback;
        assertViewCurrent();
        if (fallback !== undefined)
          mountBranch(() =>
            typeof fallback === 'function' ? (fallback as () => Node)() : fallback,
          );
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

type MappedRecord<T, K> = {
  key: K;
  item: T;
  itemSig?: Signal<T>;
  node: Node;
  end?: Node;
  preparedNodes?: Node[] | undefined;
  movingNodes?: Node[] | undefined;
  index: number;
  indexSig?: Signal<number>;
  disposeScope: () => void;
};

function isIntForKey<K>(key: K): boolean {
  return typeof key === 'number' && key >= 0 && key < 65536 && (key | 0) === key;
}

function removeForDomNode(node: Node): void {
  if (node.parentNode) {
    node.parentNode.removeChild(node);
    domMutationState.count++;
  }
}

function collectUniqueForKeys<T, K>(
  items: readonly T[],
  getKey: (item: T, index: number) => K,
  assertViewCurrent: () => void,
): K[] {
  const keys: K[] = [],
    seen = new Set<K>();
  for (let i = 0; i < items.length; i++) {
    const key = getKey(items[i]!, i);
    assertViewCurrent();
    if (seen.has(key)) throw new Error('Duplicate For key');
    seen.add(key);
    keys.push(key);
  }
  return keys;
}

function getForRowNodes<T, K>(rec: MappedRecord<T, K>): Node[] {
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
}

function clearForSnapshots<T, K>(records: readonly MappedRecord<T, K>[]): void {
  for (const rec of records) {
    rec.preparedNodes = undefined;
    rec.movingNodes = undefined;
  }
}

function discardForRows<T, K>(
  records: readonly MappedRecord<T, K>[],
  keepSnapshots: boolean,
): void {
  for (const rec of records) {
    let nodes: Node[];
    try {
      nodes = getForRowNodes(rec);
    } catch {
      nodes = [rec.node, rec.end].filter((node): node is Node => !!node);
    }
    try {
      rec.disposeScope();
    } catch {
      /* Complete remaining row teardown. */
    }
    for (const node of nodes) {
      try {
        removeForDomNode(node);
      } catch {
        /* Teardown remains best effort. */
      }
    }
  }
  // A synchronous insertion hook may still attach a node after owner cleanup.
  if (!keepSnapshots) clearForSnapshots(records);
}

function moveForRow<T, K>(
  rec: MappedRecord<T, K>,
  parent: Node,
  anchor: Node,
  assertViewCurrent: () => void,
): void {
  if (!rec.end) {
    assertViewCurrent();
    withDocumentAdoption(
      parent,
      rec.node,
      () => {
        parent.insertBefore(rec.node, anchor);
      },
      assertViewCurrent,
    );
    domMutationState.count++;
    assertViewCurrent();
    return;
  }
  const nodes = getForRowNodes(rec);
  rec.movingNodes = nodes;
  for (const node of nodes) {
    assertViewCurrent();
    withDocumentAdoption(
      parent,
      node,
      () => {
        parent.insertBefore(node, anchor);
      },
      assertViewCurrent,
    );
    domMutationState.count++;
    assertViewCurrent();
  }
  rec.movingNodes = undefined;
  rec.preparedNodes = undefined;
}

function createForMappedRecord<T, K>(
  key: K,
  item: T,
  i: number,
  core: ReturnType<typeof getWasmCore>,
  assertViewCurrent: () => void,
  render: (item: T, index: () => number, current: () => T) => Node,
): MappedRecord<T, K> {
  let rowScope: OwnerScope | undefined,
    rowDisposed = false;
  const rec: MappedRecord<T, K> = {
    key,
    item,
    node: null as unknown as Node,
    index: i,
    disposeScope() {
      if (rowDisposed) return;
      rowDisposed = true;
      try {
        if (rowScope) disposeDetachedScope(core, rowScope);
      } finally {
        try {
          if (rec.indexSig) disposeSignalNow(rec.indexSig);
        } finally {
          if (rec.itemSig) disposeSignalNow(rec.itemSig);
        }
      }
    },
  };
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
  // A row owns a detached scope rather than a public root: the record
  // is the only holder, so it needs no disposer closure or cell.
  try {
    rec.node = untrack(() =>
      runInDetachedScope(scope => {
        rowScope = scope;
        const node = render(item, index, current);
        assertViewCurrent();
        if (node.nodeType === 11) {
          const start = node.ownerDocument!.createComment('svet-for-row');
          const end = node.ownerDocument!.createComment('svet-for-row-end');
          rec.node = start;
          rec.end = end;
          rec.preparedNodes = [start, ...Array.from(node.childNodes), end];
          node.insertBefore(start, node.firstChild);
          assertViewCurrent();
          node.appendChild(end);
          assertViewCurrent();
          return start;
        }
        return node;
      }),
    );
    assertViewCurrent();
    return rec;
  } catch (failure) {
    rec.disposeScope();
    throw failure;
  }
}

function applyForDomMoves<T, K>(
  nextRecords: MappedRecord<T, K>[],
  oldIndices: Uint32Array,
  nextLen: number,
  oldLen: number,
  reusedCount: number,
  displacedCount: number,
  firstDisplacedIdx: number,
  secondDisplacedIdx: number,
  parent: Node,
  endMarker: Comment,
  assertViewCurrent: () => void,
): void {
  if (displacedCount > 0 && displacedCount <= 2 && reusedCount === nextLen && oldLen === nextLen) {
    // Move only displaced rows; fragment rows move their complete live ranges.
    const moveIdx = (idx: number) => {
      const rec = nextRecords[idx]!;
      const anchor = idx + 1 < nextLen ? nextRecords[idx + 1]!.node : endMarker;
      moveForRow(rec, parent, anchor, assertViewCurrent);
    };
    if (displacedCount === 2) moveIdx(secondDisplacedIdx);
    moveIdx(firstDisplacedIdx);
    return;
  }
  if (reusedCount === 0) {
    // Initial mount or full replacement: no existing rows are reused, so LIS is empty.
    let anchorNode: Node = endMarker;
    for (let i = nextLen - 1; i >= 0; i--) {
      const rec = nextRecords[i]!;
      moveForRow(rec, parent, anchorNode, assertViewCurrent);
      anchorNode = rec.node;
    }
    return;
  }
  const lisPositions = computeLisWasm(oldIndices.subarray(0, nextLen));
  let lisCursor = lisPositions.length - 1;
  let anchorNode: Node = endMarker;
  for (let i = nextLen - 1; i >= 0; i--) {
    const rec = nextRecords[i]!;
    const isNew = oldIndices[i] === NEW_ITEM_MARKER;
    let isStationary = false;
    if (!isNew && lisCursor >= 0 && lisPositions[lisCursor] === i) {
      isStationary = true;
      lisCursor--;
    }
    if (!isStationary) moveForRow(rec, parent, anchorNode, assertViewCurrent);
    anchorNode = rec.node;
  }
}

function teardownRemovedForRows<T, K>(
  removed: MappedRecord<T, K>[],
  oldRecords: MappedRecord<T, K>[],
  nextLen: number,
  oldLen: number,
  parent: Node,
  endMarker: Comment,
): void {
  // One untrack frame for the whole teardown, so each row's root and
  // effect disposers find reads already idle and skip their own.
  untrack(() => {
    const first = oldRecords[0]?.node;
    const wholeParent =
      nextLen === 0 &&
      removed.length === oldLen &&
      parent.nodeType === 1 &&
      parent.firstChild === first &&
      parent.lastChild === endMarker;
    if (wholeParent) {
      for (const rec of removed) rec.disposeScope();
      // The rows filled the parent, so drop them in one DOM operation
      // instead of one removeChild per row, as long as teardown left
      // that shape in place.
      if (parent.firstChild === first && parent.lastChild === endMarker) {
        (parent as Element).textContent = '';
        parent.appendChild(endMarker);
        domMutationState.count += removed.length;
        return;
      }
      for (const rec of removed) for (const node of getForRowNodes(rec)) removeForDomNode(node);
      return;
    }
    for (const rec of removed) {
      const nodes = getForRowNodes(rec);
      rec.disposeScope();
      for (const node of nodes) removeForDomNode(node);
    }
  });
}

function recoverFailedForUpdate<T, K>(
  assertViewCurrent: () => void,
  committed: boolean,
  oldRecords: MappedRecord<T, K>[],
  created: MappedRecord<T, K>[],
  removed: MappedRecord<T, K>[],
  parent: Node,
  endMarker: Comment,
  clearAllStores: () => void,
  clearScratch: () => void,
): void {
  let alive = true;
  try {
    assertViewCurrent();
  } catch {
    alive = false;
  }
  if (!alive) {
    clearAllStores();
    discardForRows(oldRecords, true);
    discardForRows(created, true);
    clearForSnapshots(oldRecords);
    clearForSnapshots(created);
    try {
      removeForDomNode(endMarker);
    } catch {
      /* Preserve lifetime failure. */
    }
  } else if (!committed) {
    clearScratch();
    discardForRows(created, true);
    clearForSnapshots(created);
    // Old rows are still live: restore their exact committed order.
    for (const rec of oldRecords) {
      try {
        moveForRow(rec, parent, endMarker, assertViewCurrent);
      } catch {
        /* Preserve original failure. */
      }
    }
  } else {
    discardForRows(removed, true);
    clearForSnapshots(removed);
  }
}

type ForReconcilerState<T, K> = {
  currentRecords: MappedRecord<T, K>[];
  scratchRecords: MappedRecord<T, K>[];
  recordMap: Map<K, MappedRecord<T, K>>;
  intKeyTable: Array<MappedRecord<T, K> | undefined>;
  scratchIndices: Uint32Array;
  updating: boolean;
};

function reconcileForRows<T, K>(
  state: ForReconcilerState<T, K>,
  props: {
    each: ReactiveValue<readonly T[]>;
    keyFn?: (item: T, index: number) => K;
    children: (item: T, index: () => number, current: () => T) => Node;
  },
  frag: DocumentFragment,
  endMarker: Comment,
  core: ReturnType<typeof getWasmCore>,
  assertViewCurrent: () => void,
): void {
  assertViewCurrent();
  const each = props.each;
  assertViewCurrent();
  const list = isReactiveAccessor(each) ? each() : each;
  const items = Array.from(list);
  assertViewCurrent();
  const getKey = props.keyFn ?? ((item: T) => item as unknown as K);
  const render = props.children;
  assertViewCurrent();
  const keys = collectUniqueForKeys(items, getKey, assertViewCurrent);
  const oldRecords = state.currentRecords,
    oldLen = oldRecords.length,
    nextLen = items.length;
  if (state.scratchIndices.length < nextLen) {
    state.scratchIndices = new Uint32Array(Math.max(nextLen, 64));
  }
  const oldIndices = state.scratchIndices;
  const nextRecords =
    state.scratchRecords.length === nextLen
      ? state.scratchRecords
      : new Array<MappedRecord<T, K>>(nextLen);
  const created: MappedRecord<T, K>[] = [],
    removed: MappedRecord<T, K>[] = [];
  const parent = endMarker.parentNode ?? frag;
  let anyMovedOrNew = false,
    prevOldIdx = -1,
    displacedCount = 0;
  let firstDisplacedIdx = -1,
    secondDisplacedIdx = -1,
    reusedCount = 0,
    committed = false;
  state.updating = true;
  try {
    // Prepare rows without publishing lookup entries or changing existing indexes.
    for (let i = 0; i < nextLen; i++) {
      const item = items[i]!,
        key = keys[i]!;
      const existing = isIntForKey(key)
        ? state.intKeyTable[key as number]
        : state.recordMap.get(key);
      if (existing) {
        reusedCount++;
        const oldIdx = existing.index;
        oldIndices[i] = oldIdx;
        if (oldIdx < prevOldIdx) {
          anyMovedOrNew = true;
          const prevWasJump =
            i >= 2
              ? oldIndices[i - 1]! > oldIdx && oldIdx > oldIndices[i - 2]!
              : oldIndices[0]! > oldIdx && i + 1 < nextLen;
          const displacedPos = prevWasJump ? i - 1 : i;
          displacedCount++;
          if (displacedCount === 1) firstDisplacedIdx = displacedPos;
          else if (displacedCount === 2) secondDisplacedIdx = displacedPos;
          if (prevWasJump) prevOldIdx = oldIdx;
        } else prevOldIdx = oldIdx;
        nextRecords[i] = existing;
      } else {
        oldIndices[i] = NEW_ITEM_MARKER;
        anyMovedOrNew = true;
        displacedCount = 999;
        const rec = createForMappedRecord<T, K>(
          key as K,
          item as T,
          i,
          core,
          assertViewCurrent,
          render,
        );
        created.push(rec);
        nextRecords[i] = rec;
      }
    }
    if (reusedCount < oldLen) {
      if (reusedCount === 0) for (let i = 0; i < oldLen; i++) removed.push(oldRecords[i]!);
      else {
        const nextSet = new Set(nextRecords);
        for (let i = 0; i < oldLen; i++) {
          const rec = oldRecords[i]!;
          if (!nextSet.has(rec)) removed.push(rec);
        }
      }
    }
    if (anyMovedOrNew) {
      applyForDomMoves(
        nextRecords,
        oldIndices,
        nextLen,
        oldLen,
        reusedCount,
        displacedCount,
        firstDisplacedIdx,
        secondDisplacedIdx,
        parent,
        endMarker,
        assertViewCurrent,
      );
    }
    assertViewCurrent();
    // DOM preparation succeeded. Publish records before any teardown callbacks.
    for (let i = 0; i < nextLen; i++) {
      const rec = nextRecords[i];
      if (!rec) continue;
      rec.index = i;
      rec.item = items[i] as T;
      if (rec.indexSig) setSignalNow(rec.indexSig, i);
      // A function-valued item is data, never a signal updater.
      if (rec.itemSig) setSignalNow(rec.itemSig, () => rec.item);
    }
    for (const rec of removed) {
      if (isIntForKey(rec.key)) state.intKeyTable[rec.key as number] = undefined;
      else state.recordMap.delete(rec.key);
    }
    for (const rec of created) {
      if (isIntForKey(rec.key)) state.intKeyTable[rec.key as number] = rec;
      else state.recordMap.set(rec.key, rec);
    }
    if (nextLen === 0) {
      state.recordMap.clear();
      state.intKeyTable.length = 0;
    }
    state.scratchRecords = oldRecords;
    state.currentRecords = nextRecords;
    committed = true;
    if (removed.length > 0)
      teardownRemovedForRows(removed, oldRecords, nextLen, oldLen, parent, endMarker);
    clearForSnapshots(removed);
    assertViewCurrent();
  } catch (failure) {
    recoverFailedForUpdate(
      assertViewCurrent,
      committed,
      oldRecords,
      created,
      removed,
      parent,
      endMarker,
      () => {
        state.currentRecords = [];
        state.scratchRecords = [];
        state.recordMap.clear();
        state.intKeyTable.length = 0;
      },
      () => {
        state.scratchRecords = [];
      },
    );
    throw failure;
  } finally {
    state.updating = false;
  }
}

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
    const assertViewCurrent = captureViewLifetime(),
      core = getWasmCore();
    const frag = document.createDocumentFragment();
    const endMarker = document.createComment('svet-for-end');
    frag.appendChild(endMarker);
    const state: ForReconcilerState<T, K> = {
      currentRecords: [],
      scratchRecords: [],
      recordMap: new Map<K, MappedRecord<T, K>>(),
      intKeyTable: [],
      scratchIndices: new Uint32Array(64),
      updating: false,
    };
    onCleanup(() => {
      const records = state.currentRecords;
      state.currentRecords = [];
      state.scratchRecords = [];
      state.recordMap.clear();
      state.intKeyTable.length = 0;
      discardForRows(records, state.updating);
      try {
        removeForDomNode(endMarker);
      } catch {
        /* Preserve owner teardown. */
      }
    });

    effect(() => reconcileForRows(state, props, frag, endMarker, core, assertViewCurrent));
    assertViewCurrent();
    return frag;
  });
}
