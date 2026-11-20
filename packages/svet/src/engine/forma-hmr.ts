import type { FormaAstNode, FormaDocumentAst, FormaPropConstraint } from './forma-ast.ts';
import type { FormaDocumentState } from './forma-document.ts';
import { FORBIDDEN_MEMBER_KEYS } from './forma-expr.ts';
import type {
  FormaHmrSnapshot,
  FormaRuntimeInstance,
  FormaStateMutator,
} from './forma-instance.ts';
import { structuredCloneIfObject } from './forma-reactive.ts';
import { batch, type ReadonlySignal, type Signal } from './reactivity.ts';

export interface PreparedFormaView {
  commit(): void;
  abort(): void;
}

export interface FormaHmrSignalCell {
  peek(): unknown;
  set(value: unknown): void;
  dispose(): void;
}

export interface FormaHmrComputedCell {
  (): unknown;
  peek(): unknown;
  dispose(): void;
}

export interface FormaHmrContext {
  componentName: string;
  parseAst(nextUiSourceOrAst: string | FormaDocumentAst): FormaDocumentAst;
  assertLive(): void;
  isDisposed(): boolean;
  tokens: Record<string, unknown>;
  i18nDictionaries: Record<string, Record<string, string>>;
  styles: Record<string, Record<string, string>>;
  propConstraints: Array<FormaPropConstraint & { compiledPattern?: RegExp | undefined }>;
  initialStateValues: Record<string, unknown>;
  letExpressions: Array<{ name: string; expr: string }>;
  shortcuts: Array<{ combo: string; action: string }>;
  rootAstNodes: FormaAstNode[];
  stateSignals: Record<string, FormaHmrSignalCell>;
  propSignals: Record<string, FormaHmrSignalCell>;
  ownedStateSignals: Map<string, FormaHmrSignalCell>;
  ownedPropSignals: Map<string, FormaHmrSignalCell>;
  computedLets: Record<string, FormaHmrComputedCell>;
  ownedComputedLets: Set<FormaHmrComputedCell>;
  locale: Signal<string>;
  hmrRevision: Signal<number>;
  prepareViews: Set<() => PreparedFormaView>;
  applyDocumentAst(
    docAst: FormaDocumentAst,
    isHotReload?: boolean,
    readOptionProps?: boolean,
  ): void;
  rebuildComputedLets(retirePrevious?: boolean): void;
}

export function exportFormaHmrSnapshot(ctx: {
  componentName: string;
  locale: Signal<string>;
  stateSignals: Record<string, FormaHmrSignalCell>;
  propSignals: Record<string, FormaHmrSignalCell>;
}): FormaHmrSnapshot {
  const stateSnap: Record<string, unknown> = {};
  for (const [k, sig] of Object.entries(ctx.stateSignals)) {
    stateSnap[k] = structuredCloneIfObject(sig.peek());
  }
  const propSnap: Record<string, unknown> = {};
  for (const [k, sig] of Object.entries(ctx.propSignals)) {
    propSnap[k] = structuredCloneIfObject(sig.peek());
  }
  return {
    componentName: ctx.componentName,
    locale: ctx.locale.peek(),
    state: stateSnap,
    props: propSnap,
  };
}

export function restoreFormaHmrSnapshot(
  ctx: {
    assertLive(): void;
    locale: Signal<string>;
    stateSignals: Record<string, FormaHmrSignalCell>;
    propSignals: Record<string, FormaHmrSignalCell>;
  },
  snapshot: FormaHmrSnapshot,
): void {
  ctx.assertLive();
  batch(() => {
    if (snapshot.locale) {
      ctx.locale.set(snapshot.locale);
    }
    if (snapshot.state) {
      for (const [k, v] of Object.entries(snapshot.state)) {
        ctx.stateSignals[k]?.set(structuredCloneIfObject(v));
      }
    }
    if (snapshot.props) {
      for (const [k, v] of Object.entries(snapshot.props)) {
        ctx.propSignals[k]?.set(structuredCloneIfObject(v));
      }
    }
  });
}

function rollbackFailedHotReload(
  ctx: FormaHmrContext,
  previousAst: FormaDocumentAst,
  previousState: Record<string, unknown>,
  previousProps: Record<string, unknown>,
  previousLets: Record<string, FormaHmrComputedCell>,
  previousOwnedLets: Set<FormaHmrComputedCell>,
  previousLocale: string,
  error: unknown,
): void {
  for (const [key, sig] of Object.entries(ctx.stateSignals)) {
    if (ctx.isDisposed()) throw error;
    if (!(key in previousState)) {
      const owned = ctx.ownedStateSignals.get(key);
      ctx.ownedStateSignals.delete(key);
      try {
        owned?.dispose();
      } catch {
        /* Keep the preparation failure while draining owned candidates. */
      }
      delete ctx.stateSignals[key];
    } else sig.set(previousState[key]);
  }
  for (const [key, sig] of Object.entries(ctx.propSignals)) {
    if (ctx.isDisposed()) throw error;
    if (!(key in previousProps)) {
      const owned = ctx.ownedPropSignals.get(key);
      ctx.ownedPropSignals.delete(key);
      try {
        owned?.dispose();
      } catch {
        /* Drain owned props without replacing the preparation failure. */
      }
      delete ctx.propSignals[key];
    } else sig.set(previousProps[key]);
  }
  if (ctx.isDisposed()) throw error;
  for (const comp of ctx.ownedComputedLets) {
    if (!previousOwnedLets.has(comp)) {
      try {
        comp.dispose();
      } catch {
        /* Preserve the preparation failure and drain siblings. */
      }
    }
  }
  if (ctx.isDisposed()) throw error;
  ctx.ownedComputedLets.clear();
  for (const comp of previousOwnedLets) ctx.ownedComputedLets.add(comp);
  for (const key of Object.keys(ctx.computedLets)) delete ctx.computedLets[key];
  Object.assign(ctx.computedLets, previousLets);
  if (ctx.isDisposed()) throw error;
  for (const key of Object.keys(ctx.initialStateValues)) delete ctx.initialStateValues[key];
  // Previous constraints already contain resolved defaults; do not rerun caller getters.
  ctx.applyDocumentAst(previousAst, true, false);
  if (ctx.isDisposed()) throw error;
  ctx.locale.set(previousLocale);
  if (ctx.isDisposed()) throw error;
  ctx.hmrRevision.update(r => r + 1);
}

export function performFormaHotReload(
  ctx: FormaHmrContext,
  nextUiSourceOrAst: string | FormaDocumentAst,
): void {
  ctx.assertLive();
  const nextAst = ctx.parseAst(nextUiSourceOrAst);
  ctx.assertLive();
  const previousAst: FormaDocumentAst = {
    componentName: ctx.componentName,
    tokens: { ...ctx.tokens } as FormaDocumentAst['tokens'],
    i18nDictionaries: { ...ctx.i18nDictionaries },
    styles: { ...ctx.styles },
    propConstraints: [...ctx.propConstraints],
    initialStateValues: { ...ctx.initialStateValues },
    letExpressions: [...ctx.letExpressions],
    shortcuts: [...ctx.shortcuts],
    rootAstNodes: [...ctx.rootAstNodes],
  };
  const previousLets = { ...ctx.computedLets };
  const previousOwnedLets = new Set(ctx.ownedComputedLets);
  const previousState = Object.fromEntries(
    Object.entries(ctx.stateSignals).map(([key, sig]) => [key, sig.peek()]),
  );
  const previousProps = Object.fromEntries(
    Object.entries(ctx.propSignals).map(([key, sig]) => [key, sig.peek()]),
  );
  const previousLocale = ctx.locale.peek();
  const prepared: PreparedFormaView[] = [];
  let committing = false;
  let previousLetsDetached = false;
  let cleanupFailed = false;
  let cleanupError: unknown;
  try {
    ctx.assertLive();
    batch(() => {
      try {
        ctx.applyDocumentAst(nextAst, true);
        ctx.assertLive();
        previousLetsDetached = true;
        ctx.rebuildComputedLets(false);
        ctx.assertLive();
        ctx.hmrRevision.update(r => r + 1);
        for (const prepare of [...ctx.prepareViews]) {
          if (ctx.prepareViews.has(prepare)) prepared.push(prepare());
        }
        committing = true;
        for (const view of prepared) view.commit();
      } catch (error) {
        for (const view of prepared.reverse()) view.abort();
        if (!committing && !ctx.isDisposed()) {
          rollbackFailedHotReload(
            ctx,
            previousAst,
            previousState,
            previousProps,
            previousLets,
            previousOwnedLets,
            previousLocale,
            error,
          );
          previousLetsDetached = false;
        }
        throw error;
      }
    });
  } finally {
    if ((committing || ctx.isDisposed()) && previousLetsDetached) {
      for (const comp of previousOwnedLets) {
        try {
          comp.dispose();
        } catch (error) {
          if (!cleanupFailed) cleanupError = error;
          cleanupFailed = true;
        }
      }
    }
  }
  if (cleanupFailed) throw cleanupError;
}

export interface FormaInstanceAssembly {
  componentName: string;
  documentState: FormaDocumentState;
  parseAst(nextUiSourceOrAst: string | FormaDocumentAst): FormaDocumentAst;
  applyDocumentAst(
    docAst: FormaDocumentAst,
    isHotReload?: boolean,
    readOptionProps?: boolean,
  ): void;
  rebuildComputedLets(retirePrevious?: boolean): void;
  computedLets: Record<string, FormaHmrComputedCell>;
  ownedComputedLets: Set<FormaHmrComputedCell>;
  locale: Signal<string>;
  activeI18n: ReadonlySignal<Record<string, string>>;
  isValid: ReadonlySignal<boolean>;
  hmrRevision: Signal<number>;
  actions: FormaStateMutator;
  ensureStateSignal(key: string, initialVal?: unknown): FormaHmrSignalCell;
  assertLive(): void;
  isDisposed(): boolean;
  markDisposed(): boolean;
  isUpdating(): boolean;
  setUpdating(next: boolean): void;
  views: {
    readonly prepareViews: Set<() => PreparedFormaView>;
    getElementByKey(key: string): HTMLElement | undefined;
    render(): DocumentFragment;
    mount(container: HTMLElement): { unmount(): void };
    disposeViews(): { failed: boolean; firstError: unknown };
  };
  eventListeners: Map<string, Set<(...args: unknown[]) => void>>;
  emitEvent(eventName: string, ...args: unknown[]): void;
}

export function assembleFormaRuntimeInstance(
  assembly: FormaInstanceAssembly,
): FormaRuntimeInstance {
  const {
    componentName,
    documentState,
    parseAst,
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
    isDisposed,
    markDisposed,
    isUpdating,
    setUpdating,
    views,
    eventListeners,
    emitEvent,
  } = assembly;

  const hmrContext: FormaHmrContext = {
    componentName,
    parseAst,
    assertLive,
    isDisposed,
    tokens: documentState.tokens,
    i18nDictionaries: documentState.i18nDictionaries,
    styles: documentState.styles,
    propConstraints: documentState.propConstraints,
    initialStateValues: documentState.initialStateValues,
    letExpressions: documentState.letExpressions,
    shortcuts: documentState.shortcuts,
    rootAstNodes: documentState.rootAstNodes,
    stateSignals: documentState.stateSignals,
    propSignals: documentState.propSignals,
    ownedStateSignals: documentState.ownedStateSignals,
    ownedPropSignals: documentState.ownedPropSignals,
    computedLets,
    ownedComputedLets,
    locale,
    hmrRevision,
    prepareViews: views.prepareViews,
    applyDocumentAst,
    rebuildComputedLets,
  };

  return {
    componentName,
    state: documentState.stateSignals,
    props: documentState.propSignals,
    computedLets: computedLets as FormaRuntimeInstance['computedLets'],
    locale,
    isValid,
    actions,
    setState(partial: Record<string, unknown>) {
      assertLive();
      batch(() => {
        for (const [k, v] of Object.entries(partial)) {
          if (FORBIDDEN_MEMBER_KEYS.has(k)) continue;
          ensureStateSignal(k, v).set(v);
        }
      });
    },
    onEvent(eventName: string, handler: (...args: unknown[]) => void) {
      assertLive();
      let set = eventListeners.get(eventName);
      if (!set) {
        set = new Set();
        eventListeners.set(eventName, set);
      }
      set.add(handler);
      return () => {
        set.delete(handler);
      };
    },
    emitEvent,
    getElementByKey(key: string) {
      return views.getElementByKey(key);
    },
    exportHmrSnapshot(): FormaHmrSnapshot {
      return exportFormaHmrSnapshot(hmrContext);
    },
    restoreHmrSnapshot(snapshot: FormaHmrSnapshot) {
      restoreFormaHmrSnapshot(hmrContext, snapshot);
    },
    hotReload(nextUiSourceOrAst: string | FormaDocumentAst) {
      assertLive();
      if (isUpdating()) throw new Error('Forma document update is already in progress');
      setUpdating(true);
      try {
        performFormaHotReload(hmrContext, nextUiSourceOrAst);
      } finally {
        setUpdating(false);
      }
    },
    render() {
      return views.render();
    },
    mount(container: HTMLElement) {
      return views.mount(container);
    },
    dispose() {
      if (!markDisposed()) return;
      let { failed, firstError } = views.disposeViews();
      const release = (resource: { dispose(): void }) => {
        try {
          resource.dispose();
        } catch (error) {
          if (!failed) firstError = error;
          failed = true;
        }
      };
      for (const sig of documentState.ownedStateSignals.values()) release(sig);
      documentState.ownedStateSignals.clear();
      for (const sig of documentState.ownedPropSignals.values()) release(sig);
      documentState.ownedPropSignals.clear();
      for (const comp of ownedComputedLets) release(comp);
      ownedComputedLets.clear();
      release(activeI18n);
      release(isValid);
      release(locale);
      release(hmrRevision);
      eventListeners.clear();
      if (failed) throw firstError;
    },
  };
}
