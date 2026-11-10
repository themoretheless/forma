/**
 * # Svet Deep Integration with **Forma** (`forma.ts`)
 *
 * Bridges the **Forma** declarative UI language & ViewModel binding system (`../forma`)
 * directly into the **Svet** Rust WASM Reactive DAG and Surgical DOM Engine:
 *
 * 1. **Forma `.ui` Parser & Compiler (`compileFormaComponent`, `mountForma`)**:
 *    - Parses Forma `.ui` component definitions (`component Name { ... }`):
 *      - `tokens { spacing.md: 16; colors.surface: #1e293b; }`
 *      - `i18n ru { ... }` / `i18n en { ... }` with runtime `locale` Signal switching
 *      - `style name { ... }` & `apply styleName;`
 *      - `prop` / `required prop` with `#[range(min, max)]` and `#[pattern('regex')]` + reactive `isValid` computed signal
 *      - `state field: Type = default;` mapped to WASM Signals
 *      - `let name = expr;` mapped to WASM Glitch-Free Computed Signals
 *      - `event name(...);` & `events: ['...'];`
 *      - `shortcut 'Mod+K' -> state.toggle('...');`
 *      - `Row`, `Column`, `Grid`, `Stack`, `Frame`, `Modal`, `Text`, `TextInput`/`TextField`, `Button`, `Badge`, `ProgressBar`, `Divider`
 *      - Control flow: `if cond { ... } else { ... }`, `match expr { ... }`, and `for item in list key item.id { ... } empty { ... }` (backed by Svet's WASM $O(n \log n)$ LIS `<For>` reconciler!)
 *      - Two-way bindings `value <-> state.field;` and event actions `clicked -> state.increment('count', 1);` / `clicked -> events.saveRequested();`
 *
 * 2. **Forma ViewModel & `Property<T>` / `Context<T>` Compatibility (`createFormaProperty`, `createFormaViewModel`)**:
 *    - Full compatibility with Forma's `Property<T>` (`get()`, `set()`, `subscribe()`) and `Context<T>` (`setContext()`, `inheritContext()`) backed by Svet's WASM `ReactiveGraph`.
 *
 * 3. **Forma Standard Library (`formaStdlib`) & Pipe Operator (`|>`)**:
 *    - Implements Forma's complete `stdlib`: math (`clamp`, `lerp`, `remap`, `sum`, `avg`), strings & i18n (`plural`, `upper`, `lower`, `capitalize`, `truncate`, `padStart`), collections (`filter`, `map`, `join`, `take`, `skip`, `unique`), and WCAG color functions (`lighten`, `darken`, `alpha`, `luminance`, `contrast`).
 */

import { For, insertChild, Show } from './dom.ts';
import {
  batch,
  computed,
  createRoot,
  effect,
  getCurrentOwner,
  isOwnerScopeDisposed,
  onCleanup,
  type ReadonlySignal,
  signal,
  type Signal,
  untrack,
} from './reactivity.ts';

// ============================================================================
// 1. Forma `Property<T>` & `Context<T>` (Rust ViewModel Binding Compatibility)
// ============================================================================

export interface FormaProperty<T> {
  readonly signal: Signal<T>;
  get(): T;
  set(val: T): void;
  update(fn: (prev: T) => T): void;
  subscribe(listener: (val: T) => void): () => void;
}

export function createFormaProperty<T>(initial: T): FormaProperty<T> {
  const sig = signal<T>(initial);
  return {
    signal: sig,
    get() {
      return sig();
    },
    set(val: T) {
      sig.set(val);
    },
    update(fn: (prev: T) => T) {
      sig.update(fn);
    },
    subscribe(listener: (val: T) => void) {
      let first = true;
      const handle = effect(() => {
        const v = sig();
        if (first) {
          first = false;
          return;
        }
        listener(v);
      });
      return () => handle.dispose();
    },
  };
}

export interface FormaContextHolder<T> {
  readonly current: ReadonlySignal<T>;
  setContext(next: T): void;
  inheritContext(): void;
}

export function createFormaContext<T>(
  initial: T,
  parentContext?: FormaContextHolder<T>,
): FormaContextHolder<T> {
  const localOverride = signal<T | null>(null);
  const isOverridden = signal(false);

  const current = computed<T>(() => {
    if (isOverridden()) {
      return localOverride() as T;
    }
    return parentContext ? parentContext.current() : initial;
  });

  return {
    current,
    setContext(next: T) {
      batch(() => {
        localOverride.set(next);
        isOverridden.set(true);
      });
    },
    inheritContext() {
      isOverridden.set(false);
    },
  };
}

// ============================================================================
// 2. Forma Standard Library (`formaStdlib`) & Submodule Exports
// ============================================================================

import { formaStdlib, transformFormaPipes } from './forma-stdlib.ts';
import {
  type FormaEvalScope,
  type InterpPart,
  type ExprAst,
  FORBIDDEN_MEMBER_KEYS,
  isSafePatternRegex,
  parseFormaExprAst,
  evalAstNode,
  evaluateFormaExpression,
  compileFormaPatternMatcher,
  doesFormaPatternMatch,
} from './forma-expr.ts';
import {
  structuredCloneIfObject,
  type SpringSignal,
  flushSpringFrame,
  createSpring,
  createPersistedSignal,
  type FormFieldRule,
  type ReactiveForm,
  createForm,
  createContractForm,
} from './forma-reactive.ts';
import {
  assertFormaAstContracts,
  parseFormaSource,
  snapshotFormaAstImpl,
  type FormaAstNode,
  type FormaDocumentAst,
  type FormaMatchArm,
  type FormaPropConstraint,
} from './forma-ast.ts';
import {
  compileFormaToJsModule,
  validateFormaSource,
  type FormaDiagnostic,
  type FormaModuleOptions,
  type FormaValidationReport,
} from './forma-aot.ts';
import { renderFormaBuiltinElement } from './forma-dom.ts';

export {
  formaStdlib,
  transformFormaPipes,
  type FormaEvalScope,
  type InterpPart,
  type ExprAst,
  evaluateFormaExpression,
  compileFormaPatternMatcher,
  doesFormaPatternMatch,
  type SpringSignal,
  flushSpringFrame,
  createSpring,
  createPersistedSignal,
  type FormFieldRule,
  type ReactiveForm,
  createForm,
  createContractForm,
  parseFormaSource,
  type FormaAstNode,
  type FormaDocumentAst,
  type FormaMatchArm,
  type FormaPropConstraint,
  validateFormaSource,
  compileFormaToJsModule,
  type FormaDiagnostic,
  type FormaModuleOptions,
  type FormaValidationReport,
};

// ============================================================================
// 3. Forma `.ui` Runtime Compiler & Lifecycle (`compileFormaComponent`, `FormaHost`)
// ============================================================================

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

export interface FormaSlotMap {
  defaultSlot?: Node | (() => Node);
  namedSlots?: Record<string, Node | (() => Node)>;
}

const _formaComponentRegistry = new Map<string, string>();

/** What an extension element renderer receives for one `Tag { ... }` occurrence. */
export interface FormaElementContext {
  tag: string;
  /** Raw property expressions of the element, after `apply` styles and `forward props` are merged in. */
  props: Record<string, string>;
  /** Evaluation scope for `evaluateFormaExpression` / `evalAstNode`; reads inside effects are tracked. */
  scope: FormaEvalScope;
  /** Makes the element reachable through `instance.getElementByKey(key)`. */
  registerKey(key: string, element: HTMLElement): void;
}
export type FormaElementRenderer = (context: FormaElementContext) => Node;

const FORMA_3D_TAGS = new Set(['Scene3D', 'OrbitCube3D', 'Viewport3D']);
// Shared across copies of the package, like the other cross-copy registries in this file.
const formaElementHost = globalThis as unknown as Record<
  symbol,
  Map<string, FormaElementRenderer> | undefined
>;
const formaElementRenderers: Map<string, FormaElementRenderer> = (formaElementHost[
  Symbol.for('svet.forma.elements.v1')
] ??= new Map<string, FormaElementRenderer>());

/**
 * Registers a renderer for one or more Forma tags that the core runtime does not draw itself.
 * It runs inside the component's reactive owner, so `effect` and `onCleanup` behave as in built-in elements.
 * Returns a function that removes the registration.
 */
export function registerFormaElement(
  tags: string | readonly string[],
  renderer: FormaElementRenderer,
): () => void {
  const names = typeof tags === 'string' ? [tags] : [...tags];
  for (const name of names) {
    if (!/^[A-Z][A-Za-z0-9_]*$/.test(name))
      throw new TypeError(`Invalid Forma element name: ${name}`);
    formaElementRenderers.set(name, renderer);
  }
  return () => {
    for (const name of names) {
      if (formaElementRenderers.get(name) === renderer) formaElementRenderers.delete(name);
    }
  };
}

/**
 * Registers a reusable Forma `.ui` component globally so any other `.ui` component
 * can embed `<ComponentName> { propA: ...; }` declaratively.
 */
export function registerFormaComponent(name: string, uiSource: string): void {
  formaSourceParser ??= parseFormaSource;
  _formaComponentRegistry.set(name, uiSource);
}

/** Own the admitted document; caller edits cannot change mounted/future views. */
function snapshotFormaAst(ast: FormaDocumentAst): FormaDocumentAst {
  void assertFormaAstContracts;
  return snapshotFormaAstImpl(ast);
}

export interface FormaCompileOptions {
  props?: Record<string, any> | undefined;
  state?: Record<string, any> | undefined;
  locale?: string | undefined;
  viewModel?: Record<string, FormaProperty<any> | Signal<any>> | undefined;
  slots?: FormaSlotMap | undefined;
  components?: Record<string, string | ((opts?: any) => FormaRuntimeInstance)> | undefined;
  actions?: Record<string, (...args: any[]) => void> | undefined;
  onAction?: ((name: string, args: any[], event?: Event) => void) | undefined;
  onBinding?: ((path: string, value: any) => void) | undefined;
}

/**
 * Instantiates a live, reactive **Svet + WASM** `FormaRuntimeInstance` directly from a pre-parsed
 * `FormaDocumentAst` (0ms runtime parsing overhead) or a raw `.ui` source string.
 */
export function compileFormaAst(
  astOrSource: FormaDocumentAst | string,
  options?: FormaCompileOptions,
): FormaRuntimeInstance {
  return compileFormaComponent(astOrSource, options);
}

// Synchronous factory construction owns every completed Forma instance it creates,
// including instances whose handle is lost when the factory throws.
type FormaConstructionDomain = { active: Set<FormaRuntimeInstance> | null };
const formaConstructionHost = globalThis as unknown as Record<
  symbol,
  FormaConstructionDomain | undefined
>;
const formaConstructionDomain = (formaConstructionHost[Symbol.for('svet.forma.construction.v1')] ??=
  { active: null });
function constructFormaChild(factory: () => FormaRuntimeInstance): FormaRuntimeInstance {
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
function createFormaPublicHandle(implementation: FormaRuntimeInstance): FormaRuntimeInstance {
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
function cachedFormaValue(value: any): Signal<any> {
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

/**
 * Compiles a Forma `.ui` source file or pre-parsed `FormaDocumentAst` into a live, reactive **Svet + WASM** `FormaRuntimeInstance`.
 */
export function compileFormaComponent(
  uiSourceOrAst: string | FormaDocumentAst,
  options?: FormaCompileOptions,
): FormaRuntimeInstance {
  formaSourceParser ??= parseFormaSource;
  return instantiateForma(uiSourceOrAst, options);
}

/**
 * Instantiates a component from an AST produced at build time (`compileFormaToJsModule`, the Vite plugin).
 * Unlike `compileFormaComponent` it never references the `.ui` parser, so a bundle that only ships
 * precompiled views does not carry it. Child components passed as source text and `hotReload(source)`
 * still work once anything in the application has pulled the parser in (`compileFormaComponent`,
 * `registerFormaComponent`, `FormaHost` with `source`); otherwise they must be ASTs.
 */
export function instantiateFormaAst(
  ast: FormaDocumentAst,
  options?: FormaCompileOptions,
): FormaRuntimeInstance {
  if (typeof ast === 'string')
    throw new TypeError(
      'instantiateFormaAst expects a pre-parsed AST; use compileFormaComponent for source text',
    );
  return instantiateForma(ast, options);
}

// Set by the entry points that accept `.ui` source text. The runtime below only reaches the parser through it.
let formaSourceParser: ((source: string) => FormaDocumentAst) | null = null;
function formaAstFromSource(source: string): FormaDocumentAst {
  if (!formaSourceParser) {
    throw new Error(
      'Forma source text needs the parser: create the component with compileFormaComponent() or pass a pre-parsed AST',
    );
  }
  return formaSourceParser(source);
}

function instantiateForma(
  uiSourceOrAst: string | FormaDocumentAst,
  options?: FormaCompileOptions,
): FormaRuntimeInstance {
  let constructing = true,
    disposed = false;
  const constructionDisposers: Array<() => void> = [];
  const trackConstruction = <T extends { dispose(): void }>(value: T): T => {
    if (disposed) {
      try {
        value.dispose();
      } finally {
        throw new Error('Forma instance is disposed');
      }
    }
    if (constructing) constructionDisposers.push(() => value.dispose());
    return value;
  };
  const ownSignal = <T>(value: T): Signal<T> => {
    if (disposed) throw new Error('Forma instance is disposed');
    return trackConstruction(signal(value));
  };
  const ownComputed = <T>(fn: () => T): ReadonlySignal<T> => {
    if (disposed) throw new Error('Forma instance is disposed');
    return trackConstruction(computed(fn));
  };
  try {
    const initialAst =
      typeof uiSourceOrAst === 'string'
        ? formaAstFromSource(uiSourceOrAst)
        : snapshotFormaAst(uiSourceOrAst);
    const componentName = initialAst.componentName;

    const tokens: Record<string, any> = {};
    const i18nDictionaries: Record<string, Record<string, string>> = {};
    const styles: Record<string, Record<string, string>> = {};
    const propConstraints: Array<FormaPropConstraint & { compiledPattern?: RegExp | undefined }> =
      [];
    const initialStateValues: Record<string, any> = Object.create(null);
    const stateSignals: Record<string, Signal<any>> = Object.create(null);
    const propSignals: Record<string, Signal<any>> = Object.create(null);
    const ownedStateSignals = new Map<string, Signal<any>>();
    const ownedPropSignals = new Map<string, Signal<any>>();
    const letExpressions: Array<{ name: string; expr: string }> = [];
    const shortcuts: Array<{ combo: string; action: string }> = [];
    const rootAstNodes: FormaAstNode[] = [];
    const hmrRevision = ownSignal(0);
    let updatingDocument = false;
    type PreparedView = { commit(): void; abort(): void };
    const prepareViews = new Set<() => PreparedView>();
    const renderDisposers = new Set<() => void>();
    const mountDisposers = new Set<() => void>();
    const assertLive = () => {
      if (disposed) throw new Error('Forma instance is disposed');
    };
    const ensureStateSignal = (key: string, initialVal: any = undefined): Signal<any> => {
      if (FORBIDDEN_MEMBER_KEYS.has(key)) {
        return cachedFormaValue(undefined);
      }
      let sig = stateSignals[key];
      if (!sig) {
        sig = ownSignal(initialVal);
        ownedStateSignals.set(key, sig);
        stateSignals[key] = sig;
      }
      return sig;
    };

    // Bind external Rust/TS ViewModel properties if provided
    if (options?.viewModel) {
      for (const [k, propOrSig] of Object.entries(options.viewModel)) {
        if ('signal' in propOrSig) {
          stateSignals[k] = propOrSig.signal;
          initialStateValues[k] = propOrSig.signal.peek();
        } else if (typeof propOrSig === 'function') {
          stateSignals[k] = propOrSig;
          initialStateValues[k] = propOrSig.peek();
        }
      }
    }

    // Bind initial external `options.state` fields or signals if provided
    if (options?.state) {
      for (const [k, valOrSig] of Object.entries(options.state)) {
        if (FORBIDDEN_MEMBER_KEYS.has(k)) continue;
        if (
          valOrSig &&
          typeof valOrSig === 'object' &&
          'signal' in valOrSig &&
          typeof (valOrSig as any).signal === 'function'
        ) {
          const extSig = (valOrSig as FormaProperty<any>).signal;
          stateSignals[k] = extSig;
          initialStateValues[k] = extSig.peek();
        } else if (typeof valOrSig === 'function' && '__svet_sig' in (valOrSig as any)) {
          const extSig = valOrSig as Signal<any>;
          stateSignals[k] = extSig;
          initialStateValues[k] = extSig.peek();
        } else {
          const cloned = structuredCloneIfObject(valOrSig);
          initialStateValues[k] = cloned;
          ensureStateSignal(k, cloned);
        }
      }
    }

    const applyDocumentAst = (
      docAst: FormaDocumentAst,
      isHotReload = false,
      readOptionProps = true,
    ) => {
      for (const k of Object.keys(tokens)) delete tokens[k];
      Object.assign(tokens, structuredCloneIfObject(docAst.tokens));

      for (const k of Object.keys(i18nDictionaries)) delete i18nDictionaries[k];
      Object.assign(i18nDictionaries, structuredCloneIfObject(docAst.i18nDictionaries));

      for (const k of Object.keys(styles)) delete styles[k];
      Object.assign(styles, structuredCloneIfObject(docAst.styles));

      propConstraints.length = 0;
      for (const c of docAst.propConstraints) {
        const defVal =
          readOptionProps &&
          options?.props &&
          Object.prototype.hasOwnProperty.call(options.props, c.name)
            ? options.props[c.name]
            : c.defaultValue;
        let compiledPattern: RegExp | undefined;
        if (c.patternSource && isSafePatternRegex(c.patternSource)) {
          try {
            compiledPattern = new RegExp(c.patternSource);
          } catch {
            compiledPattern = undefined;
          }
        }
        propConstraints.push({
          ...c,
          defaultValue: defVal,
          compiledPattern,
        });
        if (!propSignals[c.name]) {
          const sig = ownSignal(defVal);
          ownedPropSignals.set(c.name, sig);
          propSignals[c.name] = sig;
        }
      }

      for (const [name, initVal] of Object.entries(docAst.initialStateValues)) {
        if (!isHotReload || !(name in initialStateValues)) {
          initialStateValues[name] = structuredCloneIfObject(initVal);
        }
        if (!stateSignals[name]) {
          const sig = ownSignal(structuredCloneIfObject(initVal));
          ownedStateSignals.set(name, sig);
          stateSignals[name] = sig;
        }
      }

      letExpressions.length = 0;
      letExpressions.push(...docAst.letExpressions);

      shortcuts.length = 0;
      shortcuts.push(...docAst.shortcuts);

      rootAstNodes.length = 0;
      rootAstNodes.push(...docAst.rootAstNodes);
    };

    applyDocumentAst(initialAst, false);

    const locale = ownSignal(options?.locale ?? Object.keys(i18nDictionaries)[0] ?? 'ru');
    const activeI18n = ownComputed(() => {
      hmrRevision();
      return i18nDictionaries[locale()] ?? {};
    });

    // Reactive `isValid` computed signal checking all `#[range]` and `#[pattern]` constraints!
    const isValid = ownComputed(() => {
      hmrRevision();
      for (const c of propConstraints) {
        const val = propSignals[c.name]?.();
        if (c.required && (val === null || val === undefined || val === '')) {
          return false;
        }
        if (c.range && typeof val === 'number') {
          if (val < c.range[0] || val > c.range[1]) return false;
        }
        if (c.compiledPattern && typeof val === 'string') {
          if (!c.compiledPattern.test(val)) return false;
        }
      }
      return true;
    });

    const computedLets: Record<string, ReadonlySignal<any>> = Object.create(null);
    const ownedComputedLets = new Set<ReadonlySignal<any>>();
    const evalScope: FormaEvalScope = {
      state: stateSignals,
      props: propSignals,
      computedLets,
      tokens,
      i18n: activeI18n,
      isValid,
      readState: (key: string) => {
        if (disposed || FORBIDDEN_MEMBER_KEYS.has(key)) return undefined;
        if (updatingDocument) {
          const existing = stateSignals[key];
          return existing ? existing() : undefined;
        }
        return ensureStateSignal(key, undefined)();
      },
    };

    const rebuildComputedLets = (retirePrevious = true) => {
      if (retirePrevious)
        for (const oldComp of ownedComputedLets) {
          oldComp?.dispose?.();
        }
      ownedComputedLets.clear();
      for (const k of Object.keys(computedLets)) delete computedLets[k];
      for (const l of letExpressions) {
        const comp = ownComputed(() => {
          hmrRevision();
          return evaluateFormaExpression(l.expr, evalScope);
        });
        ownedComputedLets.add(comp);
        computedLets[l.name] = comp;
      }
    };
    rebuildComputedLets();

    // Built-in Forma `state.*` / `actions.*` mutators
    const actions: FormaStateMutator = {
      toggle(field: string) {
        if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
        ensureStateSignal(field, false).update(v => !v);
      },
      set(field: string, value: any) {
        if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
        ensureStateSignal(field, value).set(value);
      },
      increment(field: string, delta = 1) {
        if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
        ensureStateSignal(field, 0).update(v => Number(v || 0) + delta);
      },
      decrement(field: string, delta = 1) {
        if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
        ensureStateSignal(field, 0).update(v => Number(v || 0) - delta);
      },
      push(field: string, item: any) {
        if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
        ensureStateSignal(field, []).update(arr => (Array.isArray(arr) ? [...arr, item] : [item]));
      },
      remove(field: string, index: number) {
        const sig = stateSignals[field];
        if (sig) {
          sig.update(arr => (Array.isArray(arr) ? arr.filter((_, idx) => idx !== index) : []));
        }
      },
      reset(field?: string) {
        if (field) {
          const targetSig = stateSignals[field];
          if (targetSig && field in initialStateValues) {
            targetSig.set(structuredCloneIfObject(initialStateValues[field]));
          }
        } else {
          batch(() => {
            for (const [k, initV] of Object.entries(initialStateValues)) {
              stateSignals[k]?.set(structuredCloneIfObject(initV));
            }
          });
        }
      },
    };

    const eventListeners = new Map<string, Set<(...args: any[]) => void>>();
    let keyedElements = new Map<string, HTMLElement>();
    const viewKeyMaps = new Set<Map<string, HTMLElement>>();

    const emitEvent = (eventName: string, ...args: any[]) => {
      if (disposed) return;
      const set = eventListeners.get(eventName);
      if (set) {
        for (const fn of set) {
          fn(...args);
        }
      }
    };

    const executeFormaAction = (
      actionStr: string,
      locals?: Record<string, any>,
      domEvent?: Event,
    ) => {
      const trimmed = actionStr.trim();
      const callScope = baseScopeWithLocals(locals);

      // Parse action expression via `parseFormaExprAst` so commas inside strings `'Иванов, И.И.'`
      // or arrays `['a', 'b']` or nested calls `clamp(1, 2, 3)` are handled with 100% accuracy!
      const ast = parseFormaExprAst(trimmed);
      if (
        ast.type === 'call' &&
        ast.callee.type === 'member' &&
        ast.callee.object.type === 'ident'
      ) {
        const targetNamespace = ast.callee.object.name;
        const methodName = ast.callee.property;
        const parsedArgs = ast.args.map(argNode => evalAstNode(argNode, callScope));

        if (targetNamespace === 'state' || targetNamespace === 'actions') {
          const customFn = options?.actions?.[methodName];
          if (typeof customFn === 'function') {
            customFn(...parsedArgs, domEvent);
          } else {
            const method = methodName as keyof FormaStateMutator;
            if (typeof actions[method] === 'function') {
              (actions[method] as (...a: any[]) => void)(...parsedArgs);
            }
          }
          options?.onAction?.(methodName, parsedArgs, domEvent);
          emitEvent(methodName, ...parsedArgs, domEvent);
          return;
        }

        if (targetNamespace === 'events') {
          options?.onAction?.(methodName, parsedArgs, domEvent);
          emitEvent(methodName, ...parsedArgs);
          return;
        }
      } else if (ast.type === 'call' && ast.callee.type === 'ident') {
        const methodName = ast.callee.name;
        const parsedArgs = ast.args.map(argNode => evalAstNode(argNode, callScope));
        options?.actions?.[methodName]?.(...parsedArgs, domEvent);
        options?.onAction?.(methodName, parsedArgs, domEvent);
        emitEvent(methodName, ...parsedArgs);
      }
    };

    const baseScopeWithLocals = (locals?: Record<string, any>): FormaEvalScope =>
      locals === undefined ? evalScope : { ...evalScope, locals };

    const renderAstChildren = (
      children: FormaAstNode[],
      locals?: Record<string, any>,
    ): DocumentFragment => {
      const frag = document.createDocumentFragment();
      for (const child of children) {
        insertChild(frag, renderAstNode(child, locals));
      }
      return frag;
    };

    const renderAstNode = (ast: FormaAstNode, locals?: Record<string, any>): Node => {
      assertLive();
      const localScope = baseScopeWithLocals(locals);

      if (ast.kind === 'slot') {
        const slotName = ast.slotName!;
        const customSlot = options?.slots?.namedSlots?.[slotName];
        if (customSlot) {
          const frag = document.createDocumentFragment();
          const node = typeof customSlot === 'function' ? customSlot() : customSlot;
          insertChild(frag, node);
          return frag;
        }
        return renderAstChildren(ast.children, locals);
      }

      if (ast.kind === 'contentPresenter') {
        const defaultContent = options?.slots?.defaultSlot;
        if (defaultContent) {
          const frag = document.createDocumentFragment();
          const node = typeof defaultContent === 'function' ? defaultContent() : defaultContent;
          insertChild(frag, node);
          return frag;
        }
        return renderAstChildren(ast.children, locals);
      }

      if (ast.kind === 'match') {
        const frag = document.createDocumentFragment();
        const anchor = document.createComment('svet-forma-match');
        frag.appendChild(anchor);
        let currentNodes: Node[] = [];
        let disposeArm: (() => void) | null = null;

        const clearArm = () => {
          if (disposeArm) {
            disposeArm();
            disposeArm = null;
          }
          for (const n of currentNodes) {
            if (n.parentNode) n.parentNode.removeChild(n);
          }
          currentNodes = [];
        };

        const matchAst = parseFormaExprAst(ast.matchExpr!);
        const compiledArms = (ast.matchArms ?? []).map(arm => ({
          arm,
          matches: compileFormaPatternMatcher(arm.pattern),
        }));
        effect(() => {
          const val = evalAstNode(matchAst, localScope);
          clearArm();
          let matchedArm: (typeof compiledArms)[number]['arm'] | undefined;
          for (const entry of compiledArms) {
            if (entry.matches(val)) {
              matchedArm = entry.arm;
              break;
            }
          }
          if (matchedArm && matchedArm.childNodes.length > 0) {
            createRoot(dispose => {
              disposeArm = dispose;
              const temp = document.createDocumentFragment();
              for (const ch of matchedArm.childNodes) {
                insertChild(
                  temp,
                  untrack(() => renderAstNode(ch, locals)),
                );
              }
              const list = Array.from(temp.childNodes);
              const parent = anchor.parentNode ?? frag;
              for (const n of list) {
                parent.insertBefore(n, anchor);
              }
              currentNodes = list;
            });
          }
        });

        return frag;
      }

      if (ast.kind === 'if') {
        const condAst = parseFormaExprAst(ast.conditionExpr!);
        return Show({
          when: () => Boolean(evalAstNode(condAst, localScope)),
          children: () => renderAstChildren(ast.children, locals),
          fallback:
            ast.elseChildren && ast.elseChildren.length > 0
              ? () => renderAstChildren(ast.elseChildren!, locals)
              : undefined,
        });
      }

      if (ast.kind === 'for') {
        const itemVar = ast.itemVar!;
        const listAst = parseFormaExprAst(ast.listExpr!);
        const keyAst = ast.keyExpr ? parseFormaExprAst(ast.keyExpr) : null;
        const keyLocals: Record<string, any> = locals
          ? Object.defineProperties({}, Object.getOwnPropertyDescriptors(locals))
          : {};
        const keyScope = keyAst ? baseScopeWithLocals(keyLocals) : null;
        const listAccessor = () => {
          const res = evalAstNode(listAst, localScope);
          return Array.isArray(res) ? res : [];
        };

        return Show({
          when: () => listAccessor().length > 0,
          fallback:
            ast.emptyChildren && ast.emptyChildren.length > 0
              ? () => renderAstChildren(ast.emptyChildren!, locals)
              : undefined,
          children: () =>
            For({
              each: listAccessor,
              keyFn: keyAst
                ? item => {
                    keyLocals[itemVar] = item;
                    return evalAstNode(keyAst, keyScope!);
                  }
                : (_item, idx) => idx,
              children: (_item, _index, current) => {
                const childLocals = locals
                  ? Object.defineProperties({}, Object.getOwnPropertyDescriptors(locals))
                  : {};
                Object.defineProperty(childLocals, itemVar, {
                  get: current,
                  enumerable: true,
                  configurable: true,
                });
                const frag = renderAstChildren(ast.children, childLocals);
                return frag.childNodes.length === 1 ? frag.firstChild! : frag;
              },
            }),
        });
      }

      // Element node: priority order -> `apply` styles -> `forward props` -> explicit properties -> `match` property groups
      const mergedProps: Record<string, string> = {};
      for (const styleName of ast.appliedStyles) {
        const styleProps = styles[styleName];
        if (styleProps) {
          Object.assign(mergedProps, styleProps);
        }
      }
      for (const fwdName of ast.forwardedProps) {
        if (fwdName in propSignals) {
          mergedProps[fwdName] = `props.${fwdName}`;
        }
      }
      Object.assign(mergedProps, ast.properties);

      const tag = ast.tag ?? 'Frame';

      // Uppercase `Slot { name: "fileMenu"; ... }` or `Slot { name: "default"; ... }`
      if (tag === 'Slot') {
        const slotName = mergedProps.name
          ? String(evaluateFormaExpression(mergedProps.name, localScope) ?? 'default')
          : 'default';
        const customSlot =
          options?.slots?.namedSlots?.[slotName] ??
          (slotName === 'default' ? options?.slots?.defaultSlot : undefined);
        if (customSlot) {
          const frag = document.createDocumentFragment();
          const node = typeof customSlot === 'function' ? customSlot() : customSlot;
          insertChild(frag, node);
          return frag;
        }
        return renderAstChildren(ast.children, locals);
      }

      // Elements rendered by an extension module (`registerFormaElement`), e.g. the 3D tags of `forma-3d.ts`.
      const extensionRenderer = formaElementRenderers.get(tag);
      if (extensionRenderer) {
        return extensionRenderer({
          tag,
          props: mergedProps,
          scope: localScope,
          registerKey: (key, element) => keyedElements.set(key, element),
        });
      }
      if (FORMA_3D_TAGS.has(tag)) {
        throw new Error(
          `Forma <${tag}> needs the 3D module: add \`import 'svet-engine/forma-3d'\` (the default \`svet-engine\` entry does it for you)`,
        );
      }

      // Check if `tag` references a registered or passed child Forma `.ui` component!
      const customCompDef = options?.components?.[tag] ?? _formaComponentRegistry.get(tag);
      if (customCompDef) {
        const initialChildProps: Record<string, any> = {};
        const parsedChildProps: Array<[string, ExprAst]> = [];
        for (const [pKey, pExpr] of Object.entries(mergedProps)) {
          if (pKey === 'key') continue;
          const pAst = parseFormaExprAst(pExpr);
          parsedChildProps.push([pKey, pAst]);
          initialChildProps[pKey] = untrack(() => evalAstNode(pAst, localScope));
        }

        const childOwner = getCurrentOwner();
        const childInst = constructFormaChild(() =>
          typeof customCompDef === 'function'
            ? customCompDef({
                props: initialChildProps,
                locale: locale.peek(),
                components: options?.components,
              })
            : instantiateForma(customCompDef, {
                props: initialChildProps,
                locale: locale.peek(),
                components: options?.components,
              }),
        );

        // A callback may retire this view before returning its newly created child.
        // A closed owner cannot register cleanup, so release that child immediately.
        if (disposed || isOwnerScopeDisposed(childOwner)) {
          try {
            childInst.dispose();
          } finally {
            assertLive();
          }
          throw new Error('Forma view owner is disposed');
        }
        // The child already owns signals even if synchronization or rendering fails.
        onCleanup(() => childInst.dispose());
        assertLive();

        // Keep child `locale` and `props` reactively synchronized with parent signals!
        effect(() => {
          childInst.locale.set(locale());
        });
        for (const [pKey, pAst] of parsedChildProps) {
          const childPropSig = childInst.props[pKey];
          if (childPropSig) {
            effect(() => {
              const nextVal = evalAstNode(pAst, localScope);
              childPropSig.set(nextVal);
            });
          }
        }

        // Bridge child events (`eventName -> actionExpr`) to parent action dispatcher
        for (const [evtName, actionExpr] of Object.entries(ast.eventHandlers)) {
          childInst.onEvent(evtName, (...evtArgs: any[]) => {
            executeFormaAction(actionExpr, {
              ...(locals ?? {}),
              $event: evtArgs[0],
              $args: evtArgs,
            });
          });
        }

        const wrapperEl = document.createElement('div');
        wrapperEl.setAttribute('data-forma-component', tag);
        if (mergedProps.key) {
          const keyVal = String(evaluateFormaExpression(mergedProps.key, localScope));
          wrapperEl.setAttribute('data-forma-key', keyVal);
          keyedElements.set(keyVal, wrapperEl);
        }
        const childView = childInst.render();
        assertLive();
        wrapperEl.appendChild(childView);
        return wrapperEl;
      }

      return renderFormaBuiltinElement({
        tag,
        ast,
        mergedProps,
        localScope,
        locals,
        registerKey: (key, element) => keyedElements.set(key, element),
        ensureStateSignal,
        onBinding: options?.onBinding,
        executeAction: executeFormaAction,
        renderChild: renderAstNode,
      });
    };

    const instance: FormaRuntimeInstance = {
      componentName,
      state: stateSignals,
      props: propSignals,
      computedLets,
      locale,
      isValid,
      actions,
      setState(partial: Record<string, any>) {
        assertLive();
        batch(() => {
          for (const [k, v] of Object.entries(partial)) {
            if (FORBIDDEN_MEMBER_KEYS.has(k)) continue;
            ensureStateSignal(k, v).set(v);
          }
        });
      },
      onEvent(eventName: string, handler: (...args: any[]) => void) {
        assertLive();
        let set = eventListeners.get(eventName);
        if (!set) {
          set = new Set();
          eventListeners.set(eventName, set);
        }
        set.add(handler);
        return () => set!.delete(handler);
      },
      emitEvent,
      getElementByKey(key: string) {
        return keyedElements.get(key);
      },
      exportHmrSnapshot(): FormaHmrSnapshot {
        const stateSnap: Record<string, any> = {};
        for (const [k, sig] of Object.entries(stateSignals)) {
          stateSnap[k] = structuredCloneIfObject(sig.peek());
        }
        const propSnap: Record<string, any> = {};
        for (const [k, sig] of Object.entries(propSignals)) {
          propSnap[k] = structuredCloneIfObject(sig.peek());
        }
        return {
          componentName,
          locale: locale.peek(),
          state: stateSnap,
          props: propSnap,
        };
      },
      restoreHmrSnapshot(snapshot: FormaHmrSnapshot) {
        assertLive();
        batch(() => {
          if (snapshot.locale) {
            locale.set(snapshot.locale);
          }
          if (snapshot.state) {
            for (const [k, v] of Object.entries(snapshot.state)) {
              stateSignals[k]?.set(structuredCloneIfObject(v));
            }
          }
          if (snapshot.props) {
            for (const [k, v] of Object.entries(snapshot.props)) {
              propSignals[k]?.set(structuredCloneIfObject(v));
            }
          }
        });
      },
      hotReload(nextUiSourceOrAst: string | FormaDocumentAst) {
        assertLive();
        if (updatingDocument) throw new Error('Forma document update is already in progress');
        updatingDocument = true;
        try {
          const nextAst =
            typeof nextUiSourceOrAst === 'string'
              ? formaAstFromSource(nextUiSourceOrAst)
              : snapshotFormaAst(nextUiSourceOrAst);
          assertLive();
          const previousAst: FormaDocumentAst = {
            componentName,
            tokens: { ...tokens },
            i18nDictionaries: { ...i18nDictionaries },
            styles: { ...styles },
            propConstraints: [...propConstraints],
            initialStateValues: { ...initialStateValues },
            letExpressions: [...letExpressions],
            shortcuts: [...shortcuts],
            rootAstNodes: [...rootAstNodes],
          };
          const previousLets = { ...computedLets };
          const previousOwnedLets = new Set(ownedComputedLets);
          const previousState = Object.fromEntries(
            Object.entries(stateSignals).map(([key, sig]) => [key, sig.peek()]),
          );
          const previousProps = Object.fromEntries(
            Object.entries(propSignals).map(([key, sig]) => [key, sig.peek()]),
          );
          const previousLocale = locale.peek();
          const prepared: PreparedView[] = [];
          let committing = false,
            updateFailed = false,
            previousLetsDetached = false;
          try {
            assertLive();
            batch(() => {
              try {
                applyDocumentAst(nextAst, true);
                assertLive();
                previousLetsDetached = true;
                rebuildComputedLets(false);
                assertLive();
                hmrRevision.update(r => r + 1);
                for (const prepare of [...prepareViews]) {
                  if (prepareViews.has(prepare)) prepared.push(prepare());
                }
                committing = true;
                for (const view of prepared) view.commit();
              } catch (error) {
                for (const view of prepared.reverse()) view.abort();
                if (!committing && !disposed) {
                  for (const [key, sig] of Object.entries(stateSignals)) {
                    if (disposed) throw error;
                    if (!(key in previousState)) {
                      const owned = ownedStateSignals.get(key);
                      ownedStateSignals.delete(key);
                      try {
                        owned?.dispose();
                      } catch {
                        /* Keep the preparation failure while draining owned candidates. */
                      }
                      delete stateSignals[key];
                    } else sig.set(previousState[key]);
                  }
                  for (const [key, sig] of Object.entries(propSignals)) {
                    if (disposed) throw error;
                    if (!(key in previousProps)) {
                      const owned = ownedPropSignals.get(key);
                      ownedPropSignals.delete(key);
                      try {
                        owned?.dispose();
                      } catch {
                        /* Drain owned props without replacing the preparation failure. */
                      }
                      delete propSignals[key];
                    } else sig.set(previousProps[key]);
                  }
                  if (disposed) throw error;
                  for (const comp of ownedComputedLets) {
                    if (!previousOwnedLets.has(comp)) {
                      try {
                        comp.dispose();
                      } catch {
                        /* Preserve the preparation failure and drain siblings. */
                      }
                    }
                  }
                  if (disposed) throw error;
                  ownedComputedLets.clear();
                  for (const comp of previousOwnedLets) ownedComputedLets.add(comp);
                  previousLetsDetached = false;
                  for (const key of Object.keys(computedLets)) delete computedLets[key];
                  Object.assign(computedLets, previousLets);
                  if (disposed) throw error;
                  for (const key of Object.keys(initialStateValues)) delete initialStateValues[key];
                  // Previous constraints already contain resolved defaults; do not rerun caller getters.
                  applyDocumentAst(previousAst, true, false);
                  if (disposed) throw error;
                  locale.set(previousLocale);
                  if (disposed) throw error;
                  hmrRevision.update(r => r + 1);
                }
                throw error;
              }
            });
          } catch (error) {
            updateFailed = true;
            throw error;
          } finally {
            if ((committing || disposed) && previousLetsDetached) {
              let cleanupFailed = false,
                cleanupError: unknown;
              for (const comp of previousOwnedLets) {
                try {
                  comp.dispose();
                } catch (error) {
                  if (!cleanupFailed) cleanupError = error;
                  cleanupFailed = true;
                }
              }
              if (cleanupFailed && !updateFailed) throw cleanupError;
            }
          }
        } finally {
          updatingDocument = false;
        }
      },
      render() {
        assertLive();
        const frag = document.createDocumentFragment();
        const anchor = document.createComment(`svet-forma-hmr:${componentName}`);
        frag.appendChild(anchor);

        let currentNodes: Node[] = [];
        let disposeView: (() => void) | null = null;
        let currentViewKeys: Map<string, HTMLElement> | null = null;
        let renderRetired = false;
        let pendingViewDispose: (() => void) | null = null;
        let abortPrepared: (() => void) | null = null;
        const assertViewLive = () => {
          assertLive();
          if (renderRetired) throw new Error('Forma view is disposed');
        };

        const clearCurrentView = () => {
          if (disposeView) {
            const stop = disposeView;
            disposeView = null;
            stop();
          }
          for (const n of currentNodes) {
            if (n.parentNode) n.parentNode.removeChild(n);
          }
          currentNodes = [];
          if (currentViewKeys) {
            viewKeyMaps.delete(currentViewKeys);
            if (keyedElements === currentViewKeys) {
              keyedElements = new Map();
              for (const remaining of viewKeyMaps) keyedElements = remaining;
            }
            currentViewKeys = null;
          }
        };

        const mountCurrentView = () => {
          const stage: { dispose: (() => void) | null } = { dispose: null };
          let stagedNodes: Node[] = [];
          const previousKeys = keyedElements,
            nextKeys = new Map<string, HTMLElement>();
          keyedElements = nextKeys;
          try {
            createRoot(dispose => {
              stage.dispose = dispose;
              pendingViewDispose = dispose;
              const temp = document.createDocumentFragment();
              for (const nodeAst of rootAstNodes) {
                insertChild(
                  temp,
                  untrack(() => renderAstNode(nodeAst)),
                );
                assertViewLive();
              }
              if (shortcuts.length > 0 && typeof window !== 'undefined') {
                const compiledShortcuts = shortcuts.map(sc => {
                  const parts = sc.combo.split('+');
                  const keyLower = (parts[parts.length - 1] ?? '').toLowerCase();
                  const needsMod =
                    parts.includes('Mod') || parts.includes('Ctrl') || parts.includes('Meta');
                  return { keyLower, needsMod, action: sc.action };
                });
                const onKeyDown = (e: KeyboardEvent) => {
                  const modPressed = e.metaKey || e.ctrlKey;
                  const keyLower = e.key.toLowerCase();
                  for (const sc of compiledShortcuts) {
                    if (sc.needsMod && !modPressed) continue;
                    if (keyLower === sc.keyLower) {
                      e.preventDefault();
                      executeFormaAction(sc.action);
                    }
                  }
                };
                window.addEventListener('keydown', onKeyDown);
                onCleanup(() => window.removeEventListener('keydown', onKeyDown));
              }
              stagedNodes = Array.from(temp.childNodes);
              const parent = anchor.parentNode ?? frag;
              for (const n of stagedNodes) {
                parent.insertBefore(n, anchor);
                assertViewLive();
              }
            });
            let settled = false;
            const prepared = {
              commit() {
                if (settled) return;
                assertViewLive();
                clearCurrentView();
                assertViewLive();
                settled = true;
                abortPrepared = null;
                disposeView = stage.dispose;
                currentNodes = stagedNodes;
                keyedElements = nextKeys;
                currentViewKeys = nextKeys;
                viewKeyMaps.add(nextKeys);
              },
              abort() {
                if (settled) return;
                settled = true;
                abortPrepared = null;
                stage.dispose?.();
                for (const node of stagedNodes) node.parentNode?.removeChild(node);
                if (!renderRetired) keyedElements = previousKeys;
              },
            };
            abortPrepared = prepared.abort;
            return prepared;
          } catch (error) {
            stage.dispose?.();
            for (const node of stagedNodes) node.parentNode?.removeChild(node);
            if (!renderRetired) keyedElements = previousKeys;
            throw error;
          } finally {
            pendingViewDispose = null;
          }
        };

        mountCurrentView().commit();
        prepareViews.add(mountCurrentView);

        const retireRender = () => {
          if (renderRetired) return;
          renderRetired = true;
          renderDisposers.delete(retireRender);
          prepareViews.delete(mountCurrentView);
          abortPrepared?.();
          pendingViewDispose?.();
          clearCurrentView();
          anchor.remove();
        };
        renderDisposers.add(retireRender);
        onCleanup(retireRender);
        return frag;
      },
      mount(container: HTMLElement) {
        assertLive();
        container.innerHTML = '';
        let disposeRoot!: () => void;
        const rootFrag = createRoot(dispose => {
          disposeRoot = dispose;
          return this.render();
        });

        let unmounted = false;
        const unmount = () => {
          if (unmounted) return;
          unmounted = true;
          mountDisposers.delete(unmount);
          disposeRoot();
        };
        mountDisposers.add(unmount);
        try {
          container.appendChild(rootFrag);
          assertLive();
        } catch (error) {
          unmount();
          throw error;
        }
        return { unmount };
      },
      dispose() {
        if (disposed) return;
        disposed = true;
        let failed = false,
          firstError: unknown;
        const recordFailure = (error: unknown) => {
          if (!failed) firstError = error;
          failed = true;
        };
        const release = (resource: { dispose(): void }) => {
          try {
            resource.dispose();
          } catch (error) {
            recordFailure(error);
          }
        };
        for (const stop of [...mountDisposers]) {
          try {
            stop();
          } catch (error) {
            recordFailure(error);
          }
        }
        for (const stop of [...renderDisposers]) {
          try {
            stop();
          } catch (error) {
            recordFailure(error);
          }
        }
        for (const sig of ownedStateSignals.values()) release(sig);
        ownedStateSignals.clear();
        for (const sig of ownedPropSignals.values()) release(sig);
        ownedPropSignals.clear();
        for (const comp of ownedComputedLets) release(comp);
        ownedComputedLets.clear();
        release(activeI18n);
        release(isValid);
        release(locale);
        release(hmrRevision);
        eventListeners.clear();
        keyedElements.clear();
        viewKeyMaps.clear();
        mountDisposers.clear();
        renderDisposers.clear();
        prepareViews.clear();
        if (failed) throw firstError;
      },
    };
    const publicInstance = createFormaPublicHandle(instance);
    formaConstructionDomain.active?.add(publicInstance);
    constructing = false;
    constructionDisposers.length = 0;
    return publicInstance;
  } catch (error) {
    constructing = false;
    for (const stop of constructionDisposers.reverse()) {
      try {
        stop();
      } catch {
        /* Preserve the constructor error while draining allocations. */
      }
    }
    constructionDisposers.length = 0;
    throw error;
  }
}

/**
 * First-class Svet JSX component (`<FormaHost source={...} />` or `<FormaHost instance={...} />`)
 * that embeds a Forma `.ui` interface directly inside a Svet application tree!
 */
export function FormaHost(props: {
  source?: string;
  instance?: FormaRuntimeInstance;
  props?: Record<string, any> | undefined;
  state?: Record<string, any> | undefined;
  locale?: string | undefined;
  viewModel?: Record<string, FormaProperty<any> | Signal<any>>;
  slots?: FormaSlotMap;
  actions?: Record<string, (...args: any[]) => void>;
  onAction?: (name: string, args: any[], event?: Event) => void;
  onBinding?: (path: string, value: any) => void;
  onReady?: (instance: FormaRuntimeInstance) => void;
}): DocumentFragment {
  const inst =
    props.instance ??
    compileFormaComponent(props.source ?? '', {
      props: props.props,
      state: props.state,
      locale: props.locale,
      viewModel: props.viewModel,
      slots: props.slots,
      actions: props.actions,
      onAction: props.onAction,
      onBinding: props.onBinding,
    });
  if (props.onReady) {
    props.onReady(inst);
  }
  return inst.render();
}
