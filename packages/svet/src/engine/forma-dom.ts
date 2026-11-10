/**
 * # Forma Built-in DOM Element Renderer (`forma-dom.ts`)
 *
 * Table-driven DOM tag resolution, reactive property binding, two-way `<->`
 * state binding, event handlers, and property-group `match` arms for Forma `.ui` elements.
 */

import { insertChild } from './dom.ts';
import { effect, onCleanup, type Signal } from './reactivity.ts';
import {
  compileFormaPatternMatcher,
  evalAstNode,
  evaluateFormaExpression,
  isStaticFormaExprNode,
  parseFormaExprAst,
  type ExprAst,
  type FormaEvalScope,
} from './forma-expr.ts';
import type { FormaAstNode } from './forma-ast.ts';

interface FormaTagDescriptor {
  readonly domTag: string;
  readonly baseClass: string;
  readonly isButton?: boolean;
  readonly display?: 'row' | 'column' | 'grid';
}

const BUTTON_TAGS = new Set([
  'Button',
  'PrimaryButton',
  'GhostButton',
  'SegmentButton',
  'IconButton',
  'QuietIconButton',
  'SecondaryButton',
  'LeadingIconButton',
  'PrimaryIconButton',
  'TabButton',
]);

const FORMA_TAG_DESCRIPTORS: ReadonlyMap<string, FormaTagDescriptor> = new Map<
  string,
  FormaTagDescriptor
>([
  ['Button', { domTag: 'button', baseClass: 'btn forma-btn', isButton: true }],
  ['PrimaryButton', { domTag: 'button', baseClass: 'btn btn-primary forma-btn', isButton: true }],
  ['GhostButton', { domTag: 'button', baseClass: 'btn forma-btn', isButton: true }],
  ['SegmentButton', { domTag: 'button', baseClass: 'forma-seg-btn', isButton: true }],
  ['IconButton', { domTag: 'button', baseClass: 'icon-btn forma-icon-btn', isButton: true }],
  ['QuietIconButton', { domTag: 'button', baseClass: 'icon-btn forma-icon-btn', isButton: true }],
  ['SecondaryButton', { domTag: 'button', baseClass: 'btn forma-btn', isButton: true }],
  ['LeadingIconButton', { domTag: 'button', baseClass: 'btn forma-btn', isButton: true }],
  ['PrimaryIconButton', { domTag: 'button', baseClass: 'btn forma-btn', isButton: true }],
  ['TabButton', { domTag: 'button', baseClass: 'btn forma-btn', isButton: true }],
  ['Slider', { domTag: 'input', baseClass: '' }],
  ['Text', { domTag: 'span', baseClass: '' }],
  ['Span', { domTag: 'span', baseClass: '' }],
  ['Badge', { domTag: 'span', baseClass: 'forma-badge' }],
  ['Tag', { domTag: 'span', baseClass: '' }],
  ['Chip', { domTag: 'span', baseClass: '' }],
  ['Spacer', { domTag: 'span', baseClass: 'toolbar-spacer' }],
  ['Strong', { domTag: 'strong', baseClass: '' }],
  ['Paragraph', { domTag: 'p', baseClass: '' }],
  ['Keycap', { domTag: 'kbd', baseClass: 'forma-kbd' }],
  ['StatusDot', { domTag: 'i', baseClass: 'status-dot' }],
  ['Nav', { domTag: 'nav', baseClass: 'forma-nav' }],
  ['Footer', { domTag: 'footer', baseClass: 'forma-footer' }],
  ['Section', { domTag: 'section', baseClass: 'forma-section' }],
  ['ProgressBar', { domTag: 'progress', baseClass: '' }],
  ['Checkbox', { domTag: 'label', baseClass: 'auto-check forma-checkbox' }],
  ['Divider', { domTag: 'hr', baseClass: '' }],
  ['Separator', { domTag: 'hr', baseClass: '' }],
  ['Row', { domTag: 'div', baseClass: 'forma-row', display: 'row' }],
  ['Column', { domTag: 'div', baseClass: 'forma-col', display: 'column' }],
  ['Frame', { domTag: 'div', baseClass: '', display: 'column' }],
  ['Grid', { domTag: 'div', baseClass: '', display: 'grid' }],
  ['Stack', { domTag: 'div', baseClass: '', display: 'grid' }],
]);

export interface FormaDomElementContext {
  readonly tag: string;
  readonly ast: FormaAstNode;
  readonly mergedProps: Record<string, string>;
  readonly localScope: FormaEvalScope;
  readonly locals?: Record<string, unknown> | undefined;
  registerKey(key: string, element: HTMLElement): void;
  ensureStateSignal(key: string, initialVal?: unknown): Signal<unknown>;
  onBinding?: ((path: string, value: unknown) => void) | undefined;
  executeAction(actionStr: string, locals?: Record<string, unknown>, domEvent?: Event): void;
  renderChild(childAst: FormaAstNode, locals?: Record<string, unknown>): Node;
}

export function renderFormaBuiltinElement(ctx: FormaDomElementContext): HTMLElement {
  const { tag, ast, mergedProps, localScope, locals } = ctx;
  const descriptor = FORMA_TAG_DESCRIPTORS.get(tag);
  const isButtonTag = descriptor?.isButton ?? BUTTON_TAGS.has(tag);

  let domTag = descriptor?.domTag ?? 'div';
  if (tag === 'TextInput' || tag === 'TextField') {
    const isMultilineInput = Boolean(
      mergedProps.multiline && evaluateFormaExpression(mergedProps.multiline, localScope),
    );
    domTag = isMultilineInput ? 'textarea' : 'input';
  }

  const el =
    tag === 'SvgIcon'
      ? (document.createElementNS('http://www.w3.org/2000/svg', 'svg') as unknown as HTMLElement)
      : document.createElement(domTag);
  el.setAttribute('data-forma-type', tag);

  const baseClasses = descriptor?.baseClass ?? '';
  let userClassStr = '';
  let isActiveClass = false;
  const syncClassAttribute = () => {
    const parts: string[] = [];
    if (baseClasses) parts.push(baseClasses);
    if (userClassStr) parts.push(userClassStr);
    if (isActiveClass) parts.push('active');
    if (parts.length > 0) {
      el.setAttribute('class', parts.join(' '));
    } else {
      el.removeAttribute('class');
    }
  };
  if (baseClasses) {
    syncClassAttribute();
  }
  if (isButtonTag) {
    el.setAttribute('type', 'button');
  } else if (tag === 'Slider') {
    el.setAttribute('type', 'range');
    if (!mergedProps.min) el.setAttribute('min', '0');
    if (!mergedProps.max) el.setAttribute('max', '100');
  }

  const bindExpr = (rawExpr: string, applyVal: (val: unknown) => void) => {
    const exprAst = parseFormaExprAst(rawExpr);
    if (isStaticFormaExprNode(exprAst, locals)) {
      applyVal(evalAstNode(exprAst, localScope));
    } else {
      effect(() => {
        applyVal(evalAstNode(exprAst, localScope));
      });
    }
  };

  if (tag === 'SvgIcon') {
    bindSvgIconElement(el, mergedProps, localScope, locals);
  }

  let checkboxInput: HTMLInputElement | null = null;
  let checkboxLabelNode: Text | null = null;
  if (tag === 'Checkbox') {
    checkboxInput = document.createElement('input');
    checkboxInput.type = 'checkbox';
    el.appendChild(checkboxInput);
    checkboxLabelNode = document.createTextNode('');
    el.appendChild(checkboxLabelNode);
  }

  // Apply `transition { duration: 200ms; easing: 'spring'; }` if present
  if (ast.transition) {
    const durVal = ast.transition.duration
      ? evaluateFormaExpression(ast.transition.duration, localScope)
      : 200;
    const durStr = typeof durVal === 'number' ? `${durVal}ms` : String(durVal);
    const rawEasing = ast.transition.easing
      ? String(evaluateFormaExpression(ast.transition.easing, localScope))
      : 'ease-out';
    const easingCss = rawEasing === 'spring' ? 'cubic-bezier(0.34, 1.56, 0.64, 1)' : rawEasing;
    el.style.transition = `all ${durStr} ${easingCss}`;
  }

  // Base layout styles for Forma containers (`Row`, `Column`, `Grid`, `Stack`, `Frame`)
  if (descriptor?.display === 'row') {
    el.style.display = 'flex';
    el.style.flexDirection = 'row';
    el.style.alignItems = 'center';
  } else if (descriptor?.display === 'column') {
    el.style.display = 'flex';
    el.style.flexDirection = 'column';
  } else if (descriptor?.display === 'grid') {
    el.style.display = 'grid';
  }

  // Register `key` for `getElementByKey`
  if (mergedProps.key) {
    const keyVal = String(evaluateFormaExpression(mergedProps.key, localScope));
    el.setAttribute('data-forma-key', keyVal);
    ctx.registerKey(keyVal, el);
  }

  const hasChildren = ast.children.length > 0;
  let leadingTextNode: Text | null = null;

  // Apply reactive properties
  for (const [propName, rawExpr] of Object.entries(mergedProps)) {
    if (propName === 'key') continue;
    if (
      tag === 'SvgIcon' &&
      (propName === 'path' ||
        propName === 'path2' ||
        propName === 'size' ||
        propName === 'strokeWidth' ||
        propName === 'fill')
    ) {
      continue;
    }

    if (propName === 'text' || (propName === 'label' && tag !== 'Checkbox')) {
      if (hasChildren) {
        if (!leadingTextNode) {
          leadingTextNode = document.createTextNode('');
          el.appendChild(leadingTextNode);
        }
        const tNode = leadingTextNode;
        bindExpr(rawExpr, val => {
          tNode.nodeValue = val == null ? '' : String(val);
        });
      } else {
        bindExpr(rawExpr, val => {
          el.textContent = val == null ? '' : String(val);
        });
      }
    } else if (propName === 'label' && tag === 'Checkbox' && checkboxLabelNode) {
      const lNode = checkboxLabelNode;
      bindExpr(rawExpr, val => {
        lNode.nodeValue = val ? ` ${String(val)}` : '';
      });
    } else if (propName === 'checked' && tag === 'Checkbox' && checkboxInput) {
      const inp = checkboxInput;
      bindExpr(rawExpr, val => {
        inp.checked = Boolean(val);
      });
    } else if (propName === 'value' && (domTag === 'input' || domTag === 'textarea')) {
      bindExpr(rawExpr, val => {
        const strVal =
          tag === 'Slider' && typeof val === 'string' && val.endsWith('%')
            ? val.slice(0, -1)
            : val == null
              ? ''
              : String(val);
        (el as HTMLInputElement | HTMLTextAreaElement).value = strVal;
      });
    } else if (
      (propName === 'min' || propName === 'max' || propName === 'step') &&
      tag === 'Slider'
    ) {
      bindExpr(rawExpr, v => {
        if (v !== undefined && v !== null) {
          el.setAttribute(propName, String(v));
        }
      });
    } else if (propName === 'readOnly' && (domTag === 'input' || domTag === 'textarea')) {
      bindExpr(rawExpr, v => {
        const isRo = Boolean(v);
        (el as HTMLInputElement | HTMLTextAreaElement).readOnly = isRo;
        if (isRo) el.setAttribute('readonly', '');
        else el.removeAttribute('readonly');
      });
    } else if (
      propName === 'wrap' &&
      (tag === 'Text' || tag === 'Span' || tag === 'Paragraph' || domTag === 'textarea')
    ) {
      bindExpr(rawExpr, v => {
        const shouldWrap = Boolean(v);
        if (domTag === 'textarea') {
          el.setAttribute('wrap', shouldWrap ? 'soft' : 'off');
        }
        el.style.whiteSpace = shouldWrap ? 'pre-wrap' : 'nowrap';
        el.style.overflowWrap = shouldWrap ? 'break-word' : 'normal';
      });
    } else if (propName === 'fontSize' || propName === 'font.size') {
      bindExpr(rawExpr, v => {
        el.style.fontSize = typeof v === 'number' ? `${v}px` : String(v ?? '');
      });
    } else if ((propName === 'value' || propName === 'max') && domTag === 'progress') {
      bindExpr(rawExpr, v => {
        const val = Number(v ?? (propName === 'max' ? 100 : 0));
        el.setAttribute(propName, String(val));
        (el as HTMLProgressElement)[propName] = val;
      });
    } else if (propName === 'disabled') {
      bindExpr(rawExpr, v => {
        const isDis = Boolean(v);
        (el as HTMLButtonElement).disabled = isDis;
        if (isDis) el.setAttribute('disabled', '');
        else el.removeAttribute('disabled');
      });
    } else if (propName === 'class' || propName === 'className') {
      bindExpr(rawExpr, v => {
        userClassStr = v == null ? '' : String(v).trim();
        syncClassAttribute();
      });
    } else if (propName === 'active') {
      bindExpr(rawExpr, v => {
        isActiveClass = Boolean(v);
        syncClassAttribute();
      });
    } else if (
      propName === 'id' ||
      propName === 'role' ||
      propName === 'title' ||
      propName === 'tooltip'
    ) {
      const attrName = propName === 'tooltip' ? 'title' : propName;
      bindExpr(rawExpr, v => {
        if (v !== undefined && v !== null && v !== '') {
          el.setAttribute(attrName, String(v));
        } else {
          el.removeAttribute(attrName);
        }
      });
    } else if (propName === 'tabindex') {
      bindExpr(rawExpr, v => {
        if (v !== undefined && v !== null) {
          el.setAttribute('tabindex', String(Number(v)));
        } else {
          el.removeAttribute('tabindex');
        }
      });
    } else if (propName === 'testId') {
      bindExpr(rawExpr, v => {
        if (v !== undefined && v !== null && v !== '') {
          el.setAttribute('data-testid', String(v));
        } else {
          el.removeAttribute('data-testid');
        }
      });
    } else if (propName.startsWith('aria') && propName.length > 4) {
      const ariaAttr = `aria-${propName.slice(4).toLowerCase()}`;
      bindExpr(rawExpr, v => {
        if (v !== undefined && v !== null && v !== '') {
          el.setAttribute(ariaAttr, String(v));
        } else {
          el.removeAttribute(ariaAttr);
        }
      });
    } else if (propName === 'placeholder' && (domTag === 'input' || domTag === 'textarea')) {
      bindExpr(rawExpr, v => {
        (el as HTMLInputElement | HTMLTextAreaElement).placeholder = String(v ?? '');
      });
    } else if (propName === 'gap') {
      bindExpr(rawExpr, v => {
        el.style.gap = typeof v === 'number' ? `${v}px` : String(v);
      });
    } else if (propName === 'padding') {
      bindExpr(rawExpr, v => {
        el.style.padding = typeof v === 'number' ? `${v}px` : String(v);
      });
    } else if (propName === 'background') {
      bindExpr(rawExpr, v => {
        el.style.background = String(v ?? '');
      });
    } else if (propName === 'color') {
      bindExpr(rawExpr, v => {
        el.style.color = String(v ?? '');
      });
    } else if (propName === 'cornerRadius') {
      bindExpr(rawExpr, v => {
        el.style.borderRadius = typeof v === 'number' ? `${v}px` : String(v);
      });
    } else if (propName === 'width') {
      bindExpr(rawExpr, v => {
        el.style.width = typeof v === 'number' ? `${v}px` : String(v);
      });
    } else if (propName === 'height') {
      bindExpr(rawExpr, v => {
        el.style.height = typeof v === 'number' ? `${v}px` : String(v);
      });
    } else {
      bindExpr(rawExpr, v => {
        if (v !== undefined && v !== null) {
          el.setAttribute(`data-${propName.toLowerCase()}`, String(v));
        }
      });
    }
  }

  // Two-way bindings: `value <-> state.name` and `checked <-> state.autoRender`
  bindTwoWayHandlers(ctx, el, domTag, checkboxInput);

  // Event handlers: `clicked -> ...`, `keydown -> ...`, `changed -> ...`
  bindEventHandlers(ctx, el, checkboxInput);

  // Render children (and evaluate any property-group `match` blocks applied to this element!)
  for (const childAst of ast.children) {
    if (
      childAst.kind === 'match' &&
      childAst.matchArms &&
      childAst.matchArms.every(a => a.childNodes.length === 0) &&
      childAst.matchExpr !== undefined
    ) {
      const propMatchAst = parseFormaExprAst(childAst.matchExpr);
      const compiledPropArms = childAst.matchArms.map(a => ({
        matches: compileFormaPatternMatcher(a.pattern),
        assignments: Object.entries(a.propertyAssignments).map(
          ([pKey, pExpr]) => [pKey, parseFormaExprAst(pExpr)] as [string, ExprAst],
        ),
      }));
      effect(() => {
        const val = evalAstNode(propMatchAst, localScope);
        for (const arm of compiledPropArms) {
          if (arm.matches(val)) {
            for (const [pKey, pAst] of arm.assignments) {
              const resolved = evalAstNode(pAst, localScope);
              if (pKey === 'background') el.style.background = String(resolved ?? '');
              else if (pKey === 'color') el.style.color = String(resolved ?? '');
              else if (pKey === 'borderColor') el.style.borderColor = String(resolved ?? '');
              else if (pKey === 'text') el.textContent = String(resolved ?? '');
              else el.setAttribute(`data-${pKey.toLowerCase()}`, String(resolved ?? ''));
            }
            break;
          }
        }
      });
    } else {
      insertChild(el, ctx.renderChild(childAst, locals));
    }
  }

  return el;
}

function bindSvgIconElement(
  el: HTMLElement,
  mergedProps: Record<string, string>,
  localScope: FormaEvalScope,
  locals?: Record<string, unknown>,
): void {
  el.setAttribute('viewBox', '0 0 24 24');
  el.setAttribute('stroke-linecap', 'round');
  el.setAttribute('stroke-linejoin', 'round');
  el.setAttribute('aria-hidden', 'true');
  const path1 = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  const path2 = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  const sizeAst = mergedProps.size ? parseFormaExprAst(mergedProps.size) : null;
  const strokeWidthAst = mergedProps.strokeWidth
    ? parseFormaExprAst(mergedProps.strokeWidth)
    : null;
  const fillAst = mergedProps.fill ? parseFormaExprAst(mergedProps.fill) : null;
  const path1Ast = mergedProps.path ? parseFormaExprAst(mergedProps.path) : null;
  const path2Ast = mergedProps.path2 ? parseFormaExprAst(mergedProps.path2) : null;
  const applySvg = () => {
    const size = Number(sizeAst ? evalAstNode(sizeAst, localScope) : 15) || 15;
    const strokeWidth = Number(strokeWidthAst ? evalAstNode(strokeWidthAst, localScope) : 2) || 2;
    const fill = String(fillAst ? (evalAstNode(fillAst, localScope) ?? 'none') : 'none');
    el.setAttribute('width', String(size));
    el.setAttribute('height', String(size));
    el.setAttribute('fill', fill);
    el.setAttribute('stroke', fill === 'currentColor' ? 'none' : 'currentColor');
    el.setAttribute('stroke-width', String(strokeWidth));
    const d1 = path1Ast ? evalAstNode(path1Ast, localScope) : null;
    if (d1) {
      path1.setAttribute('d', String(d1));
      if (!path1.parentNode) el.appendChild(path1);
    } else if (path1.parentNode) {
      path1.parentNode.removeChild(path1);
    }
    const d2 = path2Ast ? evalAstNode(path2Ast, localScope) : null;
    if (d2) {
      path2.setAttribute('d', String(d2));
      if (!path2.parentNode) el.appendChild(path2);
    } else if (path2.parentNode) {
      path2.parentNode.removeChild(path2);
    }
  };
  const allSvgStatic = [sizeAst, strokeWidthAst, fillAst, path1Ast, path2Ast].every(
    astNode => !astNode || isStaticFormaExprNode(astNode, locals),
  );
  if (allSvgStatic) applySvg();
  else effect(applySvg);
}

function bindTwoWayHandlers(
  ctx: FormaDomElementContext,
  el: HTMLElement,
  domTag: string,
  checkboxInput: HTMLInputElement | null,
): void {
  const { tag, ast, locals } = ctx;
  for (const [propName, targetPath] of Object.entries(ast.twoWayBindings)) {
    const cleanName = targetPath.replace(/^state\./, '').trim();
    const targetSig = ctx.ensureStateSignal(cleanName, propName === 'checked' ? false : '');
    if (propName === 'value' && (domTag === 'input' || domTag === 'textarea')) {
      const inputEl = el as HTMLInputElement | HTMLTextAreaElement;
      effect(() => {
        const raw = targetSig();
        const nextStr =
          tag === 'Slider' && typeof raw === 'string' && raw.endsWith('%')
            ? raw.slice(0, -1)
            : String(raw ?? '');
        if (inputEl.value !== nextStr) {
          inputEl.value = nextStr;
        }
      });
      const onInput = (ev: Event) => {
        const nextVal =
          tag === 'Slider' && typeof targetSig.peek() === 'number'
            ? Number(inputEl.value)
            : inputEl.value;
        targetSig.set(nextVal);
        ctx.onBinding?.(cleanName, nextVal);
        if (ast.eventHandlers.changed) {
          ctx.executeAction(ast.eventHandlers.changed, locals, ev);
        }
      };
      inputEl.addEventListener('input', onInput);
      onCleanup(() => inputEl.removeEventListener('input', onInput));
    } else if (propName === 'checked' && checkboxInput) {
      const chkEl = checkboxInput;
      effect(() => {
        const nextBool = Boolean(targetSig());
        if (chkEl.checked !== nextBool) {
          chkEl.checked = nextBool;
        }
      });
      const onChange = (ev: Event) => {
        const nextChecked = chkEl.checked;
        targetSig.set(nextChecked);
        ctx.onBinding?.(cleanName, nextChecked);
        if (ast.eventHandlers.changed) {
          ctx.executeAction(ast.eventHandlers.changed, locals, ev);
        }
      };
      chkEl.addEventListener('change', onChange);
      onCleanup(() => chkEl.removeEventListener('change', onChange));
    }
  }
}

function bindEventHandlers(
  ctx: FormaDomElementContext,
  el: HTMLElement,
  checkboxInput: HTMLInputElement | null,
): void {
  const { ast, locals } = ctx;
  for (const [evtName, actionExpr] of Object.entries(ast.eventHandlers)) {
    if (evtName === 'clicked') {
      const onClick = (ev: MouseEvent) => {
        if (!(el as HTMLButtonElement).disabled) {
          ctx.executeAction(actionExpr, locals, ev);
        }
      };
      el.addEventListener('click', onClick);
      onCleanup(() => el.removeEventListener('click', onClick));
    } else if (evtName === 'keydown') {
      const onKeydown = (ev: KeyboardEvent) => {
        if (!(el as HTMLButtonElement).disabled) {
          ctx.executeAction(actionExpr, locals, ev);
        }
      };
      el.addEventListener('keydown', onKeydown);
      onCleanup(() => el.removeEventListener('keydown', onKeydown));
    } else if (
      evtName === 'changed' &&
      !('value' in ast.twoWayBindings) &&
      !('checked' in ast.twoWayBindings)
    ) {
      const targetEl = checkboxInput ?? el;
      const onChange = (ev: Event) => {
        ctx.executeAction(actionExpr, locals, ev);
      };
      targetEl.addEventListener('change', onChange);
      onCleanup(() => targetEl.removeEventListener('change', onChange));
    }
  }
}
