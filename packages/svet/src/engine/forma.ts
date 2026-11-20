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

import { batch, computed, effect, type ReadonlySignal, signal, type Signal } from './reactivity.ts';

// ============================================================================
// 1. Forma `Property<T>` & `Context<T>` (Rust ViewModel Binding Compatibility)
// ============================================================================

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
  type CanonicalExpressionAst,
  type CanonicalFormaValue,
  type ExprAst,
  FORBIDDEN_MEMBER_KEYS,
  canonicalValueToExprString,
  evaluateFormaExpression,
  fromCanonicalFormaExpr,
  compileFormaPatternMatcher,
  doesFormaPatternMatch,
} from './forma-expr.ts';
import {
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
  fromCanonicalFormaAst,
  parseFormaSource,
  registerFormaFrontend,
  snapshotFormaAstImpl,
  type CanonicalFormaDocument,
  type CanonicalFormaNode,
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
import { createFormaActionRunner, createFormaStateMutator } from './forma-actions.ts';
import {
  applyFormaDocument,
  bindExternalFormaState,
  createFormaDocumentState,
  rebuildFormaComputedLets,
  satisfiesFormaConstraints,
} from './forma-document.ts';
import { assembleFormaRuntimeInstance } from './forma-hmr.ts';
import { createFormaRenderer } from './forma-render.ts';
import { formaComponentRegistry } from './forma-registry.ts';
import { createFormaViewLifecycle } from './forma-view.ts';

export {
  registerFormaElement,
  type FormaElementContext,
  type FormaElementRenderer,
} from './forma-registry.ts';
import {
  cachedFormaValue,
  createFormaPublicHandle,
  formaConstructionDomain,
  type FormaCompileOptions,
  type FormaProperty,
  type FormaRuntimeInstance,
  type FormaSlotMap,
} from './forma-instance.ts';

export type {
  FormaCompileOptions,
  FormaHmrSnapshot,
  FormaProperty,
  FormaRuntimeInstance,
  FormaSlotMap,
  FormaStateMutator,
} from './forma-instance.ts';

export {
  formaStdlib,
  transformFormaPipes,
  type FormaEvalScope,
  type InterpPart,
  type ExprAst,
  type CanonicalExpressionAst,
  type CanonicalFormaValue,
  evaluateFormaExpression,
  fromCanonicalFormaExpr,
  canonicalValueToExprString,
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
  fromCanonicalFormaAst,
  registerFormaFrontend,
  type CanonicalFormaDocument,
  type CanonicalFormaNode,
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

/**
 * Registers a reusable Forma `.ui` component globally so any other `.ui` component
 * can embed `<ComponentName> { propA: ...; }` declaratively.
 */
export function registerFormaComponent(name: string, uiSource: string): void {
  formaSourceParser ??= parseFormaSource;
  formaComponentRegistry.set(name, uiSource);
}

/** Own the admitted document; caller edits cannot change mounted/future views. */
function snapshotFormaAst(ast: FormaDocumentAst): FormaDocumentAst {
  void assertFormaAstContracts;
  return snapshotFormaAstImpl(ast);
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

    const documentState = createFormaDocumentState();
    const {
      tokens,
      i18nDictionaries,
      styles,
      propConstraints,
      initialStateValues,
      stateSignals,
      propSignals,
      letExpressions,
      shortcuts,
      rootAstNodes,
    } = documentState;
    const hmrRevision = ownSignal(0);
    let updatingDocument = false;
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
        documentState.ownedStateSignals.set(key, sig);
        stateSignals[key] = sig;
      }
      return sig;
    };

    bindExternalFormaState({ stateSignals, initialStateValues }, options, ensureStateSignal);

    const applyDocumentAst = (
      docAst: FormaDocumentAst,
      isHotReload = false,
      readOptionProps = true,
    ) =>
      applyFormaDocument(documentState, docAst, {
        isHotReload,
        optionProps: readOptionProps ? options?.props : undefined,
        ownSignal,
      });

    applyDocumentAst(initialAst, false);

    const locale = ownSignal(options?.locale ?? Object.keys(i18nDictionaries)[0] ?? 'ru');
    const activeI18n = ownComputed(() => {
      hmrRevision();
      return i18nDictionaries[locale()] ?? {};
    });

    // Reactive `isValid` computed signal checking all `#[range]` and `#[pattern]` constraints!
    const isValid = ownComputed(() => {
      hmrRevision();
      return satisfiesFormaConstraints(propConstraints, propSignals);
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

    const rebuildComputedLets = (retirePrevious = true) =>
      rebuildFormaComputedLets(
        { computedLets, ownedComputedLets, letExpressions },
        expression =>
          ownComputed(() => {
            hmrRevision();
            return evaluateFormaExpression(expression, evalScope);
          }),
        retirePrevious,
      );
    rebuildComputedLets();

    // Built-in Forma `state.*` / `actions.*` mutators
    const actions = createFormaStateMutator({
      ensureStateSignal,
      stateSignals,
      initialStateValues,
    });

    const eventListeners = new Map<string, Set<(...args: any[]) => void>>();
    const emitEvent = (eventName: string, ...args: any[]) => {
      if (disposed) return;
      const set = eventListeners.get(eventName);
      if (set) {
        for (const fn of set) fn(...args);
      }
    };
    const baseScopeWithLocals = (locals?: Record<string, any>): FormaEvalScope =>
      locals === undefined ? evalScope : { ...evalScope, locals };
    const executeFormaAction = createFormaActionRunner({
      actions,
      baseScopeWithLocals,
      emitEvent,
      options,
    });
    const renderAstNode = createFormaRenderer({
      assertLive,
      isDisposed: () => disposed,
      instantiateChild: instantiateForma,
      baseScopeWithLocals,
      ensureStateSignal,
      executeFormaAction,
      registerKey: (key, element) => views.registerKey(key, element),
      locale,
      options,
      propSignals,
      styles,
    });
    const views = createFormaViewLifecycle({
      componentName,
      rootAstNodes,
      shortcuts,
      assertLive,
      renderAstNode,
      executeFormaAction,
    });

    const instance = assembleFormaRuntimeInstance({
      componentName,
      documentState,
      parseAst: nextUiSourceOrAst =>
        typeof nextUiSourceOrAst === 'string'
          ? formaAstFromSource(nextUiSourceOrAst)
          : snapshotFormaAst(nextUiSourceOrAst),
      applyDocumentAst,
      rebuildComputedLets,
      computedLets,
      ownedComputedLets,
      locale,
      activeI18n,
      isValid,
      hmrRevision,
      actions,
      ensureStateSignal,
      assertLive,
      isDisposed: () => disposed,
      markDisposed: () => {
        if (disposed) return false;
        disposed = true;
        return true;
      },
      isUpdating: () => updatingDocument,
      setUpdating: next => {
        updatingDocument = next;
      },
      views,
      eventListeners,
      emitEvent,
    });
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
