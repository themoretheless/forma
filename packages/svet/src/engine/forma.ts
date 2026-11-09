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
// 2. Forma Standard Library (`formaStdlib`) & Pipe Expression Evaluator
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
  isStaticFormaExprNode,
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
};

// ============================================================================
// 3. Forma `.ui` AST & Runtime Compiler (`compileFormaComponent`, `mountForma`)
// ============================================================================

export interface FormaPropConstraint {
  name: string;
  type: string;
  required: boolean;
  defaultValue: any;
  range?: [number, number];
  patternSource?: string;
}

export interface FormaDocumentAst {
  componentName: string;
  tokens: Record<string, any>;
  i18nDictionaries: Record<string, Record<string, string>>;
  styles: Record<string, Record<string, string>>;
  propConstraints: FormaPropConstraint[];
  initialStateValues: Record<string, any>;
  letExpressions: Array<{ name: string; expr: string }>;
  shortcuts: Array<{ combo: string; action: string }>;
  rootAstNodes: FormaAstNode[];
}

export interface FormaStateMutator {
  toggle(field: string): void;
  set(field: string, value: any): void;
  increment(field: string, delta?: number): void;
  decrement(field: string, delta?: number): void;
  push(field: string, item: any): void;
  remove(field: string, index: number): void;
  reset(field?: string): void;
}

export interface FormaDiagnostic {
  severity: 'error' | 'warning' | 'info';
  message: string;
  line?: number;
}

export interface FormaValidationReport {
  valid: boolean;
  componentName: string;
  diagnostics: FormaDiagnostic[];
  propCount: number;
  stateCount: number;
  letCount: number;
  has3DViewport: boolean;
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

interface FormaAstNode {
  kind: 'element' | 'if' | 'for' | 'match' | 'slot' | 'contentPresenter';
  tag?: string;
  slotName?: string;
  properties: Record<string, string>;
  twoWayBindings: Record<string, string>;
  eventHandlers: Record<string, string>;
  appliedStyles: string[];
  forwardedProps: string[];
  transition?: Record<string, string>;
  children: FormaAstNode[];
  // For `if`
  conditionExpr?: string;
  elseChildren?: FormaAstNode[];
  // For `for`
  itemVar?: string;
  listExpr?: string;
  keyExpr?: string;
  emptyChildren?: FormaAstNode[];
  // For `match`
  matchExpr?: string;
  matchArms?: Array<{
    pattern: string;
    propertyAssignments: Record<string, string>;
    childNodes: FormaAstNode[];
  }>;
}

function setNestedObjectKey(obj: Record<string, any>, dottedKey: string, value: any): void {
  const parts = dottedKey.split('.');
  for (const p of parts) {
    if (FORBIDDEN_MEMBER_KEYS.has(p)) {
      return;
    }
  }
  let curr = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const p = parts[i];
    if (!Object.prototype.hasOwnProperty.call(curr, p) || !curr[p] || typeof curr[p] !== 'object') {
      curr[p] = Object.create(null);
    }
    curr = curr[p];
  }
  curr[parts[parts.length - 1]] = value;
}

function extractBraceBlock(source: string, openBraceIdx: number): { body: string; endIdx: number } {
  let depth = 0;
  let i = openBraceIdx;
  const n = source.length;
  while (i < n) {
    const ch = source[i];
    if (ch === "'" || ch === '"') {
      const q = ch;
      i++;
      while (i < n && source[i] !== q) {
        if (source[i] === '\\') i++;
        i++;
      }
      i++;
      continue;
    }
    if (ch === '{') depth++;
    else if (ch === '}') {
      depth--;
      if (depth === 0) {
        return { body: source.slice(openBraceIdx + 1, i), endIdx: i + 1 };
      }
    }
    i++;
  }
  return { body: source.slice(openBraceIdx + 1), endIdx: n };
}

function parseFormaUiBody(bodyStr: string): FormaAstNode[] {
  const nodes: FormaAstNode[] = [];
  let i = 0;
  const src = bodyStr.trim();
  const n = src.length;

  while (i < n) {
    while (i < n && /\s/.test(src[i])) i++;
    if (i >= n) break;

    // Skip comments
    if (src.startsWith('//', i)) {
      const nl = src.indexOf('\n', i + 2);
      i = nl === -1 ? n : nl + 1;
      continue;
    }

    if (src.startsWith('#[', i))
      throw new Error('Forma contract annotation cannot follow visual declarations');

    // 1. `if <cond> { ... } else { ... }`
    if (src.startsWith('if ', i) || src.startsWith('if(', i)) {
      const braceIdx = src.indexOf('{', i + 2);
      if (braceIdx === -1) break;
      const condExpr = src.slice(i + 2, braceIdx).trim();
      const thenBlock = extractBraceBlock(src, braceIdx);
      i = thenBlock.endIdx;
      while (i < n && /\s/.test(src[i])) i++;

      let elseChildren: FormaAstNode[] = [];
      if (src.startsWith('else', i)) {
        const elseBrace = src.indexOf('{', i + 4);
        if (elseBrace !== -1) {
          const elseBlock = extractBraceBlock(src, elseBrace);
          elseChildren = parseFormaUiBody(elseBlock.body);
          i = elseBlock.endIdx;
        }
      }
      nodes.push({
        kind: 'if',
        conditionExpr: condExpr,
        properties: {},
        twoWayBindings: {},
        eventHandlers: {},
        appliedStyles: [],
        forwardedProps: [],
        children: parseFormaUiBody(thenBlock.body),
        elseChildren,
      });
      continue;
    }

    // 1b. `match <expr> { pattern => ... }`
    if (src.startsWith('match ', i)) {
      const braceIdx = src.indexOf('{', i + 6);
      if (braceIdx === -1) break;
      const matchExpr = src.slice(i + 6, braceIdx).trim();
      const matchBlock = extractBraceBlock(src, braceIdx);
      i = matchBlock.endIdx;
      nodes.push({
        kind: 'match',
        matchExpr,
        matchArms: parseMatchArms(matchBlock.body),
        properties: {},
        twoWayBindings: {},
        eventHandlers: {},
        appliedStyles: [],
        forwardedProps: [],
        children: [],
      });
      continue;
    }

    // 1c. `slot <name> { ... }`
    const slotDeclMatch = src.slice(i).match(/^slot\s+([a-zA-Z0-9_]+)\s*\{/);
    if (slotDeclMatch) {
      const slotName = slotDeclMatch[1];
      const braceIdx = src.indexOf('{', i);
      const slotBlock = extractBraceBlock(src, braceIdx);
      i = slotBlock.endIdx;
      nodes.push({
        kind: 'slot',
        slotName,
        properties: {},
        twoWayBindings: {},
        eventHandlers: {},
        appliedStyles: [],
        forwardedProps: [],
        children: parseFormaUiBody(slotBlock.body),
      });
      continue;
    }

    // 1d. `ContentPresenter { ... }`
    if (/^ContentPresenter\s*\{/.test(src.slice(i))) {
      const braceIdx = src.indexOf('{', i);
      const cpBlock = extractBraceBlock(src, braceIdx);
      i = cpBlock.endIdx;
      nodes.push({
        kind: 'contentPresenter',
        properties: {},
        twoWayBindings: {},
        eventHandlers: {},
        appliedStyles: [],
        forwardedProps: [],
        children: parseFormaUiBody(cpBlock.body),
      });
      continue;
    }

    // 2. `for <item> in <list> key <keyExpr> { ... } empty { ... }`
    if (src.startsWith('for ', i)) {
      const braceIdx = src.indexOf('{', i + 4);
      if (braceIdx === -1) break;
      const header = src.slice(i + 4, braceIdx).trim();
      const forMatch = header.match(/^([a-zA-Z_][a-zA-Z0-9_]*)\s+in\s+(.+?)(?:\s+key\s+(.+))?$/);
      const itemBlock = extractBraceBlock(src, braceIdx);
      i = itemBlock.endIdx;
      while (i < n && /\s/.test(src[i])) i++;

      let emptyChildren: FormaAstNode[] = [];
      if (src.startsWith('empty', i)) {
        const emptyBrace = src.indexOf('{', i + 5);
        if (emptyBrace !== -1) {
          const emptyBlock = extractBraceBlock(src, emptyBrace);
          emptyChildren = parseFormaUiBody(emptyBlock.body);
          i = emptyBlock.endIdx;
        }
      }

      if (forMatch) {
        nodes.push({
          kind: 'for',
          itemVar: forMatch[1].trim(),
          listExpr: forMatch[2].trim(),
          keyExpr: forMatch[3]?.trim(),
          properties: {},
          twoWayBindings: {},
          eventHandlers: {},
          appliedStyles: [],
          forwardedProps: [],
          children: parseFormaUiBody(itemBlock.body),
          emptyChildren,
        });
      }
      continue;
    }

    // 3. UI Element Block: `TagName { ... }`
    const tagMatch = src.slice(i).match(/^([A-Z][a-zA-Z0-9_]*)\s*\{/);
    if (tagMatch) {
      const tag = tagMatch[1];
      const braceIdx = src.indexOf('{', i);
      const block = extractBraceBlock(src, braceIdx);
      i = block.endIdx;
      nodes.push(parseElementInterior(tag, block.body));
      continue;
    }

    // Skip unrecognized line/statement
    const semi = src.indexOf(';', i);
    i = semi === -1 ? n : semi + 1;
  }

  return nodes;
}

function parseMatchArms(armsSource: string): Array<{
  pattern: string;
  propertyAssignments: Record<string, string>;
  childNodes: FormaAstNode[];
}> {
  const arms: Array<{
    pattern: string;
    propertyAssignments: Record<string, string>;
    childNodes: FormaAstNode[];
  }> = [];

  let i = 0;
  const src = armsSource.trim();
  const n = src.length;

  while (i < n) {
    while (i < n && /\s/.test(src[i])) i++;
    if (i >= n) break;

    const arrowIdx = src.indexOf('=>', i);
    if (arrowIdx === -1) break;
    const pattern = src.slice(i, arrowIdx).trim();
    i = arrowIdx + 2;
    while (i < n && /\s/.test(src[i])) i++;

    if (src[i] === '{') {
      const blk = extractBraceBlock(src, i);
      i = blk.endIdx;
      const bodyTrimmed = blk.body.trim();
      // Check if block contains UI elements (`TagName {`) or property assignments (`prop: val;`)
      if (/^[A-Z][a-zA-Z0-9_]*\s*\{/.test(bodyTrimmed)) {
        arms.push({
          pattern,
          propertyAssignments: {},
          childNodes: parseFormaUiBody(bodyTrimmed),
        });
      } else {
        const propAssigns: Record<string, string> = {};
        for (const rawLine of bodyTrimmed.split(';')) {
          const line = rawLine.trim();
          if (!line) continue;
          const cIdx = line.indexOf(':');
          if (cIdx !== -1) {
            propAssigns[line.slice(0, cIdx).trim()] = line.slice(cIdx + 1).trim();
          }
        }
        arms.push({
          pattern,
          propertyAssignments: propAssigns,
          childNodes: [],
        });
      }
    } else {
      // Single inline element: `Tone.Danger => Spinner {}`
      const tagMatch = src.slice(i).match(/^([A-Z][a-zA-Z0-9_]*)\s*\{/);
      if (tagMatch) {
        const bIdx = src.indexOf('{', i);
        const blk = extractBraceBlock(src, bIdx);
        i = blk.endIdx;
        arms.push({
          pattern,
          propertyAssignments: {},
          childNodes: [parseElementInterior(tagMatch[1], blk.body)],
        });
      } else {
        break;
      }
    }
  }

  return arms;
}

function parseElementInterior(tag: string, interior: string): FormaAstNode {
  const node: FormaAstNode = {
    kind: 'element',
    tag,
    properties: {},
    twoWayBindings: {},
    eventHandlers: {},
    appliedStyles: [],
    forwardedProps: [],
    children: [],
  };

  let i = 0;
  const src = interior.trim();
  const n = src.length;

  while (i < n) {
    while (i < n && /\s/.test(src[i])) i++;
    if (i >= n) break;

    if (src.startsWith('//', i)) {
      const nl = src.indexOf('\n', i + 2);
      i = nl === -1 ? n : nl + 1;
      continue;
    }

    // Check `forward props { a, b, c };`
    const forwardMatch = src.slice(i).match(/^forward\s+props\s*\{([^}]*)\}\s*;?/);
    if (forwardMatch) {
      const names = forwardMatch[1]
        .split(',')
        .map(s => s.trim())
        .filter(Boolean);
      node.forwardedProps.push(...names);
      i += forwardMatch[0].length;
      continue;
    }

    // Check `transition { duration: 200ms; easing: 'spring'; }`
    const transMatch = src.slice(i).match(/^transition\s*\{/);
    if (transMatch) {
      const bIdx = src.indexOf('{', i);
      const blk = extractBraceBlock(src, bIdx);
      i = blk.endIdx;
      node.transition = node.transition ?? {};
      for (const rawLine of blk.body.split(';')) {
        const line = rawLine.trim();
        if (!line) continue;
        const cIdx = line.indexOf(':');
        if (cIdx !== -1) {
          node.transition[line.slice(0, cIdx).trim()] = line.slice(cIdx + 1).trim();
        }
      }
      continue;
    }

    // Check nested control flow (`if`, `for`, `match`, `slot`, `ContentPresenter`) or child element (`TagName {`)
    if (
      src.startsWith('if ', i) ||
      src.startsWith('for ', i) ||
      src.startsWith('match ', i) ||
      src.startsWith('slot ', i) ||
      /^ContentPresenter\s*\{/.test(src.slice(i)) ||
      /^[A-Z][a-zA-Z0-9_]*\s*\{/.test(src.slice(i))
    ) {
      const remainingNodes = parseFormaUiBody(src.slice(i));
      node.children.push(...remainingNodes);
      break;
    }

    // Statement ending with `;`
    const semiIdx = findStatementSemicolon(src, i);
    const stmt = (semiIdx === -1 ? src.slice(i) : src.slice(i, semiIdx)).trim();
    i = semiIdx === -1 ? n : semiIdx + 1;

    if (!stmt) continue;

    // `apply styleName`
    if (stmt.startsWith('apply ')) {
      node.appliedStyles.push(stmt.slice(6).trim());
      continue;
    }

    // Two-way binding: `value <-> state.name`
    const twoWayIdx = stmt.indexOf('<->');
    if (twoWayIdx !== -1) {
      const propKey = stmt.slice(0, twoWayIdx).trim();
      const targetExpr = stmt.slice(twoWayIdx + 3).trim();
      node.twoWayBindings[propKey] = targetExpr;
      continue;
    }

    // Event action: `clicked -> state.increment('count', 1)` or `clicked -> events.saveRequested()`
    const arrowIdx = stmt.indexOf('->');
    if (arrowIdx !== -1) {
      const evtName = stmt.slice(0, arrowIdx).trim();
      const actionExpr = stmt.slice(arrowIdx + 2).trim();
      node.eventHandlers[evtName] = actionExpr;
      continue;
    }

    // Property assignment: `key: expr`
    const colonIdx = stmt.indexOf(':');
    if (colonIdx !== -1) {
      const propKey = stmt.slice(0, colonIdx).trim();
      const valExpr = stmt.slice(colonIdx + 1).trim();
      node.properties[propKey] = valExpr;
    }
  }

  return node;
}

function findStatementSemicolon(src: string, start: number): number {
  let i = start;
  const n = src.length;
  while (i < n) {
    const ch = src[i];
    if (ch === "'" || ch === '"') {
      const q = ch;
      i++;
      while (i < n && src[i] !== q) {
        if (src[i] === '\\') i++;
        i++;
      }
      i++;
      continue;
    }
    if (ch === ';') return i;
    i++;
  }
  return -1;
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

/** Read one contract token, including inline forms and quoted closing brackets. */
function readFormaContract(source: string, start: number): { content: string; end: number } | null {
  let quote: string | null = null;
  for (let i = start + 2; i < source.length; i++) {
    const ch = source[i];
    if (quote) {
      if (ch === '\\') {
        i++;
        continue;
      }
      if (ch === quote) quote = null;
    } else if (ch === "'" || ch === '"') quote = ch;
    else if (ch === ']') return { content: source.slice(start + 2, i).trim(), end: i + 1 };
  }
  return null;
}

/** Contracts belong to component declarations, never nested metadata or UI bodies. */
function assertFormaContractDepth(source: string): void {
  let depth = 0;
  for (let i = 0; i < source.length; i++) {
    const ch = source[i];
    if (ch === "'" || ch === '"') {
      const quote = ch;
      for (i++; i < source.length; i++) {
        if (source[i] === '\\') i++;
        else if (source[i] === quote) break;
      }
    } else if (source.startsWith('//', i)) {
      const end = source.indexOf('\n', i + 2);
      if (end === -1) break;
      i = end;
    } else if (source.startsWith('#[', i)) {
      if (depth !== 0)
        throw new Error('Forma contract annotation is unsupported inside nested blocks');
      const token = readFormaContract(source, i);
      if (!token) throw new Error('Unclosed Forma contract annotation');
      i = token.end - 1;
    } else if (ch === '{') depth++;
    else if (ch === '}') depth--;
  }
}

const FORMA_PATTERN_CONTRACT = /^pattern\(\s*(['"])((?:\\[\s\S]|(?!\1)[^\\])+)\1\s*\)$/;

/** Known contracts must have the same complete grammar in parser and validator. */
function formaContractSyntaxError(content: string): string | undefined {
  if (
    /^range\b/.test(content) &&
    !/^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/.test(content)
  ) {
    return 'Malformed Forma contract annotation: expected range(min, max)';
  }
  if (/^pattern\b/.test(content) && !FORMA_PATTERN_CONTRACT.test(content)) {
    return 'Malformed Forma contract annotation: expected pattern(quoted regex)';
  }
  const range = content.match(/^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/);
  const pattern = content.match(FORMA_PATTERN_CONTRACT);
  if (!range && !pattern) return 'Unsupported Forma contract annotation: ' + content;
  if (range) {
    const min = Number(range[1]),
      max = Number(range[2]);
    if (!Number.isFinite(min) || !Number.isFinite(max) || min > max) {
      return 'Invalid Forma contract annotation: range requires finite ordered bounds';
    }
  }
  if (pattern) {
    if (!isSafePatternRegex(pattern[2])) return 'Unsafe Forma pattern contract annotation';
    try {
      new RegExp(pattern[2]);
    } catch {
      return 'Invalid regular expression in Forma pattern contract annotation';
    }
  }
  return undefined;
}

const FORMA_AST_EXIT = Symbol('forma-ast-exit');
const FORMA_AST_STRINGS = [
  'tag',
  'slotName',
  'conditionExpr',
  'itemVar',
  'listExpr',
  'keyExpr',
  'matchExpr',
] as const;
const FORMA_AST_REQUIRED: Partial<Record<FormaAstNode['kind'], Array<keyof FormaAstNode>>> = {
  element: ['tag'],
  if: ['conditionExpr'],
  for: ['itemVar', 'listExpr'],
  match: ['matchExpr'],
  slot: ['slotName'],
};
/** Iterative DFS distinguishes cycles from reusable acyclic node references. */
function assertFormaAstTree(ast: FormaDocumentAst): void {
  const fail = (reason: string): never => {
    throw new Error('Invalid Forma AST tree: ' + reason);
  };
  const stringMap = (value: unknown): boolean => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
    for (const key in value)
      if (Object.hasOwn(value, key) && typeof (value as Record<string, unknown>)[key] !== 'string')
        return false;
    return true;
  };
  const strings = (value: unknown): boolean => {
    if (!Array.isArray(value)) return false;
    for (const entry of value) if (typeof entry !== 'string') return false;
    return true;
  };
  if (!Array.isArray(ast.rootAstNodes)) fail('expected rootAstNodes array');
  const active = new Set<FormaAstNode>(),
    done = new Set<FormaAstNode>();
  const stack: Array<FormaAstNode | typeof FORMA_AST_EXIT> = Array.from(ast.rootAstNodes);
  const pushBranch = (branch: FormaAstNode[] | undefined) => {
    if (branch === undefined) return;
    if (!Array.isArray(branch)) fail('expected branch array');
    for (const child of branch) stack.push(child);
  };
  while (stack.length) {
    const node = stack.pop()!;
    if (node === FORMA_AST_EXIT) {
      const finished = stack.pop() as FormaAstNode;
      active.delete(finished);
      done.add(finished);
      continue;
    }
    if (!node || typeof node !== 'object') fail('expected node object');
    if (active.has(node)) fail('cyclic node reference');
    if (done.has(node)) continue;
    switch (node.kind) {
      case 'element':
      case 'if':
      case 'for':
      case 'match':
      case 'slot':
      case 'contentPresenter':
        break;
      default:
        fail('unknown node kind');
    }
    if (
      !stringMap(node.properties) ||
      !stringMap(node.twoWayBindings) ||
      !stringMap(node.eventHandlers)
    )
      fail('expected expression maps');
    if (!strings(node.appliedStyles) || !strings(node.forwardedProps))
      fail('expected style/forward arrays');
    if (!Array.isArray(node.children)) fail('expected children array');
    if (node.transition !== undefined && !stringMap(node.transition))
      fail('invalid transition map');
    for (const key of FORMA_AST_STRINGS) {
      if (node[key] !== undefined && typeof node[key] !== 'string') fail('invalid ' + key);
    }
    for (const key of FORMA_AST_REQUIRED[node.kind] ?? []) {
      const value = node[key];
      if (typeof value !== 'string' || !value.trim()) fail('missing ' + node.kind + '.' + key);
    }
    if (node.keyExpr !== undefined && !node.keyExpr.trim()) fail('empty key expression');
    if (node.kind === 'match' && !Array.isArray(node.matchArms)) fail('missing match arms');
    active.add(node);
    stack.push(node, FORMA_AST_EXIT);
    pushBranch(node.children);
    pushBranch(node.elseChildren);
    pushBranch(node.emptyChildren);
    if (node.matchArms !== undefined) {
      if (!Array.isArray(node.matchArms)) fail('expected match arms array');
      for (const arm of node.matchArms) {
        if (
          !arm ||
          typeof arm.pattern !== 'string' ||
          !stringMap(arm.propertyAssignments) ||
          !Array.isArray(arm.childNodes)
        )
          fail('invalid match arm');
        pushBranch(arm.childNodes);
      }
    }
  }
}

function assertFormaAstMetadata(ast: FormaDocumentAst): void {
  const fail = (reason: string): never => {
    throw new Error('Invalid Forma AST document: ' + reason);
  };
  const record = (value: unknown): value is Record<string, unknown> =>
    !!value && typeof value === 'object' && !Array.isArray(value);
  if (typeof ast.componentName !== 'string' || !ast.componentName.trim())
    fail('missing component name');
  if (!record(ast.tokens) || !record(ast.initialStateValues)) fail('expected token/state maps');
  for (const map of [ast.styles, ast.i18nDictionaries]) {
    if (!record(map)) fail('expected style/i18n maps');
    for (const entry of Object.values(map)) {
      if (!record(entry) || Object.values(entry).some(value => typeof value !== 'string'))
        fail('expected style/i18n string maps');
    }
  }
  if (!Array.isArray(ast.letExpressions) || !Array.isArray(ast.shortcuts))
    fail('expected let/shortcut arrays');
  for (const name of Object.keys(ast.initialStateValues)) {
    if (!/^[A-Za-z0-9_]+$/.test(name) || FORBIDDEN_MEMBER_KEYS.has(name))
      fail('invalid state name');
  }
  const letNames = new Set<string>();
  for (const entry of ast.letExpressions) {
    if (
      !entry ||
      typeof entry.name !== 'string' ||
      !/^[A-Za-z0-9_]+$/.test(entry.name) ||
      FORBIDDEN_MEMBER_KEYS.has(entry.name) ||
      typeof entry.expr !== 'string' ||
      !entry.expr.trim()
    )
      fail('invalid let declaration');
    if (letNames.has(entry.name)) fail('duplicate let ' + entry.name);
    letNames.add(entry.name);
  }
  for (const entry of ast.shortcuts) {
    if (
      !entry ||
      typeof entry.combo !== 'string' ||
      !entry.combo.trim() ||
      typeof entry.action !== 'string' ||
      !entry.action.trim()
    )
      fail('invalid shortcut declaration');
  }
}

/** Validate AST contract data before construction or document mutation. */
function assertFormaAstContracts(ast: FormaDocumentAst): void {
  const fail = (reason: string): never => {
    throw new Error('Invalid Forma AST contract: ' + reason);
  };
  if (!ast || !Array.isArray(ast.propConstraints)) fail('expected propConstraints array');
  const names = new Set<string>();
  for (const c of ast.propConstraints) {
    if (
      !c ||
      typeof c.name !== 'string' ||
      !/^[A-Za-z0-9_]+$/.test(c.name) ||
      FORBIDDEN_MEMBER_KEYS.has(c.name)
    )
      fail('invalid prop name');
    if (names.has(c.name)) fail('duplicate prop ' + c.name);
    names.add(c.name);
    if (typeof c.type !== 'string' || !c.type.trim() || typeof c.required !== 'boolean')
      fail('invalid prop metadata');
    if (c.range !== undefined) {
      if (
        !Array.isArray(c.range) ||
        c.range.length !== 2 ||
        typeof c.range[0] !== 'number' ||
        !Number.isFinite(c.range[0]) ||
        typeof c.range[1] !== 'number' ||
        !Number.isFinite(c.range[1]) ||
        c.range[0] > c.range[1]
      ) {
        fail('range requires two finite ordered bounds');
      }
    }
    if (c.patternSource !== undefined) {
      if (
        typeof c.patternSource !== 'string' ||
        !c.patternSource.length ||
        !isSafePatternRegex(c.patternSource)
      )
        fail('invalid or unsafe pattern');
      try {
        new RegExp(c.patternSource);
      } catch {
        fail('invalid pattern regex');
      }
    }
  }
  assertFormaAstMetadata(ast);
  assertFormaAstTree(ast);
}

/** Own the admitted document; caller edits cannot change mounted/future views. */
function snapshotFormaAst(ast: FormaDocumentAst): FormaDocumentAst {
  assertFormaAstContracts(ast);
  const copies = new Map<FormaAstNode, FormaAstNode>();
  const pending: FormaAstNode[] = [];
  const copy = (node: FormaAstNode): FormaAstNode => {
    let result = copies.get(node);
    if (!result) {
      result = {
        kind: node.kind,
        tag: node.tag,
        slotName: node.slotName,
        conditionExpr: node.conditionExpr,
        itemVar: node.itemVar,
        listExpr: node.listExpr,
        keyExpr: node.keyExpr,
        matchExpr: node.matchExpr,
        properties: { ...node.properties },
        twoWayBindings: { ...node.twoWayBindings },
        eventHandlers: { ...node.eventHandlers },
        appliedStyles: [...node.appliedStyles],
        forwardedProps: [...node.forwardedProps],
        children: [],
        transition: node.transition === undefined ? undefined : { ...node.transition },
      };
      copies.set(node, result);
      pending.push(node);
    }
    return result;
  };
  const rootAstNodes = ast.rootAstNodes.map(copy);
  while (pending.length) {
    const node = pending.pop()!,
      result = copies.get(node)!;
    result.children = node.children.map(copy);
    result.elseChildren = node.elseChildren?.map(copy);
    result.emptyChildren = node.emptyChildren?.map(copy);
    result.matchArms = node.matchArms?.map(arm => ({
      pattern: arm.pattern,
      propertyAssignments: { ...arm.propertyAssignments },
      childNodes: arm.childNodes.map(copy),
    }));
  }
  const snapshot: FormaDocumentAst = {
    componentName: ast.componentName,
    rootAstNodes,
    tokens: structuredCloneIfObject(ast.tokens),
    styles: structuredCloneIfObject(ast.styles),
    i18nDictionaries: structuredCloneIfObject(ast.i18nDictionaries),
    initialStateValues: Object.fromEntries(
      Object.entries(ast.initialStateValues).map(([key, value]) => [
        key,
        structuredCloneIfObject(value),
      ]),
    ),
    propConstraints: ast.propConstraints.map(c => ({
      name: c.name,
      type: c.type,
      required: c.required,
      patternSource: c.patternSource,
      range: c.range === undefined ? undefined : [c.range[0], c.range[1]],
      defaultValue: structuredCloneIfObject(c.defaultValue),
    })),
    letExpressions: ast.letExpressions.map(entry => ({ name: entry.name, expr: entry.expr })),
    shortcuts: ast.shortcuts.map(entry => ({ combo: entry.combo, action: entry.action })),
  };
  assertFormaAstContracts(snapshot);
  return snapshot;
}

/**
 * Pre-parses a Forma `.ui` source string into a serializable `FormaDocumentAst` at build time (AOT)
 * so the browser can instantiate `compileFormaAst(ast, options)` with 0ms of runtime source parsing!
 */
export function parseFormaSource(rawUiSource: string): FormaDocumentAst {
  const cMatch = rawUiSource.match(/component\s+([A-Za-z0-9_]+)(?:\s*:\s*[A-Za-z0-9_]+)?\s*\{/);
  const componentName = cMatch ? cMatch[1] : 'AnonymousFormaComponent';
  const cBraceIdx = cMatch ? rawUiSource.indexOf('{', cMatch.index!) : -1;
  const compBody = cBraceIdx !== -1 ? extractBraceBlock(rawUiSource, cBraceIdx).body : rawUiSource;
  assertFormaContractDepth(compBody);

  const tokens: Record<string, any> = {};
  const i18nDictionaries: Record<string, Record<string, string>> = {};
  const styles: Record<string, Record<string, string>> = {};
  const propConstraints: FormaPropConstraint[] = [];
  const initialStateValues: Record<string, any> = Object.create(null);
  const letExpressions: Array<{ name: string; expr: string }> = [];
  const shortcuts: Array<{ combo: string; action: string }> = [];
  const rootAstNodes: FormaAstNode[] = [];

  let i = 0;
  const src = compBody.trim();
  const n = src.length;
  let pendingRange: [number, number] | undefined;
  let pendingPatternSource: string | undefined;

  while (i < n) {
    while (i < n && /\s/.test(src[i])) i++;
    if (i >= n) break;

    if (src.startsWith('//', i)) {
      const nl = src.indexOf('\n', i + 2);
      i = nl === -1 ? n : nl + 1;
      continue;
    }

    // Attribute annotations: `#[range(min, max)]` or `#[pattern('regex')]`
    if (src.startsWith('#[', i)) {
      const token = readFormaContract(src, i);
      if (!token) throw new Error('Unclosed Forma contract annotation');
      const syntaxError = formaContractSyntaxError(token.content);
      if (syntaxError) throw new Error(syntaxError);
      const rangeMatch = token.content.match(
        /^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/,
      );
      if (rangeMatch) {
        if (pendingRange !== undefined)
          throw new Error('Duplicate Forma range contract annotation');
        pendingRange = [Number(rangeMatch[1]), Number(rangeMatch[2])];
      }
      const patternMatch = token.content.match(FORMA_PATTERN_CONTRACT);
      if (patternMatch && pendingPatternSource !== undefined)
        throw new Error('Duplicate Forma pattern contract annotation');
      if (patternMatch && isSafePatternRegex(patternMatch[2])) {
        try {
          new RegExp(patternMatch[2]);
          pendingPatternSource = patternMatch[2];
        } catch {
          pendingPatternSource = undefined;
        }
      }
      i = token.end;
      continue;
    }

    if (
      (pendingRange !== undefined || pendingPatternSource !== undefined) &&
      !/^(?:required\s+)?prop\s+/.test(src.slice(i))
    ) {
      throw new Error('Forma contract annotation must immediately precede a prop declaration');
    }

    // `tokens { ... }`
    if (src.startsWith('tokens', i) && /^tokens\s*\{/.test(src.slice(i))) {
      const bIdx = src.indexOf('{', i);
      const blk = extractBraceBlock(src, bIdx);
      i = blk.endIdx;
      for (const rawLine of blk.body.split(';')) {
        const line = rawLine.trim();
        if (!line) continue;
        const cIdx = line.indexOf(':');
        if (cIdx !== -1) {
          const k = line.slice(0, cIdx).trim();
          const vRaw = line.slice(cIdx + 1).trim();
          const v = /^-?\d+(\.\d+)?$/.test(vRaw) ? Number(vRaw) : vRaw.replace(/^['"]|['"]$/g, '');
          setNestedObjectKey(tokens, k, v);
        }
      }
      continue;
    }

    // `i18n <locale> { ... }`
    const i18nMatch = src.slice(i).match(/^i18n\s+([a-zA-Z_-]+)\s*\{/);
    if (i18nMatch) {
      const loc = i18nMatch[1];
      const bIdx = src.indexOf('{', i);
      const blk = extractBraceBlock(src, bIdx);
      i = blk.endIdx;
      i18nDictionaries[loc] = i18nDictionaries[loc] ?? {};
      for (const rawLine of blk.body.split(';')) {
        const line = rawLine.trim();
        if (!line) continue;
        const cIdx = line.indexOf(':');
        if (cIdx !== -1) {
          const k = line.slice(0, cIdx).trim();
          const v = line
            .slice(cIdx + 1)
            .trim()
            .replace(/^['"]|['"]$/g, '');
          i18nDictionaries[loc][k] = v;
        }
      }
      continue;
    }

    // `style <name> { ... }`
    const styleMatch = src.slice(i).match(/^style\s+([a-zA-Z0-9_]+)\s*\{/);
    if (styleMatch) {
      const styleName = styleMatch[1];
      const bIdx = src.indexOf('{', i);
      const blk = extractBraceBlock(src, bIdx);
      i = blk.endIdx;
      styles[styleName] = {};
      for (const rawLine of blk.body.split(';')) {
        const line = rawLine.trim();
        if (!line) continue;
        const cIdx = line.indexOf(':');
        if (cIdx !== -1) {
          styles[styleName][line.slice(0, cIdx).trim()] = line.slice(cIdx + 1).trim();
        }
      }
      continue;
    }

    // If we hit a visual container (`Column {`, `Row {`, `Frame {`, `Grid {`, `Stack {`, `Text {`, `Button {`, etc.)
    if (/^[A-Z][a-zA-Z0-9_]*\s*\{/.test(src.slice(i))) {
      rootAstNodes.push(...parseFormaUiBody(src.slice(i)));
      break;
    }

    // Top-level statement ending with `;`
    const semiIdx = findStatementSemicolon(src, i);
    const stmt = (semiIdx === -1 ? src.slice(i) : src.slice(i, semiIdx)).trim();
    i = semiIdx === -1 ? n : semiIdx + 1;
    if (!stmt) continue;

    // `prop` or `required prop`
    const propMatch = stmt.match(
      /^(required\s+)?prop\s+([a-zA-Z0-9_]+)\s*:\s*([^=]+?)(?:\s*=\s*(.+))?$/,
    );
    if (propMatch) {
      const required = Boolean(propMatch[1]);
      const name = propMatch[2].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(name))
        throw new Error('Invalid Forma AST document: reserved declaration name');
      const type = propMatch[3].trim();
      const defRaw = propMatch[4]?.trim();
      const defVal = defRaw !== undefined ? parseLiteralValue(defRaw) : null;

      propConstraints.push({
        name,
        type,
        required,
        defaultValue: defVal,
        range: pendingRange,
        patternSource: pendingPatternSource,
      });
      pendingRange = undefined;
      pendingPatternSource = undefined;
      continue;
    }

    // `state <name>: <Type> = <default>`
    const stateMatch = stmt.match(/^state\s+([a-zA-Z0-9_]+)\s*:\s*([^=]+?)\s*=\s*(.+)$/);
    if (stateMatch) {
      const name = stateMatch[1].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(name))
        throw new Error('Invalid Forma AST document: reserved declaration name');
      const initVal = parseLiteralValue(stateMatch[3].trim());
      initialStateValues[name] = structuredCloneIfObject(initVal);
      continue;
    }

    // `let <name> = <expr>`
    const letMatch = stmt.match(/^let\s+([a-zA-Z0-9_]+)\s*=\s*(.+)$/);
    if (letMatch) {
      const letName = letMatch[1].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(letName))
        throw new Error('Invalid Forma AST document: reserved let name');
      letExpressions.push({ name: letName, expr: letMatch[2].trim() });
      continue;
    }

    // `shortcut '<combo>' -> <action>`
    const shortcutMatch = stmt.match(/^shortcut\s+['"](.+?)['"]\s*->\s*(.+)$/);
    if (shortcutMatch) {
      shortcuts.push({ combo: shortcutMatch[1].trim(), action: shortcutMatch[2].trim() });
      continue;
    }
  }

  if (pendingRange !== undefined || pendingPatternSource !== undefined) {
    throw new Error('Dangling Forma contract annotation: expected prop declaration');
  }

  const ast: FormaDocumentAst = {
    componentName,
    tokens,
    i18nDictionaries,
    styles,
    propConstraints,
    initialStateValues,
    letExpressions,
    shortcuts,
    rootAstNodes,
  };
  assertFormaAstContracts(ast);
  return ast;
}

export interface FormaCompileOptions {
  props?: Record<string, any>;
  state?: Record<string, any>;
  locale?: string;
  viewModel?: Record<string, FormaProperty<any> | Signal<any>>;
  slots?: FormaSlotMap;
  components?: Record<string, string | ((opts?: any) => FormaRuntimeInstance)>;
  actions?: Record<string, (...args: any[]) => void>;
  onAction?: (name: string, args: any[], event?: Event) => void;
  onBinding?: (path: string, value: any) => void;
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
    const propConstraints: Array<FormaPropConstraint & { compiledPattern?: RegExp }> = [];
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
          if (field in stateSignals && field in initialStateValues) {
            stateSignals[field].set(structuredCloneIfObject(initialStateValues[field]));
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
      for (let i = 0; i < children.length; i++) {
        insertChild(frag, renderAstNode(children[i], locals));
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
          for (let i = 0; i < compiledArms.length; i++) {
            if (compiledArms[i].matches(val)) {
              matchedArm = compiledArms[i].arm;
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
        if (styles[styleName]) {
          Object.assign(mergedProps, styles[styleName]);
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
          if (pKey in childInst.props) {
            effect(() => {
              const nextVal = evalAstNode(pAst, localScope);
              childInst.props[pKey].set(nextVal);
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

      const isButtonTag =
        tag === 'Button' ||
        tag === 'PrimaryButton' ||
        tag === 'GhostButton' ||
        tag === 'SegmentButton' ||
        tag === 'IconButton' ||
        tag === 'QuietIconButton' ||
        tag === 'SecondaryButton' ||
        tag === 'LeadingIconButton' ||
        tag === 'PrimaryIconButton' ||
        tag === 'TabButton';

      let domTag = 'div';
      const isMultilineInput =
        (tag === 'TextInput' || tag === 'TextField') &&
        Boolean(
          mergedProps.multiline && evaluateFormaExpression(mergedProps.multiline, localScope),
        );
      if (isButtonTag) domTag = 'button';
      else if (tag === 'TextInput' || tag === 'TextField')
        domTag = isMultilineInput ? 'textarea' : 'input';
      else if (tag === 'Slider') domTag = 'input';
      else if (
        tag === 'Text' ||
        tag === 'Span' ||
        tag === 'Badge' ||
        tag === 'Tag' ||
        tag === 'Chip' ||
        tag === 'Spacer'
      )
        domTag = 'span';
      else if (tag === 'Strong') domTag = 'strong';
      else if (tag === 'Paragraph') domTag = 'p';
      else if (tag === 'Keycap') domTag = 'kbd';
      else if (tag === 'StatusDot') domTag = 'i';
      else if (tag === 'Nav') domTag = 'nav';
      else if (tag === 'Footer') domTag = 'footer';
      else if (tag === 'Section') domTag = 'section';
      else if (tag === 'ProgressBar') domTag = 'progress';
      else if (tag === 'Checkbox') domTag = 'label';
      else if (tag === 'Divider' || tag === 'Separator') domTag = 'hr';

      const el =
        tag === 'SvgIcon'
          ? (document.createElementNS(
              'http://www.w3.org/2000/svg',
              'svg',
            ) as unknown as HTMLElement)
          : document.createElement(domTag);
      el.setAttribute('data-forma-type', tag);

      const baseClasses =
        tag === 'PrimaryButton'
          ? 'btn btn-primary forma-btn'
          : tag === 'IconButton' || tag === 'QuietIconButton'
            ? 'icon-btn forma-icon-btn'
            : tag === 'SegmentButton'
              ? 'forma-seg-btn'
              : isButtonTag
                ? 'btn forma-btn'
                : tag === 'Checkbox'
                  ? 'auto-check forma-checkbox'
                  : tag === 'Keycap'
                    ? 'forma-kbd'
                    : tag === 'Badge'
                      ? 'forma-badge'
                      : tag === 'StatusDot'
                        ? 'status-dot'
                        : tag === 'Spacer'
                          ? 'toolbar-spacer'
                          : tag === 'Nav'
                            ? 'forma-nav'
                            : tag === 'Footer'
                              ? 'forma-footer'
                              : tag === 'Section'
                                ? 'forma-section'
                                : tag === 'Row'
                                  ? 'forma-row'
                                  : tag === 'Column'
                                    ? 'forma-col'
                                    : '';

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

      const bindExpr = (rawExpr: string, applyVal: (val: any) => void) => {
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
          const strokeWidth =
            Number(strokeWidthAst ? evalAstNode(strokeWidthAst, localScope) : 2) || 2;
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

      // Base layout styles for Forma containers (`Row`, `Column`, `Grid`, `Stack`, `Frame`, `Modal`)
      if (tag === 'Row') {
        el.style.display = 'flex';
        el.style.flexDirection = 'row';
        el.style.alignItems = 'center';
      } else if (tag === 'Column' || tag === 'Frame') {
        el.style.display = 'flex';
        el.style.flexDirection = 'column';
      } else if (tag === 'Grid') {
        el.style.display = 'grid';
      } else if (tag === 'Stack') {
        el.style.display = 'grid';
      }

      // Register `key` for `getElementByKey`
      if (mergedProps.key) {
        const keyVal = String(evaluateFormaExpression(mergedProps.key, localScope));
        el.setAttribute('data-forma-key', keyVal);
        keyedElements.set(keyVal, el);
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
        } else if (propName === 'width' || propName === 'height') {
          bindExpr(rawExpr, v => {
            (el.style as any)[propName] = typeof v === 'number' ? `${v}px` : String(v);
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
      for (const [propName, targetPath] of Object.entries(ast.twoWayBindings)) {
        const cleanName = targetPath.replace(/^state\./, '').trim();
        const targetSig = ensureStateSignal(cleanName, propName === 'checked' ? false : '');
        if (targetSig && propName === 'value' && (domTag === 'input' || domTag === 'textarea')) {
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
            options?.onBinding?.(cleanName, nextVal);
            if (ast.eventHandlers.changed) {
              executeFormaAction(ast.eventHandlers.changed, locals, ev);
            }
          };
          inputEl.addEventListener('input', onInput);
          onCleanup(() => inputEl.removeEventListener('input', onInput));
        } else if (targetSig && propName === 'checked' && checkboxInput) {
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
            options?.onBinding?.(cleanName, nextChecked);
            if (ast.eventHandlers.changed) {
              executeFormaAction(ast.eventHandlers.changed, locals, ev);
            }
          };
          chkEl.addEventListener('change', onChange);
          onCleanup(() => chkEl.removeEventListener('change', onChange));
        }
      }

      // Event handlers: `clicked -> ...`, `keydown -> ...`, `changed -> ...`
      for (const [evtName, actionExpr] of Object.entries(ast.eventHandlers)) {
        if (evtName === 'clicked') {
          const onClick = (ev: MouseEvent) => {
            if (!(el as HTMLButtonElement).disabled) {
              executeFormaAction(actionExpr, locals, ev);
            }
          };
          el.addEventListener('click', onClick);
          onCleanup(() => el.removeEventListener('click', onClick));
        } else if (evtName === 'keydown') {
          const onKeydown = (ev: KeyboardEvent) => {
            if (!(el as HTMLButtonElement).disabled) {
              executeFormaAction(actionExpr, locals, ev);
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
            executeFormaAction(actionExpr, locals, ev);
          };
          targetEl.addEventListener('change', onChange);
          onCleanup(() => targetEl.removeEventListener('change', onChange));
        }
      }

      // Render children (and evaluate any property-group `match` blocks applied to this element!)
      for (const childAst of ast.children) {
        if (
          childAst.kind === 'match' &&
          childAst.matchArms &&
          childAst.matchArms.every(a => a.childNodes.length === 0)
        ) {
          const propMatchAst = parseFormaExprAst(childAst.matchExpr!);
          const compiledPropArms = childAst.matchArms.map(a => ({
            matches: compileFormaPatternMatcher(a.pattern),
            assignments: Object.entries(a.propertyAssignments).map(
              ([pKey, pExpr]) => [pKey, parseFormaExprAst(pExpr)] as [string, ExprAst],
            ),
          }));
          effect(() => {
            const val = evalAstNode(propMatchAst, localScope);
            for (let i = 0; i < compiledPropArms.length; i++) {
              const arm = compiledPropArms[i];
              if (arm.matches(val)) {
                for (let j = 0; j < arm.assignments.length; j++) {
                  const [pKey, pAst] = arm.assignments[j];
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
          insertChild(el, renderAstNode(childAst, locals));
        }
      }

      return el;
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
              if (k in stateSignals) {
                stateSignals[k].set(structuredCloneIfObject(v));
              }
            }
          }
          if (snapshot.props) {
            for (const [k, v] of Object.entries(snapshot.props)) {
              if (k in propSignals) {
                propSignals[k].set(structuredCloneIfObject(v));
              }
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
                  const keyLower = parts[parts.length - 1].toLowerCase();
                  const needsMod =
                    parts.includes('Mod') || parts.includes('Ctrl') || parts.includes('Meta');
                  return { keyLower, needsMod, action: sc.action };
                });
                const onKeyDown = (e: KeyboardEvent) => {
                  const modPressed = e.metaKey || e.ctrlKey;
                  const keyLower = e.key.toLowerCase();
                  for (let i = 0; i < compiledShortcuts.length; i++) {
                    const sc = compiledShortcuts[i];
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

function parseLiteralValue(raw: string): any {
  const s = raw.trim();
  if (s === 'true') return true;
  if (s === 'false') return false;
  if (s === 'null') return null;
  if (/^-?\d+(\.\d+)?$/.test(s)) return Number(s);
  if ((s.startsWith("'") && s.endsWith("'")) || (s.startsWith('"') && s.endsWith('"'))) {
    const inner = s.slice(1, -1);
    let out = '';
    for (let i = 0; i < inner.length; i++) {
      if (inner[i] === '\\' && i + 1 < inner.length) {
        const next = inner[++i];
        if (next === 'n') out += '\n';
        else if (next === 'r') out += '\r';
        else if (next === 't') out += '\t';
        else out += next;
      } else {
        out += inner[i];
      }
    }
    return out;
  }
  if (s.startsWith('[') || s.startsWith('{')) {
    try {
      const normalizedJson = s.replace(/'/g, '"').replace(/([a-zA-Z0-9_]+)\s*:/g, '"$1":');
      return JSON.parse(normalizedJson, (key, val) =>
        FORBIDDEN_MEMBER_KEYS.has(key) ? undefined : val,
      );
    } catch {
      return [];
    }
  }
  return s;
}

/**
 * Performs AOT / live-studio static analysis and validation on a Forma `.ui` source string.
 * Checks balanced braces, `component Name { ... }` header, `#[range]` / `#[pattern]` contracts,
 * `state`, `prop`, `let`, and 3D viewport nodes without executing DOM side effects.
 */
export function validateFormaSource(uiSource: string): FormaValidationReport {
  const diagnostics: FormaDiagnostic[] = [];
  const trimmed = uiSource.trim();

  if (!trimmed) {
    diagnostics.push({
      severity: 'error',
      message: 'Пустой файл Forma .ui: ожидается объявление component Name { ... }',
      line: 1,
    });
    return {
      valid: false,
      componentName: 'Unknown',
      diagnostics,
      propCount: 0,
      stateCount: 0,
      letCount: 0,
      has3DViewport: false,
    };
  }

  const compMatch = uiSource.match(/component\s+([A-Za-z0-9_]+)(?:\s*:\s*[A-Za-z0-9_]+)?\s*\{/);
  const componentName = compMatch ? compMatch[1] : 'AnonymousFormaComponent';
  if (!compMatch) {
    diagnostics.push({
      severity: 'warning',
      message:
        'Отсутствует явный заголовок `component <Name> { ... }` (используется анонимный режим)',
      line: 1,
    });
  }

  const contracts: Array<{ text: string; line: number }> = [];
  // Verify balanced braces `{` and `}` ignoring string literals
  let depth = 0;
  let inQuote: string | null = null;
  let lineNum = 1;
  for (let i = 0; i < uiSource.length; i++) {
    const ch = uiSource[i];
    if (ch === '\n') lineNum++;
    if (inQuote) {
      if (ch === '\\') {
        i++;
        continue;
      }
      if (ch === inQuote) inQuote = null;
      continue;
    }
    if (ch === "'" || ch === '"') {
      inQuote = ch;
      continue;
    }
    if (ch === '/' && uiSource[i + 1] === '/') {
      const nl = uiSource.indexOf('\n', i + 2);
      if (nl === -1) break;
      lineNum++;
      i = nl;
      continue;
    }
    if (ch === '#' && uiSource[i + 1] === '[') {
      const token = readFormaContract(uiSource, i);
      if (!token) {
        contracts.push({ text: uiSource.slice(i), line: lineNum });
        break;
      }
      const text = uiSource.slice(i, token.end);
      contracts.push({ text, line: lineNum });
      lineNum += (text.match(/\n/g) ?? []).length;
      i = token.end - 1;
      continue;
    }
    if (ch === '{') depth++;
    else if (ch === '}') {
      depth--;
      if (depth < 0) {
        diagnostics.push({
          severity: 'error',
          message: `Лишняя закрывающая фигурная скобка '}' на строке ${lineNum}`,
          line: lineNum,
        });
        break;
      }
    }
  }

  if (depth > 0) {
    diagnostics.push({
      severity: 'error',
      message: `Незакрытый блок '{' (ожидается ${depth} закрывающих '}')`,
      line: lineNum,
    });
  }

  // Validate `#[range(min, max)]` and `#[pattern('...')]` attributes
  contracts.forEach(({ text: s, line }) => {
    const idx = line - 1;
    if (s.startsWith('#[')) {
      if (!s.endsWith(']')) {
        diagnostics.push({
          severity: 'error',
          message: `Незакрытый атрибут контракта '${s}' на строке ${idx + 1}`,
          line: idx + 1,
        });
      } else {
        const inner = s.slice(2, -1).trim();
        const syntaxError = formaContractSyntaxError(inner);
        if (syntaxError) {
          diagnostics.push({ severity: 'error', message: syntaxError, line });
          return;
        }
        const rMatch = inner.match(/^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/);
        if (rMatch && Number(rMatch[1]) > Number(rMatch[2])) {
          diagnostics.push({
            severity: 'error',
            message: `Некорректный контракт #[range]: минимум (${rMatch[1]}) больше максимума (${rMatch[2]})`,
            line: idx + 1,
          });
        }
        const pMatch = inner.match(FORMA_PATTERN_CONTRACT);
        if (pMatch) {
          if (!isSafePatternRegex(pMatch[2])) {
            diagnostics.push({
              severity: 'error',
              message: `Небезопасное регулярное выражение в #[pattern] (возможен ReDoS): ${pMatch[2]}`,
              line: idx + 1,
            });
          } else {
            try {
              new RegExp(pMatch[2]);
            } catch (e: any) {
              diagnostics.push({
                severity: 'error',
                message: `Ошибка регулярного выражения в #[pattern]: ${e?.message ?? pMatch[2]}`,
                line: idx + 1,
              });
            }
          }
        }
      }
    }
  });

  let precompiledAst: FormaDocumentAst | undefined;
  if (!diagnostics.some(d => d.severity === 'error')) {
    try {
      precompiledAst = parseFormaSource(uiSource);
    } catch (error) {
      diagnostics.push({
        severity: 'error',
        message: error instanceof Error ? error.message : String(error),
        line: contracts[0]?.line ?? 1,
      });
    }
  }

  const propCount = precompiledAst
    ? precompiledAst.propConstraints.length
    : (uiSource.match(/(?:^|\s)(?:required\s+)?prop\s+[a-zA-Z0-9_]+\s*:/gm) ?? []).length;
  const stateCount = precompiledAst
    ? Object.keys(precompiledAst.initialStateValues).length
    : (uiSource.match(/(?:^|\s)state\s+[a-zA-Z0-9_]+\s*:/gm) ?? []).length;
  const letCount = precompiledAst
    ? precompiledAst.letExpressions.length
    : (uiSource.match(/(?:^|\s)let\s+[a-zA-Z0-9_]+\s*=/gm) ?? []).length;
  const has3DViewport = /\b(Scene3D|OrbitCube3D|Viewport3D)\s*\{/.test(uiSource);

  const hasError = diagnostics.some(d => d.severity === 'error');
  const report: FormaValidationReport = {
    valid: !hasError,
    componentName,
    diagnostics,
    propCount,
    stateCount,
    letCount,
    has3DViewport,
  };
  if (precompiledAst) {
    Object.defineProperty(report, '_ast', { value: precompiledAst, enumerable: false });
  }
  return report;
}

/**
 * Generates a True Pre-Parsed AST AOT JavaScript/ES Module from a Forma `.ui` source file.
 * Both `uiAst` (`parseFormaSource`) and `uiValidation` (`validateFormaSource`) are computed
 * at build time so the browser runs `compileFormaAst(uiAst, options)` with 0ms of source parsing!
 */
export interface FormaModuleOptions {
  /**
   * Keep the original `.ui` text in the module as `uiSource` (default). Build tools turn it off for
   * production output: the runtime only needs `uiAst`, and the text would otherwise ship twice.
   */
  embedSource?: boolean;
}

export function compileFormaToJsModule(
  uiSource: string,
  importSource = 'svet-engine',
  moduleOptions: FormaModuleOptions = {},
): string {
  const report = validateFormaSource(uiSource);
  const precompiledAst =
    (report as FormaValidationReport & { _ast?: FormaDocumentAst })._ast ??
    parseFormaSource(uiSource);
  return `import { instantiateFormaAst as compileFormaAst } from ${JSON.stringify(importSource)};
export const uiSource = ${moduleOptions.embedSource === false ? 'undefined' : JSON.stringify(uiSource)};
export const uiAst = ${JSON.stringify(precompiledAst)};
export const uiValidation = ${JSON.stringify(report)};
const _activeInstances = import.meta.hot ? (import.meta.hot.data.instances ??= new Set()) : null;
const _hmrAst = import.meta.hot ? (import.meta.hot.data.currentAst ??= { value: uiAst }) : null;
export default function createFormaView(options) {
  const inst = _hmrAst ? compileFormaAst(_hmrAst.value, options) : compileFormaAst(uiAst, options);
  let mountedViews = 0;
  const origMount = inst.mount.bind(inst);
  const origDispose = inst.dispose.bind(inst);
  inst.dispose = () => {
    _activeInstances?.delete(inst);
    origDispose();
  };
  inst.mount = (container) => {
    const handle = origMount(container);
    mountedViews++;
    _activeInstances?.add(inst);
    let unmounted = false;
    return {
      unmount() {
        if (unmounted) return;
        unmounted = true;
        try { handle.unmount(); }
        finally {
          if (--mountedViews === 0) _activeInstances?.delete(inst);
        }
      },
    };
  };
  return inst;
}
if (import.meta.hot) {
  import.meta.hot.accept((newMod) => {
    if (newMod) {
      if (newMod.uiValidation?.valid === false) {
        const messages = (newMod.uiValidation.diagnostics ?? []).map((item) => item.message).join('; ');
        throw new Error('Invalid Forma HMR module: ' + messages);
      }
      const nextAst = newMod.uiAst;
      const instances = [..._activeInstances];
      _hmrAst.value = nextAst;
      let failed = false, failure;
      for (const inst of instances) {
        if (!_activeInstances.has(inst)) continue;
        try { inst.hotReload(nextAst); }
        catch (error) { if (!failed) failure = error; failed = true; }
      }
      if (failed) throw failure;
    }
  });
}
`;
}

/**
 * First-class Svet JSX component (`<FormaHost source={...} />` or `<FormaHost instance={...} />`)
 * that embeds a Forma `.ui` interface directly inside a Svet application tree!
 */
export function FormaHost(props: {
  source?: string;
  instance?: FormaRuntimeInstance;
  props?: Record<string, any>;
  state?: Record<string, any>;
  locale?: string;
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
