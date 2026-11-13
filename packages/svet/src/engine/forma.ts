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

import { insertChild } from './dom.ts';
import {
  batch,
  computed,
  createRoot,
  effect,
  onCleanup,
  type ReadonlySignal,
  signal,
  type Signal,
  untrack,
} from './reactivity.ts';

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
  type ExprAst,
  FORBIDDEN_MEMBER_KEYS,
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
import { createFormaActionRunner, createFormaStateMutator } from './forma-actions.ts';
import {
  applyFormaDocument,
  bindExternalFormaState,
  type FormaDocumentState,
} from './forma-document.ts';
import { createFormaRenderer } from './forma-render.ts';
import { formaComponentRegistry } from './forma-registry.ts';

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
  type FormaHmrSnapshot,
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

    bindExternalFormaState({ stateSignals, initialStateValues }, options, ensureStateSignal);

    const documentState: FormaDocumentState = {
      tokens,
      i18nDictionaries,
      styles,
      propConstraints,
      initialStateValues,
      stateSignals,
      propSignals,
      ownedStateSignals,
      ownedPropSignals,
      letExpressions,
      shortcuts,
      rootAstNodes,
    };
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
    const actions = createFormaStateMutator({
      ensureStateSignal,
      stateSignals,
      initialStateValues,
    });

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
      registerKey: (key, element) => keyedElements.set(key, element),
      locale,
      options,
      propSignals,
      styles,
    });

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
