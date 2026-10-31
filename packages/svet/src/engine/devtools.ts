/**
 * # Svet DevTools, Deterministic Time-Travel Debugger & High-Performance UI Kit (`devtools.ts`)
 *
 * 1. **Deterministic Time-Travel Debugger (`createTimeTravelDebugger`)**:
 *    - Records snapshots of tracked `Signal<any>` values and WASM/GPU telemetry.
 *    - Allows scrubbing backward/forward (`undo()`, `redo()`, `jumpTo(index)`) atomically inside `batch()`
 *      without triggering duplicate intermediate history entries!
 *
 * 2. **100,000+ Row Windowed Virtual List (`VirtualList` / `createVirtualList`)**:
 *    - Renders only the visible viewport slice (`[startIndex .. endIndex]`) plus overscan buffer,
 *      keeping DOM node count under ~25 elements even for 100,000+ reactive items!
 */

import { getDomMutationCount, h, insertChild } from './dom.ts';
import { getGpuTelemetry, type GpuDeviceTelemetry } from './gpu.ts';
import {
  batch,
  computed,
  effect,
  getGraphTelemetry,
  onCleanup,
  type ReadonlySignal,
  signal,
  type Signal,
} from './reactivity.ts';

// ============================================================================
// 1. Deterministic Time-Travel Debugger (`createTimeTravelDebugger`)
// ============================================================================

export interface TimeTravelSnapshot<T extends Record<string, any> = Record<string, any>> {
  readonly index: number;
  readonly label: string;
  readonly timestamp: number;
  readonly values: T;
  readonly wasmTelemetry: ReturnType<typeof getGraphTelemetry>;
  readonly gpuTelemetry: GpuDeviceTelemetry;
  readonly domMutations: number;
}

export interface TimeTravelDebugger<T extends Record<string, Signal<any>>> {
  readonly history: ReadonlySignal<Array<TimeTravelSnapshot<{ [K in keyof T]: ReturnType<T[K]> }>>>;
  readonly cursor: ReadonlySignal<number>;
  readonly canUndo: ReadonlySignal<boolean>;
  readonly canRedo: ReadonlySignal<boolean>;
  /** Records a new snapshot of all watched signals with an optional label */
  record(label?: string): TimeTravelSnapshot<{ [K in keyof T]: ReturnType<T[K]> }>;
  /** Scrubs to a specific snapshot index in $O(1)$ using atomic WASM `batch()` */
  jumpTo(index: number): boolean;
  /** Steps 1 snapshot back in time */
  undo(): boolean;
  /** Steps 1 snapshot forward in time */
  redo(): boolean;
  /** Clears history and records current state as index 0 */
  clear(): void;
  dispose(): void;
}

function cloneValue<V>(val: V): V {
  if (val !== null && typeof val === 'object') {
    if (typeof structuredClone === 'function') {
      try {
        return structuredClone(val);
      } catch {
        // Fallback for non-cloneable properties
      }
    }
    return JSON.parse(JSON.stringify(val));
  }
  return val;
}

function areValuesEqual(a: any, b: any): boolean {
  if (Object.is(a, b)) return true;
  if (a !== null && b !== null && typeof a === 'object' && typeof b === 'object') {
    try {
      return JSON.stringify(a) === JSON.stringify(b);
    } catch {
      return false;
    }
  }
  return false;
}

/**
 * Creates a deterministic $O(1)$ Time-Travel Debugger over a dictionary of Svet `Signal`s.
 * Automatically records initial state and can either auto-record on signal changes or record on demand.
 * Uses structural sharing across unchanged keys to minimize memory overhead!
 */
export function createTimeTravelDebugger<T extends Record<string, Signal<any>>>(
  watchedSignals: T,
  options?: {
    maxSnapshots?: number;
    autoRecord?: boolean;
  }
): TimeTravelDebugger<T> {
  type ValueMap = { [K in keyof T]: ReturnType<T[K]> };
  const maxSnapshots = options?.maxSnapshots ?? 100;
  const autoRecord = options?.autoRecord ?? true;

  const historySig = signal<Array<TimeTravelSnapshot<ValueMap>>>([]);
  const cursorSig = signal<number>(-1);
  let isTimeTraveling = false;

  const captureValues = (prevValues?: ValueMap): { values: ValueMap; changed: boolean } => {
    const out: Record<string, any> = {};
    let changed = !prevValues;
    for (const [key, sig] of Object.entries(watchedSignals)) {
      const liveVal = sig.peek();
      if (prevValues && key in prevValues && areValuesEqual(prevValues[key], liveVal)) {
        // Structural sharing: reuse exact reference from previous snapshot!
        out[key] = prevValues[key];
      } else {
        out[key] = cloneValue(liveVal);
        changed = true;
      }
    }
    return { values: out as ValueMap, changed };
  };

  const record = (label = 'State Mutation'): TimeTravelSnapshot<ValueMap> => {
    const currentList = historySig.peek();
    const curIdx = cursorSig.peek();
    // Truncate any future branch if we recorded after an undo
    const sliced = curIdx >= 0 && curIdx < currentList.length - 1
      ? currentList.slice(0, curIdx + 1)
      : [...currentList];

    const prevSnap = sliced.length > 0 ? sliced[sliced.length - 1] : undefined;
    const { values, changed } = captureValues(prevSnap?.values);
    if (prevSnap && !changed && label === 'Auto Snapshot') {
      return prevSnap;
    }

    const snap: TimeTravelSnapshot<ValueMap> = {
      index: sliced.length,
      label,
      timestamp: Date.now(),
      values,
      wasmTelemetry: getGraphTelemetry(),
      gpuTelemetry: getGpuTelemetry(),
      domMutations: getDomMutationCount(),
    };

    sliced.push(snap);
    if (sliced.length > maxSnapshots) {
      sliced.shift();
      // Re-index
      for (let i = 0; i < sliced.length; i++) {
        (sliced[i] as any).index = i;
      }
    }

    batch(() => {
      historySig.set(sliced);
      cursorSig.set(sliced.length - 1);
    });

    return snap;
  };

  // Record initial snapshot (`t = 0`)
  record('Initial State (t=0)');

  let autoEffectDispose: (() => void) | null = null;
  if (autoRecord) {
    let isFirstRun = true;
    const eff = effect(() => {
      // Subscribe to all watched signals
      for (const sig of Object.values(watchedSignals)) {
        sig();
      }
      if (isFirstRun) {
        isFirstRun = false;
        return;
      }
      if (isTimeTraveling) return;
      record('Auto Snapshot');
    });
    autoEffectDispose = () => eff.dispose();
  }

  const jumpTo = (targetIndex: number): boolean => {
    const list = historySig.peek();
    if (targetIndex < 0 || targetIndex >= list.length) {
      return false;
    }
    const targetSnap = list[targetIndex];
    isTimeTraveling = true;
    try {
      batch(() => {
        for (const [key, sig] of Object.entries(watchedSignals)) {
          if (key in targetSnap.values) {
            sig.set(cloneValue(targetSnap.values[key]));
          }
        }
        cursorSig.set(targetIndex);
      });
    } finally {
      isTimeTraveling = false;
    }
    return true;
  };

  const undo = (): boolean => {
    const cur = cursorSig.peek();
    if (cur <= 0) return false;
    return jumpTo(cur - 1);
  };

  const redo = (): boolean => {
    const cur = cursorSig.peek();
    const list = historySig.peek();
    if (cur >= list.length - 1) return false;
    return jumpTo(cur + 1);
  };

  const clear = (): void => {
    batch(() => {
      historySig.set([]);
      cursorSig.set(-1);
    });
    record('Reset State (t=0)');
  };

  const canUndo = computed(() => cursorSig() > 0);
  const canRedo = computed(() => cursorSig() >= 0 && cursorSig() < historySig().length - 1);

  return {
    history: historySig,
    cursor: cursorSig,
    canUndo,
    canRedo,
    record,
    jumpTo,
    undo,
    redo,
    clear,
    dispose() {
      if (autoEffectDispose) autoEffectDispose();
      historySig.dispose();
      cursorSig.dispose();
      canUndo.dispose();
      canRedo.dispose();
    },
  };
}

// ============================================================================
// 2. High-Performance Windowed Virtual List (`VirtualList` for 100,000+ rows)
// ============================================================================

export interface VirtualListProps<T> {
  /** Reactive signal or array of items (supports 100,000+ rows) */
  items: ReadonlySignal<T[]> | (() => T[]) | T[];
  /** Fixed row height in pixels OR dynamic row height resolver `(item, index) => heightPx` */
  itemHeight: number | ((item: T, index: number) => number);
  /** Viewport height in pixels */
  height: number;
  /** Number of extra rows rendered above and below the viewport (default: 4) */
  overscan?: number;
  /** Row renderer returning a DOM Node for `(item, globalIndex)` */
  renderItem: (item: T, index: number) => Node;
  /** Optional custom CSS class or inline style on the scroll container */
  style?: Partial<CSSStyleDeclaration> | string;
}

export interface VirtualListHandle<T> extends HTMLElement {
  /** Programmatically scrolls to a specific item index and updates the rendered window immediately */
  scrollToIndex(index: number): void;
  /** Returns the currently rendered `[startIndex, endIndex)` range */
  getVisibleRange(): [number, number];
}

function binarySearchPrefixOffset(prefixOffsets: Float64Array, targetScrollTop: number): number {
  let low = 0;
  let high = Math.max(0, prefixOffsets.length - 2);
  while (low < high) {
    const mid = (low + high + 1) >>> 1;
    if (prefixOffsets[mid] <= targetScrollTop) {
      low = mid;
    } else {
      high = mid - 1;
    }
  }
  return low;
}

/**
 * Windowed Virtual List component (`VirtualList`) that renders 100,000+ rows at 120fps
 * by mounting ONLY the visible slice `[startIndex .. endIndex)` inside the DOM.
 * Supports both fixed `itemHeight` ($O(1)$) and dynamic `itemHeight: (item, index) => number`
 * via $O(\log n)$ binary search over prefix height sums!
 */
export function VirtualList<T>(props: VirtualListProps<T>): VirtualListHandle<T> {
  const isDynamicHeight = typeof props.itemHeight === 'function';
  const fixedItemHeight = typeof props.itemHeight === 'number' ? Math.max(1, props.itemHeight) : 28;
  const viewportHeight = Math.max(1, props.height);
  const overscan = props.overscan ?? 4;

  const scrollTopSig = signal(0);
  const visibleRangeSig = signal<[number, number]>([0, 0]);

  const getItems = (): T[] => {
    return typeof props.items === 'function' ? (props.items as () => T[])() : props.items;
  };

  // Memoize dynamic row prefix heights in a WASM `computed()` signal so scrolling
  // does NOT recompute $O(N)$ prefix sums — only $O(\log n)$ binary search runs on scroll!
  const dynamicPrefixOffsets = isDynamicHeight
    ? computed<Float64Array>(() => {
        const list = getItems();
        const totalCount = list.length;
        const heightFn = props.itemHeight as (item: T, index: number) => number;
        const prefixes = new Float64Array(totalCount + 1);
        for (let i = 0; i < totalCount; i++) {
          prefixes[i + 1] = prefixes[i] + Math.max(1, heightFn(list[i], i));
        }
        return prefixes;
      })
    : null;

  const container = document.createElement('div') as unknown as VirtualListHandle<T>;
  container.setAttribute('data-svet-virtual-list', 'true');
  container.style.position = 'relative';
  container.style.height = `${viewportHeight}px`;
  container.style.overflowY = 'auto';
  container.style.overflowX = 'hidden';

  if (props.style) {
    if (typeof props.style === 'string') {
      container.style.cssText += `; ${props.style}`;
    } else {
      Object.assign(container.style, props.style);
    }
  }

  const phantomSpacer = document.createElement('div');
  phantomSpacer.style.position = 'relative';
  phantomSpacer.style.width = '100%';
  phantomSpacer.style.pointerEvents = 'none';

  const contentLayer = document.createElement('div');
  contentLayer.setAttribute('data-svet-virtual-layer', 'true');
  contentLayer.style.position = 'absolute';
  contentLayer.style.top = '0';
  contentLayer.style.left = '0';
  contentLayer.style.right = '0';
  contentLayer.style.willChange = 'transform';

  container.appendChild(phantomSpacer);
  container.appendChild(contentLayer);

  const onScroll = () => {
    scrollTopSig.set(container.scrollTop);
  };
  container.addEventListener('scroll', onScroll, { passive: true });
  onCleanup(() => container.removeEventListener('scroll', onScroll));

  effect(() => {
    const list = getItems();
    const totalCount = list.length;
    const scrollTop = scrollTopSig();

    let startIdx = 0;
    let endIdx = 0;
    let topOffsetPx = 0;
    let prefixes: Float64Array | null = null;

    if (!isDynamicHeight) {
      phantomSpacer.style.height = `${totalCount * fixedItemHeight}px`;
      const rawStart = Math.floor(scrollTop / fixedItemHeight);
      const visibleCount = Math.ceil(viewportHeight / fixedItemHeight);
      startIdx = Math.max(0, rawStart - overscan);
      endIdx = Math.min(totalCount, rawStart + visibleCount + overscan);
      topOffsetPx = startIdx * fixedItemHeight;
    } else {
      prefixes = dynamicPrefixOffsets!();
      const totalHeight = prefixes[totalCount];
      phantomSpacer.style.height = `${totalHeight}px`;

      const rawStart = binarySearchPrefixOffset(prefixes, scrollTop);
      const rawEnd = binarySearchPrefixOffset(prefixes, scrollTop + viewportHeight) + 1;
      startIdx = Math.max(0, rawStart - overscan);
      endIdx = Math.min(totalCount, rawEnd + overscan);
      topOffsetPx = prefixes[startIdx];
    }

    visibleRangeSig.set([startIdx, endIdx]);
    contentLayer.style.transform = `translateY(${topOffsetPx}px)`;
    contentLayer.innerHTML = '';

    const frag = document.createDocumentFragment();
    for (let idx = startIdx; idx < endIdx; idx++) {
      const rowH =
        isDynamicHeight && prefixes
          ? Math.max(1, prefixes[idx + 1] - prefixes[idx])
          : fixedItemHeight;
      const rowWrapper = document.createElement('div');
      rowWrapper.setAttribute('data-virtual-index', String(idx));
      rowWrapper.style.height = `${rowH}px`;
      rowWrapper.style.boxSizing = 'border-box';
      rowWrapper.style.display = 'flex';
      rowWrapper.style.alignItems = 'center';
      insertChild(rowWrapper, props.renderItem(list[idx], idx));
      frag.appendChild(rowWrapper);
    }
    contentLayer.appendChild(frag);
  });

  container.scrollToIndex = (index: number) => {
    let clampedTop = 0;
    if (isDynamicHeight && dynamicPrefixOffsets) {
      const prefixes = dynamicPrefixOffsets.peek();
      if (index >= 0 && index < prefixes.length) {
        clampedTop = prefixes[index];
      }
    } else {
      clampedTop = Math.max(0, index * fixedItemHeight);
    }
    container.scrollTop = clampedTop;
    scrollTopSig.set(clampedTop);
  };

  container.getVisibleRange = () => visibleRangeSig.peek();

  return container;
}
