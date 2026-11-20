/**
 * Loading a parsed Forma document into the containers one instance keeps: tokens, translations,
 * styles, prop constraints, state and prop signals, `let` expressions, shortcuts and the tree.
 */
import type { FormaAstNode, FormaDocumentAst, FormaPropConstraint } from './forma-ast.ts';
import { FORBIDDEN_MEMBER_KEYS, isSafePatternRegex } from './forma-expr.ts';
import type { FormaCompileOptions, FormaProperty, FormaRuntimeInstance } from './forma-instance.ts';
import { structuredCloneIfObject } from './forma-reactive.ts';
import type { ReadonlySignal, Signal } from './reactivity.ts';

/** The containers of one instance. They are updated in place, so readers keep their references. */
export interface FormaDocumentState {
  tokens: Record<string, unknown>;
  i18nDictionaries: Record<string, Record<string, string>>;
  styles: Record<string, Record<string, string>>;
  propConstraints: Array<FormaPropConstraint & { compiledPattern?: RegExp | undefined }>;
  initialStateValues: Record<string, unknown>;
  stateSignals: FormaRuntimeInstance['state'];
  propSignals: FormaRuntimeInstance['props'];
  /** Signals the instance created itself, as opposed to ones bound from a view model. */
  ownedStateSignals: Map<string, FormaRuntimeInstance['state'][string]>;
  ownedPropSignals: Map<string, FormaRuntimeInstance['props'][string]>;
  letExpressions: Array<{ name: string; expr: string }>;
  shortcuts: Array<{ combo: string; action: string }>;
  rootAstNodes: FormaAstNode[];
}

export function createFormaDocumentState(): FormaDocumentState {
  return {
    tokens: {},
    i18nDictionaries: {},
    styles: {},
    propConstraints: [],
    initialStateValues: Object.create(null) as Record<string, unknown>,
    stateSignals: Object.create(null) as FormaDocumentState['stateSignals'],
    propSignals: Object.create(null) as FormaDocumentState['propSignals'],
    ownedStateSignals: new Map(),
    ownedPropSignals: new Map(),
    letExpressions: [],
    shortcuts: [],
    rootAstNodes: [],
  };
}

/**
 * @param isHotReload keeps the initial values of state fields that already exist
 * @param optionProps prop values passed by the host; they win over declared defaults
 * @param ownSignal creates a signal the instance disposes
 */
export function applyFormaDocument(
  {
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
  }: FormaDocumentState,
  docAst: FormaDocumentAst,
  {
    isHotReload,
    optionProps,
    ownSignal,
  }: {
    isHotReload: boolean;
    optionProps: Record<string, unknown> | undefined;
    ownSignal: <T>(value: T) => Signal<T>;
  },
): void {
  for (const k of Object.keys(tokens)) delete tokens[k];
  Object.assign(tokens, structuredCloneIfObject(docAst.tokens));

  for (const k of Object.keys(i18nDictionaries)) delete i18nDictionaries[k];
  Object.assign(i18nDictionaries, structuredCloneIfObject(docAst.i18nDictionaries));

  for (const k of Object.keys(styles)) delete styles[k];
  Object.assign(styles, structuredCloneIfObject(docAst.styles));

  propConstraints.length = 0;
  for (const c of docAst.propConstraints) {
    const defVal =
      optionProps && Object.prototype.hasOwnProperty.call(optionProps, c.name)
        ? optionProps[c.name]
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
}

/**
 * Binds the state a host passes in: a view model's properties and signals are used as they are,
 * so the host and the component share them; plain values in `options.state` become signals the
 * instance owns. Runs before the document is loaded, so declared defaults do not replace them.
 */
export function bindExternalFormaState(
  {
    stateSignals,
    initialStateValues,
  }: Pick<FormaDocumentState, 'stateSignals' | 'initialStateValues'>,
  options: FormaCompileOptions | undefined,
  ensureStateSignal: (key: string, initialVal?: unknown) => FormaRuntimeInstance['state'][string],
): void {
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
}

/** Whether every prop meets its `required`, `#[range]` and `#[pattern]` constraints. */
export function satisfiesFormaConstraints(
  propConstraints: FormaDocumentState['propConstraints'],
  propSignals: FormaDocumentState['propSignals'],
): boolean {
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
}

/**
 * Replaces the computed signals of the document's `let` declarations.
 *
 * @param compute creates the computed signal of one expression, owned by the instance
 * @param retirePrevious disposes the signals being replaced; a hot reload that may still roll
 *   back keeps them alive
 */
export function rebuildFormaComputedLets(
  {
    computedLets,
    ownedComputedLets,
    letExpressions,
  }: {
    computedLets: Record<string, ReadonlySignal<unknown>>;
    ownedComputedLets: Set<ReadonlySignal<unknown>>;
    letExpressions: FormaDocumentState['letExpressions'];
  },
  compute: (expression: string) => ReadonlySignal<unknown>,
  retirePrevious: boolean,
): void {
  if (retirePrevious)
    for (const oldComp of ownedComputedLets) {
      oldComp?.dispose?.();
    }
  ownedComputedLets.clear();
  for (const k of Object.keys(computedLets)) delete computedLets[k];
  for (const l of letExpressions) {
    const comp = compute(l.expr);
    ownedComputedLets.add(comp);
    computedLets[l.name] = comp;
  }
}
