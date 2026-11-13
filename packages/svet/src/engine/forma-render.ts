/**
 * Rendering the parsed tree of a Forma component into DOM: slots, control flow, nested components
 * and the built-in elements, each bound to the signals of one instance.
 */

import { For, insertChild, Show } from './dom.ts';
import type { FormaCompileOptions } from './forma.ts';
import type { FormaAstNode, FormaDocumentAst } from './forma-ast.ts';
import { renderFormaBuiltinElement } from './forma-dom.ts';
import {
  compileFormaPatternMatcher,
  evalAstNode,
  evaluateFormaExpression,
  type ExprAst,
  type FormaEvalScope,
  parseFormaExprAst,
} from './forma-expr.ts';
import { constructFormaChild, type FormaRuntimeInstance } from './forma-instance.ts';
import { FORMA_3D_TAGS, formaComponentRegistry, formaElementRenderers } from './forma-registry.ts';
import {
  createRoot,
  effect,
  getCurrentOwner,
  isOwnerScopeDisposed,
  onCleanup,
  type Signal,
  untrack,
} from './reactivity.ts';

/** What one instance lends its renderer. */
export interface FormaRenderContext {
  /** Throws once the instance is disposed. */
  assertLive(): void;
  isDisposed(): boolean;
  /** Instantiates a component embedded by name. */
  instantiateChild(
    uiSourceOrAst: string | FormaDocumentAst,
    options?: FormaCompileOptions,
  ): FormaRuntimeInstance;
  baseScopeWithLocals(locals?: FormaEvalScope['locals']): FormaEvalScope;
  ensureStateSignal(key: string, initialVal?: unknown): FormaRuntimeInstance['state'][string];
  executeFormaAction(actionStr: string, locals?: FormaEvalScope['locals'], domEvent?: Event): void;
  /** Records the element rendered for a `key`, for `getElementByKey`. */
  registerKey(key: string, element: HTMLElement): void;
  locale: Signal<string>;
  options: FormaCompileOptions | undefined;
  propSignals: FormaRuntimeInstance['props'];
  styles: Record<string, Record<string, string>>;
}

export function createFormaRenderer({
  assertLive,
  isDisposed,
  instantiateChild,
  baseScopeWithLocals,
  ensureStateSignal,
  executeFormaAction,
  registerKey,
  locale,
  options,
  propSignals,
  styles,
}: FormaRenderContext) {
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
        registerKey,
      });
    }
    if (FORMA_3D_TAGS.has(tag)) {
      throw new Error(
        `Forma <${tag}> needs the 3D module: add \`import 'svet-engine/forma-3d'\` (the default \`svet-engine\` entry does it for you)`,
      );
    }

    // Check if `tag` references a registered or passed child Forma `.ui` component!
    const customCompDef = options?.components?.[tag] ?? formaComponentRegistry.get(tag);
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
          : instantiateChild(customCompDef, {
              props: initialChildProps,
              locale: locale.peek(),
              components: options?.components,
            }),
      );

      // A callback may retire this view before returning its newly created child.
      // A closed owner cannot register cleanup, so release that child immediately.
      if (isDisposed() || isOwnerScopeDisposed(childOwner)) {
        try {
          childInst.dispose();
        } finally {
          assertLive();
        }
        throw new Error('Forma view owner is isDisposed()');
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
        registerKey(keyVal, wrapperEl);
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
      registerKey,
      ensureStateSignal,
      onBinding: options?.onBinding,
      executeAction: executeFormaAction,
      renderChild: renderAstNode,
    });
  };

  return renderAstNode;
}
