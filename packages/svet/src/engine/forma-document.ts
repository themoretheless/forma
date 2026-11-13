/**
 * Loading a parsed Forma document into the containers one instance keeps: tokens, translations,
 * styles, prop constraints, state and prop signals, `let` expressions, shortcuts and the tree.
 */
import type { FormaAstNode, FormaDocumentAst, FormaPropConstraint } from './forma-ast.ts';
import { isSafePatternRegex } from './forma-expr.ts';
import type { FormaRuntimeInstance } from './forma-instance.ts';
import { structuredCloneIfObject } from './forma-reactive.ts';
import type { Signal } from './reactivity.ts';

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
