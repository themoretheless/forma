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

import { For, h, insertChild, Show } from './dom.ts';
import {
  Canvas3D,
  Viewport3D,
  createCubeMesh3D,
  createPerspectiveCamera3D,
  createTransform3D,
} from './gpu.ts';
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
  parentContext?: FormaContextHolder<T>
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

function parseHexToRgb(hex: string): [number, number, number] {
  const clean = hex.replace(/^#/, '').trim();
  if (clean.length === 3) {
    return [
      parseInt(clean[0] + clean[0], 16),
      parseInt(clean[1] + clean[1], 16),
      parseInt(clean[2] + clean[2], 16),
    ];
  }
  const num = parseInt(clean.slice(0, 6), 16) || 0;
  return [(num >> 16) & 255, (num >> 8) & 255, num & 255];
}

function rgbToHex(r: number, g: number, b: number): string {
  const clampByte = (n: number) => Math.max(0, Math.min(255, Math.round(n)));
  return (
    '#' +
    [clampByte(r), clampByte(g), clampByte(b)]
      .map((x) => x.toString(16).padStart(2, '0'))
      .join('')
  );
}

function relativeLuminance(hex: string): number {
  const [r, g, b] = parseHexToRgb(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export const formaStdlib: Record<string, (...args: any[]) => any> = {
  // Math
  min: (...args: number[]) => Math.min(...args),
  max: (...args: number[]) => Math.max(...args),
  clamp: (val: number, min: number, max: number) => Math.min(Math.max(val, min), max),
  abs: (val: number) => Math.abs(val),
  round: (val: number) => Math.round(val),
  floor: (val: number) => Math.floor(val),
  ceil: (val: number) => Math.ceil(val),
  pow: (a: number, b: number) => Math.pow(a, b),
  sqrt: (val: number) => Math.sqrt(val),
  sign: (val: number) => Math.sign(val),
  lerp: (a: number, b: number, t: number) => a + (b - a) * t,
  remap: (v: number, inMin: number, inMax: number, outMin: number, outMax: number) =>
    outMin + ((v - inMin) / (inMax - inMin || 1)) * (outMax - outMin),
  sum: (arr: number[]) => (Array.isArray(arr) ? arr.reduce((a, b) => a + Number(b), 0) : 0),
  avg: (arr: number[]) =>
    Array.isArray(arr) && arr.length > 0
      ? arr.reduce((a, b) => a + Number(b), 0) / arr.length
      : 0,

  // Strings & i18n
  len: (v: any) => (v == null ? 0 : typeof v === 'string' || Array.isArray(v) ? v.length : Object.keys(v).length),
  trim: (s: string) => String(s ?? '').trim(),
  upper: (s: string) => String(s ?? '').toUpperCase(),
  toUpperCase: (s: string) => String(s ?? '').toUpperCase(),
  lower: (s: string) => String(s ?? '').toLowerCase(),
  toLowerCase: (s: string) => String(s ?? '').toLowerCase(),
  capitalize: (s: string) => {
    const str = String(s ?? '');
    return str.charAt(0).toUpperCase() + str.slice(1);
  },
  contains: (s: any, sub: any) =>
    Array.isArray(s) ? s.includes(sub) : String(s ?? '').includes(String(sub)),
  startsWith: (s: string, prefix: string) => String(s ?? '').startsWith(prefix),
  endsWith: (s: string, suffix: string) => String(s ?? '').endsWith(suffix),
  replace: (s: string, from: string, to: string) => String(s ?? '').replaceAll(from, to),
  slice: (s: any, start: number, end?: number) => (s ? s.slice(start, end) : ''),
  split: (s: string, sep: string) => String(s ?? '').split(sep),
  padStart: (s: string, targetLen: number, pad = ' ') => String(s ?? '').padStart(targetLen, pad),
  padEnd: (s: string, targetLen: number, pad = ' ') => String(s ?? '').padEnd(targetLen, pad),
  repeat: (s: string, count: number) => String(s ?? '').repeat(Math.max(0, count)),
  truncate: (s: string, maxLen: number, ellipsis = '…') => {
    const str = String(s ?? '');
    return str.length > maxLen ? str.slice(0, Math.max(0, maxLen - ellipsis.length)) + ellipsis : str;
  },
  plural: (count: number, one: string, few: string, many?: string) => {
    const n = Math.abs(Math.trunc(Number(count) || 0));
    if (many === undefined) {
      return n === 1 ? one : few;
    }
    const mod10 = n % 10;
    const mod100 = n % 100;
    if (mod10 === 1 && mod100 !== 11) return one;
    if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return few;
    return many;
  },

  // Lists & Records
  count: (arr: any[]) => (Array.isArray(arr) ? arr.length : 0),
  first: (arr: any[]) => (Array.isArray(arr) ? arr[0] : undefined),
  last: (arr: any[]) => (Array.isArray(arr) && arr.length > 0 ? arr[arr.length - 1] : undefined),
  join: (arr: any[], sep = ', ') => (Array.isArray(arr) ? arr.join(sep) : ''),
  reverse: (arr: any[]) => (Array.isArray(arr) ? [...arr].reverse() : []),
  take: (arr: any[], n: number) => (Array.isArray(arr) ? arr.slice(0, n) : []),
  skip: (arr: any[], n: number) => (Array.isArray(arr) ? arr.slice(n) : []),
  map: (arr: any[], fieldOrFn: string | ((x: any) => any)) => {
    if (!Array.isArray(arr)) return [];
    if (typeof fieldOrFn === 'function') return arr.map(fieldOrFn);
    return arr.map((item) => (item && typeof item === 'object' ? item[fieldOrFn] : item));
  },
  filter: (arr: any[], fieldOrFn: string | ((x: any) => boolean)) => {
    if (!Array.isArray(arr)) return [];
    if (typeof fieldOrFn === 'function') return arr.filter(fieldOrFn);
    return arr.filter((item) => Boolean(item && typeof item === 'object' ? item[fieldOrFn] : item));
  },
  unique: (arr: any[]) => (Array.isArray(arr) ? Array.from(new Set(arr)) : []),
  range: (start: number, end: number) =>
    Array.from({ length: Math.max(0, end - start) }, (_, i) => start + i),
  keys: (obj: Record<string, any>) => (obj && typeof obj === 'object' ? Object.keys(obj) : []),
  values: (obj: Record<string, any>) => (obj && typeof obj === 'object' ? Object.values(obj) : []),

  // Predicates & Casts
  String: (v: any) => String(v ?? ''),
  Number: (v: any) => Number(v ?? 0),
  Bool: (v: any) => Boolean(v),
  isEmpty: (v: any) =>
    v == null || v === '' || (Array.isArray(v) && v.length === 0) || (typeof v === 'object' && Object.keys(v).length === 0),
  isNull: (v: any) => v === null || v === undefined,
  coalesce: (...args: any[]) => args.find((a) => a !== null && a !== undefined),

  // WCAG Color Functions
  rgb: (r: number, g: number, b: number) => rgbToHex(r, g, b),
  rgba: (r: number, g: number, b: number, a: number) => `rgba(${r}, ${g}, ${b}, ${a})`,
  alpha: (hex: string, a: number) => {
    const [r, g, b] = parseHexToRgb(hex);
    return `rgba(${r}, ${g}, ${b}, ${a})`;
  },
  lighten: (hex: string, amount: number) => {
    const [r, g, b] = parseHexToRgb(hex);
    const factor = amount <= 1 ? amount * 255 : amount;
    return rgbToHex(r + factor, g + factor, b + factor);
  },
  darken: (hex: string, amount: number) => {
    const [r, g, b] = parseHexToRgb(hex);
    const factor = amount <= 1 ? amount * 255 : amount;
    return rgbToHex(r - factor, g - factor, b - factor);
  },
  luminance: (hex: string) => Number(relativeLuminance(hex).toFixed(4)),
  contrast: (hexA: string, hexB: string) => {
    const l1 = relativeLuminance(hexA);
    const l2 = relativeLuminance(hexB);
    const brightest = Math.max(l1, l2);
    const darkest = Math.min(l1, l2);
    return Number(((brightest + 0.05) / (darkest + 0.05)).toFixed(2));
  },
};

/**
 * Transforms Forma pipe syntax `a |> fn(b, c) |> fn2()` into standard nested calls `fn2(fn(a, b, c))`.
 */
export function transformFormaPipes(expr: string): string {
  if (!expr.includes('|>')) return expr;
  const stages = expr.split('|>').map((s) => s.trim());
  let acc = stages[0];
  for (let i = 1; i < stages.length; i++) {
    const stage = stages[i];
    const openParen = stage.indexOf('(');
    if (openParen !== -1 && stage.endsWith(')')) {
      const fnName = stage.slice(0, openParen).trim();
      const argsInner = stage.slice(openParen + 1, -1).trim();
      acc = argsInner.length > 0 ? `${fnName}(${acc}, ${argsInner})` : `${fnName}(${acc})`;
    } else {
      acc = `${stage}(${acc})`;
    }
  }
  return acc;
}

export interface FormaEvalScope {
  state: Record<string, Signal<any>>;
  props: Record<string, Signal<any>>;
  computedLets: Record<string, ReadonlySignal<any>>;
  tokens: Record<string, any>;
  i18n: ReadonlySignal<Record<string, string>>;
  isValid: ReadonlySignal<boolean>;
  locals?: Record<string, any>;
}

// ============================================================================
// CSP-Safe AST Expression Parser & Evaluator (Zero `new Function` / Zero `with`)
// ============================================================================

type ExprAst =
  | { type: 'literal'; value: any }
  | { type: 'interp'; template: string }
  | { type: 'ident'; name: string }
  | { type: 'member'; object: ExprAst; property: string }
  | { type: 'index'; object: ExprAst; index: ExprAst }
  | { type: 'call'; callee: ExprAst; args: ExprAst[] }
  | { type: 'unary'; op: string; arg: ExprAst }
  | { type: 'binary'; op: string; left: ExprAst; right: ExprAst }
  | { type: 'conditional'; test: ExprAst; consequent: ExprAst; alternate: ExprAst }
  | { type: 'array'; elements: ExprAst[] };

interface ExprToken {
  kind: 'num' | 'str' | 'ident' | 'op' | 'punc';
  val: string;
}

const _exprAstCache = new Map<string, ExprAst>();
const MAX_EXPR_AST_CACHE_SIZE = 2048;

const FORBIDDEN_MEMBER_KEYS = new Set([
  '__proto__',
  'prototype',
  'constructor',
  '__defineGetter__',
  '__defineSetter__',
  '__lookupGetter__',
  '__lookupSetter__',
]);

function isSafePatternRegex(pattern: string): boolean {
  if (pattern.length > 256) return false;
  // Reject nested quantifiers (`(a+)+`, `(a*)*`, `(\w+)+`) that cause catastrophic ReDoS backtracking
  if (/\([^()]*[+*][^()]*\)[+*?]/.test(pattern)) {
    return false;
  }
  return true;
}

function tokenizeFormaExpr(src: string): ExprToken[] {
  const tokens: ExprToken[] = [];
  let i = 0;
  const n = src.length;

  while (i < n) {
    const ch = src[i];
    if (/\s/.test(ch)) {
      i++;
      continue;
    }

    if (ch === "'" || ch === '"') {
      const q = ch;
      let s = '';
      i++;
      while (i < n && src[i] !== q) {
        if (src[i] === '\\' && i + 1 < n) {
          if (src[i + 1] === '$') {
            s += '\\$';
            i += 2;
            continue;
          }
          i++;
        }
        s += src[i++];
      }
      i++;
      tokens.push({ kind: 'str', val: s });
      continue;
    }

    if (/\d/.test(ch)) {
      let numStr = '';
      while (i < n && /[0-9.]/.test(src[i])) {
        numStr += src[i++];
      }
      // Consume optional Forma unit (`px`, `ms`, `s`, `deg`, `rad`, `rem`, `em`, `%`, `turn`)
      let unit = '';
      while (i < n && /[a-zA-Z%]/.test(src[i])) {
        unit += src[i++];
      }
      if (unit && unit !== 'px' && unit !== 'ms') {
        tokens.push({ kind: 'str', val: numStr + unit });
      } else {
        tokens.push({ kind: 'num', val: numStr });
      }
      continue;
    }

    // Multi-char operators
    const three = src.slice(i, i + 3);
    if (three === '===' || three === '!==') {
      tokens.push({ kind: 'op', val: three });
      i += 3;
      continue;
    }
    const two = src.slice(i, i + 2);
    if (['==', '!=', '<=', '>=', '&&', '||', '??'].includes(two)) {
      tokens.push({ kind: 'op', val: two });
      i += 2;
      continue;
    }

    if ('+-*/%<>!?:'.includes(ch)) {
      tokens.push({ kind: 'op', val: ch });
      i++;
      continue;
    }

    if ('()[].,'.includes(ch)) {
      tokens.push({ kind: 'punc', val: ch });
      i++;
      continue;
    }

    if (/[a-zA-Z_$#]/.test(ch)) {
      let id = '';
      while (i < n && /[a-zA-Z0-9_$#-]/.test(src[i])) {
        id += src[i++];
      }
      tokens.push({ kind: 'ident', val: id });
      continue;
    }

    i++;
  }

  return tokens;
}

function parseFormaExprAst(raw: string): ExprAst {
  const cached = _exprAstCache.get(raw);
  if (cached) return cached;

  const transformed = transformFormaPipes(raw);
  const tokens = tokenizeFormaExpr(transformed);
  let pos = 0;

  const peek = (): ExprToken | undefined => tokens[pos];
  const next = (): ExprToken | undefined => tokens[pos++];

  const parsePrimary = (): ExprAst => {
    const tok = next();
    if (!tok) return { type: 'literal', value: undefined };

    if (tok.kind === 'num') {
      return { type: 'literal', value: Number(tok.val) };
    }
    if (tok.kind === 'str') {
      if (tok.val.includes('${')) {
        return { type: 'interp', template: tok.val };
      }
      return { type: 'literal', value: tok.val };
    }
    if (tok.kind === 'ident') {
      if (tok.val === 'true') return { type: 'literal', value: true };
      if (tok.val === 'false') return { type: 'literal', value: false };
      if (tok.val === 'null') return { type: 'literal', value: null };
      if (tok.val === 'undefined') return { type: 'literal', value: undefined };
      if (tok.val.startsWith('#')) return { type: 'literal', value: tok.val };
      return { type: 'ident', name: tok.val };
    }
    if (tok.kind === 'punc' && tok.val === '(') {
      const inner = parseTernary();
      if (peek()?.val === ')') next();
      return inner;
    }
    if (tok.kind === 'punc' && tok.val === '[') {
      const elements: ExprAst[] = [];
      while (peek() && peek()!.val !== ']') {
        elements.push(parseTernary());
        if (peek()?.val === ',') next();
      }
      if (peek()?.val === ']') next();
      return { type: 'array', elements };
    }
    return { type: 'literal', value: undefined };
  };

  const parseMemberAndCall = (): ExprAst => {
    let node = parsePrimary();
    while (peek()) {
      const p = peek()!;
      if (p.kind === 'punc' && p.val === '.') {
        next();
        const propTok = next();
        if (propTok) {
          node = { type: 'member', object: node, property: propTok.val };
        }
      } else if (p.kind === 'punc' && p.val === '[') {
        next();
        const idxExpr = parseTernary();
        if (peek()?.val === ']') next();
        node = { type: 'index', object: node, index: idxExpr };
      } else if (p.kind === 'punc' && p.val === '(') {
        next();
        const args: ExprAst[] = [];
        while (peek() && peek()!.val !== ')') {
          args.push(parseTernary());
          if (peek()?.val === ',') next();
        }
        if (peek()?.val === ')') next();
        node = { type: 'call', callee: node, args };
      } else {
        break;
      }
    }
    return node;
  };

  const parseUnary = (): ExprAst => {
    const p = peek();
    if (p && p.kind === 'op' && (p.val === '!' || p.val === '-' || p.val === '+')) {
      next();
      return { type: 'unary', op: p.val, arg: parseUnary() };
    }
    return parseMemberAndCall();
  };

  const parseMultiplicative = (): ExprAst => {
    let left = parseUnary();
    while (peek() && ['*', '/', '%'].includes(peek()!.val)) {
      const op = next()!.val;
      left = { type: 'binary', op, left, right: parseUnary() };
    }
    return left;
  };

  const parseAdditive = (): ExprAst => {
    let left = parseMultiplicative();
    while (peek() && ['+', '-'].includes(peek()!.val)) {
      const op = next()!.val;
      left = { type: 'binary', op, left, right: parseMultiplicative() };
    }
    return left;
  };

  const parseRelational = (): ExprAst => {
    let left = parseAdditive();
    while (peek() && ['<', '>', '<=', '>='].includes(peek()!.val)) {
      const op = next()!.val;
      left = { type: 'binary', op, left, right: parseAdditive() };
    }
    return left;
  };

  const parseEquality = (): ExprAst => {
    let left = parseRelational();
    while (peek() && ['==', '!=', '===', '!=='].includes(peek()!.val)) {
      const op = next()!.val;
      left = { type: 'binary', op, left, right: parseRelational() };
    }
    return left;
  };

  const parseLogicalAnd = (): ExprAst => {
    let left = parseEquality();
    while (peek() && peek()!.val === '&&') {
      const op = next()!.val;
      left = { type: 'binary', op, left, right: parseEquality() };
    }
    return left;
  };

  const parseLogicalOr = (): ExprAst => {
    let left = parseLogicalAnd();
    while (peek() && (peek()!.val === '||' || peek()!.val === '??')) {
      const op = next()!.val;
      left = { type: 'binary', op, left, right: parseLogicalAnd() };
    }
    return left;
  };

  const parseTernary = (): ExprAst => {
    const test = parseLogicalOr();
    if (peek() && peek()!.val === '?') {
      next();
      const consequent = parseTernary();
      if (peek()?.val === ':') next();
      const alternate = parseTernary();
      return { type: 'conditional', test, consequent, alternate };
    }
    return test;
  };

  const ast = parseTernary();
  if (_exprAstCache.size >= MAX_EXPR_AST_CACHE_SIZE) {
    const oldestKey = _exprAstCache.keys().next().value;
    if (oldestKey !== undefined) {
      _exprAstCache.delete(oldestKey);
    }
  }
  _exprAstCache.set(raw, ast);
  return ast;
}

function evalAstNode(node: ExprAst, scope: FormaEvalScope): any {
  switch (node.type) {
    case 'literal':
      return node.value;

    case 'interp':
      return node.template.replace(/\\?\$\{([^}]+)\}/g, (match, subExpr) => {
        if (match.startsWith('\\')) {
          return match.slice(1);
        }
        const val = evaluateFormaExpression(subExpr, scope);
        return val == null ? '' : String(val);
      });

    case 'array':
      return node.elements.map((el) => evalAstNode(el, scope));

    case 'ident': {
      const name = node.name;
      if (FORBIDDEN_MEMBER_KEYS.has(name)) return undefined;
      if (name === 'isValid') return scope.isValid();
      if (name === 'tokens') return scope.tokens;
      if (name === 'i18n') return scope.i18n();
      if (name === 'state') return scope.state;
      if (name === 'props') return scope.props;
      if (scope.locals && Object.prototype.hasOwnProperty.call(scope.locals, name)) {
        return scope.locals[name];
      }
      if (Object.prototype.hasOwnProperty.call(scope.computedLets, name)) {
        return scope.computedLets[name]();
      }
      if (Object.prototype.hasOwnProperty.call(scope.state, name)) {
        return scope.state[name]();
      }
      if (Object.prototype.hasOwnProperty.call(scope.props, name)) {
        return scope.props[name]();
      }
      if (Object.prototype.hasOwnProperty.call(formaStdlib, name)) {
        return formaStdlib[name];
      }
      return name;
    }

    case 'member': {
      if (FORBIDDEN_MEMBER_KEYS.has(node.property)) {
        return undefined;
      }
      if (node.object.type === 'ident') {
        const rootName = node.object.name;
        if (rootName === 'state') {
          const sig = Object.prototype.hasOwnProperty.call(scope.state, node.property)
            ? scope.state[node.property]
            : undefined;
          return typeof sig === 'function' ? sig() : sig;
        }
        if (rootName === 'props') {
          const sig = Object.prototype.hasOwnProperty.call(scope.props, node.property)
            ? scope.props[node.property]
            : undefined;
          return typeof sig === 'function' ? sig() : sig;
        }
        if (rootName === 'i18n') {
          const dict = scope.i18n();
          return Object.prototype.hasOwnProperty.call(dict, node.property)
            ? dict[node.property]
            : undefined;
        }
      }
      const objVal = evalAstNode(node.object, scope);
      if (objVal == null) return undefined;
      if (typeof objVal === 'object' || typeof objVal === 'function') {
        if (!Object.prototype.hasOwnProperty.call(objVal, node.property) && !(node.property in Array.prototype && Array.isArray(objVal)) && !(node.property in String.prototype && typeof objVal === 'string')) {
          return undefined;
        }
      }
      return objVal[node.property];
    }

    case 'index': {
      const objVal = evalAstNode(node.object, scope);
      const idxVal = evalAstNode(node.index, scope);
      if (objVal == null || FORBIDDEN_MEMBER_KEYS.has(String(idxVal))) return undefined;
      if (
        typeof objVal === 'object' &&
        !Array.isArray(objVal) &&
        !Object.prototype.hasOwnProperty.call(objVal, idxVal)
      ) {
        return undefined;
      }
      return objVal[idxVal];
    }

    case 'call': {
      const argVals = node.args.map((a) => evalAstNode(a, scope));
      if (
        node.callee.type === 'ident' &&
        Object.prototype.hasOwnProperty.call(formaStdlib, node.callee.name)
      ) {
        return formaStdlib[node.callee.name](...argVals);
      }
      if (node.callee.type === 'member') {
        if (FORBIDDEN_MEMBER_KEYS.has(node.callee.property)) {
          return undefined;
        }
        const targetObj = evalAstNode(node.callee.object, scope);
        const method = targetObj?.[node.callee.property];
        if (
          typeof method === 'function' &&
          method !== Function &&
          method !== eval &&
          method !== Object.prototype.constructor
        ) {
          return method.apply(targetObj, argVals);
        }
        return undefined;
      }
      const fnVal = evalAstNode(node.callee, scope);
      if (
        typeof fnVal === 'function' &&
        fnVal !== Function &&
        fnVal !== eval &&
        fnVal !== Object.prototype.constructor
      ) {
        return fnVal(...argVals);
      }
      return undefined;
    }

    case 'unary': {
      const v = evalAstNode(node.arg, scope);
      if (node.op === '!') return !v;
      if (node.op === '-') return -Number(v);
      if (node.op === '+') return +Number(v);
      return v;
    }

    case 'binary': {
      if (node.op === '&&') {
        const l = evalAstNode(node.left, scope);
        return l ? evalAstNode(node.right, scope) : l;
      }
      if (node.op === '||') {
        const l = evalAstNode(node.left, scope);
        return l ? l : evalAstNode(node.right, scope);
      }
      if (node.op === '??') {
        const l = evalAstNode(node.left, scope);
        return l !== null && l !== undefined ? l : evalAstNode(node.right, scope);
      }
      const l = evalAstNode(node.left, scope);
      const r = evalAstNode(node.right, scope);
      switch (node.op) {
        case '+':
          return l + r;
        case '-':
          return Number(l) - Number(r);
        case '*':
          return Number(l) * Number(r);
        case '/':
          return Number(l) / Number(r);
        case '%':
          return Number(l) % Number(r);
        case '<':
          return l < r;
        case '>':
          return l > r;
        case '<=':
          return l <= r;
        case '>=':
          return l >= r;
        case '==':
        case '===':
          return l === r;
        case '!=':
        case '!==':
          return l !== r;
        default:
          return undefined;
      }
    }

    case 'conditional': {
      const cond = evalAstNode(node.test, scope);
      return cond ? evalAstNode(node.consequent, scope) : evalAstNode(node.alternate, scope);
    }
  }
}

/**
 * Evaluates a Forma expression string against a reactive scope (`state`, `props`, `tokens`, `i18n`, `let`, `item`)
 * using a 100% CSP-Safe cached AST evaluator (Zero `new Function`, Zero `eval`, Zero `with (__env)`!).
 * Any signal read inside the expression is automatically tracked by Svet's WASM `ReactiveGraph`!
 */
export function evaluateFormaExpression(rawExpr: string, scope: FormaEvalScope): any {
  const expr = rawExpr.trim();
  if (!expr) return undefined;
  try {
    const ast = parseFormaExprAst(expr);
    return evalAstNode(ast, scope);
  } catch {
    return undefined;
  }
}

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

function parseMatchArms(
  armsSource: string
): Array<{
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

export function doesFormaPatternMatch(pattern: string, actualValue: any): boolean {
  const pat = pattern.trim();
  if (pat === '_') return true;

  // OR patterns: `'loading' | 'pending'`
  if (pat.includes('|')) {
    return pat
      .split('|')
      .some((sub) => doesFormaPatternMatch(sub.trim(), actualValue));
  }

  // Inclusive range: `1..=5`
  const incRange = pat.match(/^(-?\d+(?:\.\d+)?)\.\.=(-?\d+(?:\.\d+)?)$/);
  if (incRange && typeof actualValue === 'number') {
    return actualValue >= Number(incRange[1]) && actualValue <= Number(incRange[2]);
  }

  // Exclusive range: `0..10`
  const excRange = pat.match(/^(-?\d+(?:\.\d+)?)\.\.(-?\d+(?:\.\d+)?)$/);
  if (excRange && typeof actualValue === 'number') {
    return actualValue >= Number(excRange[1]) && actualValue < Number(excRange[2]);
  }

  // String literal `'error'` or `"error"`
  if ((pat.startsWith("'") && pat.endsWith("'")) || (pat.startsWith('"') && pat.endsWith('"'))) {
    return String(actualValue) === pat.slice(1, -1);
  }

  // Enum variant `Tone.Danger` -> matches `'Danger'` or `'Tone.Danger'`
  if (pat.includes('.')) {
    const shortVariant = pat.split('.').pop()!;
    return String(actualValue) === pat || String(actualValue) === shortVariant;
  }

  // Numeric or boolean literal
  if (/^-?\d+(\.\d+)?$/.test(pat)) {
    return Number(actualValue) === Number(pat);
  }
  if (pat === 'true') return actualValue === true;
  if (pat === 'false') return actualValue === false;

  return String(actualValue) === pat;
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
        .map((s) => s.trim())
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

function sourceChar(s: string, idx: number): string {
  return s[idx] ?? '';
}

const _formaComponentRegistry = new Map<string, string>();

/**
 * Registers a reusable Forma `.ui` component globally so any other `.ui` component
 * can embed `<ComponentName> { propA: ...; }` declaratively.
 */
export function registerFormaComponent(name: string, uiSource: string): void {
  _formaComponentRegistry.set(name, uiSource);
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

  const tokens: Record<string, any> = {};
  const i18nDictionaries: Record<string, Record<string, string>> = {};
  const styles: Record<string, Record<string, string>> = {};
  const propConstraints: FormaPropConstraint[] = [];
  const initialStateValues: Record<string, any> = {};
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
      const nl = src.indexOf('\n', i + 2);
      const lineEnd = nl === -1 ? n : nl;
      const lineStr = src.slice(i, lineEnd).trim();
      if (lineStr.endsWith(']')) {
        const attrContent = lineStr.slice(2, -1).trim();
        const rangeMatch = attrContent.match(
          /^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/
        );
        if (rangeMatch) {
          pendingRange = [Number(rangeMatch[1]), Number(rangeMatch[2])];
        }
        const patternMatch = attrContent.match(/^pattern\(\s*['"](.+?)['"]\s*\)$/);
        if (patternMatch && isSafePatternRegex(patternMatch[1])) {
          try {
            new RegExp(patternMatch[1]);
            pendingPatternSource = patternMatch[1];
          } catch {
            pendingPatternSource = undefined;
          }
        }
        i = lineEnd;
        continue;
      }
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
          const v = /^-?\d+(\.\d+)?$/.test(vRaw)
            ? Number(vRaw)
            : vRaw.replace(/^['"]|['"]$/g, '');
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
          const v = line.slice(cIdx + 1).trim().replace(/^['"]|['"]$/g, '');
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
    const propMatch = stmt.match(/^(required\s+)?prop\s+([a-zA-Z0-9_]+)\s*:\s*([^=]+?)(?:\s*=\s*(.+))?$/);
    if (propMatch) {
      const required = Boolean(propMatch[1]);
      const name = propMatch[2].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(name)) continue;
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
      if (FORBIDDEN_MEMBER_KEYS.has(name)) continue;
      const initVal = parseLiteralValue(stateMatch[3].trim());
      initialStateValues[name] = structuredCloneIfObject(initVal);
      continue;
    }

    // `let <name> = <expr>`
    const letMatch = stmt.match(/^let\s+([a-zA-Z0-9_]+)\s*=\s*(.+)$/);
    if (letMatch) {
      const letName = letMatch[1].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(letName)) continue;
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

  return {
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
}

/**
 * Instantiates a live, reactive **Svet + WASM** `FormaRuntimeInstance` directly from a pre-parsed
 * `FormaDocumentAst` (0ms runtime parsing overhead) or a raw `.ui` source string.
 */
export function compileFormaAst(
  astOrSource: FormaDocumentAst | string,
  options?: {
    props?: Record<string, any>;
    locale?: string;
    viewModel?: Record<string, FormaProperty<any> | Signal<any>>;
    slots?: FormaSlotMap;
    components?: Record<string, string | ((opts?: any) => FormaRuntimeInstance)>;
  }
): FormaRuntimeInstance {
  return compileFormaComponent(astOrSource, options);
}

/**
 * Compiles a Forma `.ui` source file or pre-parsed `FormaDocumentAst` into a live, reactive **Svet + WASM** `FormaRuntimeInstance`.
 */
export function compileFormaComponent(
  uiSourceOrAst: string | FormaDocumentAst,
  options?: {
    props?: Record<string, any>;
    locale?: string;
    viewModel?: Record<string, FormaProperty<any> | Signal<any>>;
    slots?: FormaSlotMap;
    components?: Record<string, string | ((opts?: any) => FormaRuntimeInstance)>;
  }
): FormaRuntimeInstance {
  const initialAst =
    typeof uiSourceOrAst === 'string' ? parseFormaSource(uiSourceOrAst) : uiSourceOrAst;
  const componentName = initialAst.componentName;

  const tokens: Record<string, any> = {};
  const i18nDictionaries: Record<string, Record<string, string>> = {};
  const styles: Record<string, Record<string, string>> = {};
  const propConstraints: Array<FormaPropConstraint & { compiledPattern?: RegExp }> = [];
  const initialStateValues: Record<string, any> = {};
  const stateSignals: Record<string, Signal<any>> = {};
  const propSignals: Record<string, Signal<any>> = {};
  const letExpressions: Array<{ name: string; expr: string }> = [];
  const shortcuts: Array<{ combo: string; action: string }> = [];
  const rootAstNodes: FormaAstNode[] = [];
  const hmrRevision = signal(0);

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

  const applyDocumentAst = (docAst: FormaDocumentAst, isHotReload = false) => {
    for (const k of Object.keys(tokens)) delete tokens[k];
    Object.assign(tokens, structuredCloneIfObject(docAst.tokens));

    for (const k of Object.keys(i18nDictionaries)) delete i18nDictionaries[k];
    Object.assign(i18nDictionaries, structuredCloneIfObject(docAst.i18nDictionaries));

    for (const k of Object.keys(styles)) delete styles[k];
    Object.assign(styles, structuredCloneIfObject(docAst.styles));

    propConstraints.length = 0;
    for (const c of docAst.propConstraints) {
      const defVal =
        options?.props && Object.prototype.hasOwnProperty.call(options.props, c.name)
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
        propSignals[c.name] = signal(defVal);
      }
    }

    for (const [name, initVal] of Object.entries(docAst.initialStateValues)) {
      if (!isHotReload || !(name in initialStateValues)) {
        initialStateValues[name] = structuredCloneIfObject(initVal);
      }
      if (!stateSignals[name]) {
        stateSignals[name] = signal(structuredCloneIfObject(initVal));
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

  const locale = signal(options?.locale ?? Object.keys(i18nDictionaries)[0] ?? 'ru');
  const activeI18n = computed(() => {
    hmrRevision();
    return i18nDictionaries[locale()] ?? {};
  });

  // Reactive `isValid` computed signal checking all `#[range]` and `#[pattern]` constraints!
  const isValid = computed(() => {
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

  const computedLets: Record<string, ReadonlySignal<any>> = {};
  const evalScope = {
    state: stateSignals,
    props: propSignals,
    computedLets,
    tokens,
    i18n: activeI18n,
    isValid,
  };

  const rebuildComputedLets = () => {
    for (const oldComp of Object.values(computedLets)) {
      oldComp?.dispose?.();
    }
    for (const k of Object.keys(computedLets)) delete computedLets[k];
    for (const l of letExpressions) {
      computedLets[l.name] = computed(() => {
        hmrRevision();
        return evaluateFormaExpression(l.expr, evalScope);
      });
    }
  };
  rebuildComputedLets();

  // Built-in Forma `state.*` / `actions.*` mutators
  const actions: FormaStateMutator = {
    toggle(field: string) {
      const sig = stateSignals[field];
      if (sig) sig.update((v) => !v);
    },
    set(field: string, value: any) {
      const sig = stateSignals[field];
      if (sig) sig.set(value);
    },
    increment(field: string, delta = 1) {
      const sig = stateSignals[field];
      if (sig) sig.update((v) => Number(v || 0) + delta);
    },
    decrement(field: string, delta = 1) {
      const sig = stateSignals[field];
      if (sig) sig.update((v) => Number(v || 0) - delta);
    },
    push(field: string, item: any) {
      const sig = stateSignals[field];
      if (sig) sig.update((arr) => (Array.isArray(arr) ? [...arr, item] : [item]));
    },
    remove(field: string, index: number) {
      const sig = stateSignals[field];
      if (sig) {
        sig.update((arr) =>
          Array.isArray(arr) ? arr.filter((_, idx) => idx !== index) : []
        );
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
  const keyedElements = new Map<string, HTMLElement>();

  const emitEvent = (eventName: string, ...args: any[]) => {
    const set = eventListeners.get(eventName);
    if (set) {
      for (const fn of set) {
        fn(...args);
      }
    }
  };

  const executeFormaAction = (actionStr: string, locals?: Record<string, any>) => {
    const trimmed = actionStr.trim();
    const callScope = { ...evalScope, locals };

    // Parse action expression via `parseFormaExprAst` so commas inside strings `'Иванов, И.И.'`
    // or arrays `['a', 'b']` or nested calls `clamp(1, 2, 3)` are handled with 100% accuracy!
    const ast = parseFormaExprAst(trimmed);
    if (ast.type === 'call' && ast.callee.type === 'member' && ast.callee.object.type === 'ident') {
      const targetNamespace = ast.callee.object.name;
      const methodName = ast.callee.property;
      const parsedArgs = ast.args.map((argNode) => evalAstNode(argNode, callScope));

      if (targetNamespace === 'state' || targetNamespace === 'actions') {
        const method = methodName as keyof FormaStateMutator;
        if (typeof actions[method] === 'function') {
          (actions[method] as (...a: any[]) => void)(...parsedArgs);
        }
        return;
      }

      if (targetNamespace === 'events') {
        emitEvent(methodName, ...parsedArgs);
        return;
      }
    }
  };

  const renderAstNode = (ast: FormaAstNode, locals?: Record<string, any>): Node => {
    const localScope = { ...evalScope, locals };

    if (ast.kind === 'slot') {
      const slotName = ast.slotName!;
      const customSlot = options?.slots?.namedSlots?.[slotName];
      if (customSlot) {
        const frag = document.createDocumentFragment();
        const node = typeof customSlot === 'function' ? customSlot() : customSlot;
        insertChild(frag, node);
        return frag;
      }
      const frag = document.createDocumentFragment();
      for (const ch of ast.children) {
        insertChild(frag, renderAstNode(ch, locals));
      }
      return frag;
    }

    if (ast.kind === 'contentPresenter') {
      const defaultContent = options?.slots?.defaultSlot;
      if (defaultContent) {
        const frag = document.createDocumentFragment();
        const node = typeof defaultContent === 'function' ? defaultContent() : defaultContent;
        insertChild(frag, node);
        return frag;
      }
      const frag = document.createDocumentFragment();
      for (const ch of ast.children) {
        insertChild(frag, renderAstNode(ch, locals));
      }
      return frag;
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

      effect(() => {
        const val = evaluateFormaExpression(ast.matchExpr!, localScope);
        clearArm();
        const matchedArm = (ast.matchArms ?? []).find((arm) =>
          doesFormaPatternMatch(arm.pattern, val)
        );
        if (matchedArm && matchedArm.childNodes.length > 0) {
          createRoot((dispose) => {
            disposeArm = dispose;
            const temp = document.createDocumentFragment();
            for (const ch of matchedArm.childNodes) {
              insertChild(temp, untrack(() => renderAstNode(ch, locals)));
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
      return Show({
        when: () => Boolean(evaluateFormaExpression(ast.conditionExpr!, localScope)),
        children: () => {
          const frag = document.createDocumentFragment();
          for (const ch of ast.children) {
            insertChild(frag, renderAstNode(ch, locals));
          }
          return frag;
        },
        fallback:
          ast.elseChildren && ast.elseChildren.length > 0
            ? () => {
                const frag = document.createDocumentFragment();
                for (const ch of ast.elseChildren!) {
                  insertChild(frag, renderAstNode(ch, locals));
                }
                return frag;
              }
            : undefined,
      });
    }

    if (ast.kind === 'for') {
      const itemVar = ast.itemVar!;
      const listAccessor = () => {
        const res = evaluateFormaExpression(ast.listExpr!, localScope);
        return Array.isArray(res) ? res : [];
      };

      return Show({
        when: () => listAccessor().length > 0,
        fallback:
          ast.emptyChildren && ast.emptyChildren.length > 0
            ? () => {
                const frag = document.createDocumentFragment();
                for (const ch of ast.emptyChildren!) {
                  insertChild(frag, renderAstNode(ch, locals));
                }
                return frag;
              }
            : undefined,
        children: () =>
          For({
            each: listAccessor,
            keyFn: (item, idx) =>
              ast.keyExpr
                ? evaluateFormaExpression(ast.keyExpr, {
                    ...localScope,
                    locals: { ...(locals ?? {}), [itemVar]: item },
                  })
                : idx,
            children: (_item, _index, current) => {
              const frag = document.createDocumentFragment();
              const childLocals = Object.defineProperties({}, Object.getOwnPropertyDescriptors(locals ?? {}));
              Object.defineProperty(childLocals, itemVar, { get: current, enumerable: true, configurable: true });
              for (const ch of ast.children) {
                insertChild(frag, renderAstNode(ch, childLocals));
              }
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

    // Declarative 3D GPU Widget inside Forma `.ui`: `Scene3D { rotX: ...; rotY: ...; rotZ: ...; scale: ...; width: ...; height: ...; }`
    if (tag === 'Scene3D' || tag === 'OrbitCube3D' || tag === 'Viewport3D') {
      const width = () => mergedProps.width
        ? Number(evaluateFormaExpression(mergedProps.width, localScope)) || 320
        : 320;
      const height = () => mergedProps.height
        ? Number(evaluateFormaExpression(mergedProps.height, localScope)) || 180
        : 180;
      const transform = createTransform3D({
        position: [0, 0, -3.2],
        rotation: [0.4, 0.6, 0.1],
        scale: [1, 1, 1],
      });
      const camera = createPerspectiveCamera3D({
        fovYRad: Math.PI / 3,
        aspect: width() / Math.max(1, height()),
      });
      effect(() => camera.aspect.set(width() / Math.max(1, height())));
      const cube = createCubeMesh3D({ size: 1.35, transform, camera });
      onCleanup(() => {
        cube.mesh.dispose();
        camera.dispose();
        transform.dispose();
      });

      effect(() => {
        const rx = mergedProps.rotX
          ? Number(evaluateFormaExpression(mergedProps.rotX, localScope)) || 0
          : 0.4;
        const ry = mergedProps.rotY
          ? Number(evaluateFormaExpression(mergedProps.rotY, localScope)) || 0
          : 0.6;
        const rz = mergedProps.rotZ
          ? Number(evaluateFormaExpression(mergedProps.rotZ, localScope)) || 0
          : 0.1;
        transform.rotation.set([rx, ry, rz]);
      });

      if (mergedProps.scale) {
        effect(() => {
          const s = Number(evaluateFormaExpression(mergedProps.scale, localScope)) || 1;
          transform.scale.set([s, s, s]);
        });
      }

      const canvas = (tag === 'Viewport3D' ? Viewport3D : Canvas3D)({
        renderMode: mergedProps.renderMode ? String(evaluateFormaExpression(mergedProps.renderMode, localScope)) as 'sync' | 'animation-frame' : undefined,
        mesh: cube.mesh,
        edges: cube.edges,
        faces: cube.faces,
        width,
        height,
        orbitTransform: transform,
        strokeStyle: mergedProps.strokeColor
          ? String(evaluateFormaExpression(mergedProps.strokeColor, localScope))
          : '#38bdf8',
      });
      canvas.setAttribute('data-forma-type', tag);
      if (mergedProps.key) {
        const keyVal = String(evaluateFormaExpression(mergedProps.key, localScope));
        canvas.setAttribute('data-forma-key', keyVal);
        keyedElements.set(keyVal, canvas);
      }
      return canvas;
    }

    // Check if `tag` references a registered or passed child Forma `.ui` component!
    const customCompDef =
      options?.components?.[tag] ?? _formaComponentRegistry.get(tag);
    if (customCompDef) {
      const initialChildProps: Record<string, any> = {};
      for (const [pKey, pExpr] of Object.entries(mergedProps)) {
        if (pKey === 'key') continue;
        initialChildProps[pKey] = untrack(() => evaluateFormaExpression(pExpr, localScope));
      }

      const childInst =
        typeof customCompDef === 'function'
          ? customCompDef({
              props: initialChildProps,
              locale: locale.peek(),
              components: options?.components,
            })
          : compileFormaComponent(customCompDef, {
              props: initialChildProps,
              locale: locale.peek(),
              components: options?.components,
            });

      // Keep child `locale` and `props` reactively synchronized with parent signals!
      effect(() => {
        childInst.locale.set(locale());
      });
      for (const [pKey, pExpr] of Object.entries(mergedProps)) {
        if (pKey === 'key') continue;
        if (pKey in childInst.props) {
          effect(() => {
            const nextVal = evaluateFormaExpression(pExpr, localScope);
            childInst.props[pKey].set(nextVal);
          });
        }
      }

      // Bridge child events (`eventName -> actionExpr`) to parent action dispatcher
      for (const [evtName, actionExpr] of Object.entries(ast.eventHandlers)) {
        childInst.onEvent(evtName, (...evtArgs: any[]) => {
          executeFormaAction(actionExpr, { ...(locals ?? {}), $event: evtArgs[0], $args: evtArgs });
        });
      }

      const wrapperEl = document.createElement('div');
      wrapperEl.setAttribute('data-forma-component', tag);
      if (mergedProps.key) {
        const keyVal = String(evaluateFormaExpression(mergedProps.key, localScope));
        wrapperEl.setAttribute('data-forma-key', keyVal);
        keyedElements.set(keyVal, wrapperEl);
      }
      wrapperEl.appendChild(childInst.render());
      onCleanup(() => childInst.dispose());
      return wrapperEl;
    }

    let domTag = 'div';
    if (tag === 'Button') domTag = 'button';
    else if (tag === 'TextInput' || tag === 'TextField') domTag = 'input';
    else if (tag === 'Text' || tag === 'Badge' || tag === 'Tag' || tag === 'Chip') domTag = 'span';
    else if (tag === 'Divider' || tag === 'Separator') domTag = 'hr';

    const el = document.createElement(domTag);
    el.setAttribute('data-forma-type', tag);

    // Apply `transition { duration: 200ms; easing: 'spring'; }` if present
    if (ast.transition) {
      const durVal = ast.transition.duration
        ? evaluateFormaExpression(ast.transition.duration, localScope)
        : 200;
      const durStr = typeof durVal === 'number' ? `${durVal}ms` : String(durVal);
      const rawEasing = ast.transition.easing
        ? String(evaluateFormaExpression(ast.transition.easing, localScope))
        : 'ease-out';
      const easingCss =
        rawEasing === 'spring' ? 'cubic-bezier(0.34, 1.56, 0.64, 1)' : rawEasing;
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

    // Apply reactive properties
    for (const [propName, rawExpr] of Object.entries(mergedProps)) {
      if (propName === 'key') continue;

      if (propName === 'text') {
        effect(() => {
          const val = evaluateFormaExpression(rawExpr, localScope);
          el.textContent = val == null ? '' : String(val);
        });
      } else if (propName === 'value' && domTag === 'input') {
        effect(() => {
          const val = evaluateFormaExpression(rawExpr, localScope);
          (el as HTMLInputElement).value = val == null ? '' : String(val);
        });
      } else if (propName === 'disabled') {
        effect(() => {
          (el as HTMLButtonElement).disabled = Boolean(
            evaluateFormaExpression(rawExpr, localScope)
          );
        });
      } else if (propName === 'placeholder' && domTag === 'input') {
        effect(() => {
          (el as HTMLInputElement).placeholder = String(
            evaluateFormaExpression(rawExpr, localScope) ?? ''
          );
        });
      } else if (propName === 'gap') {
        effect(() => {
          const v = evaluateFormaExpression(rawExpr, localScope);
          el.style.gap = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else if (propName === 'padding') {
        effect(() => {
          const v = evaluateFormaExpression(rawExpr, localScope);
          el.style.padding = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else if (propName === 'background') {
        effect(() => {
          el.style.background = String(evaluateFormaExpression(rawExpr, localScope) ?? '');
        });
      } else if (propName === 'color') {
        effect(() => {
          el.style.color = String(evaluateFormaExpression(rawExpr, localScope) ?? '');
        });
      } else if (propName === 'cornerRadius') {
        effect(() => {
          const v = evaluateFormaExpression(rawExpr, localScope);
          el.style.borderRadius = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else if (propName === 'width' || propName === 'height') {
        effect(() => {
          const v = evaluateFormaExpression(rawExpr, localScope);
          (el.style as any)[propName] = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else {
        effect(() => {
          const v = evaluateFormaExpression(rawExpr, localScope);
          if (v !== undefined && v !== null) {
            el.setAttribute(`data-${propName.toLowerCase()}`, String(v));
          }
        });
      }
    }

    // Two-way bindings: `value <-> state.name`
    for (const [propName, targetPath] of Object.entries(ast.twoWayBindings)) {
      const cleanName = targetPath.replace(/^state\./, '').trim();
      const targetSig = stateSignals[cleanName];
      if (targetSig && propName === 'value' && domTag === 'input') {
        const inputEl = el as HTMLInputElement;
        effect(() => {
          const nextStr = String(targetSig() ?? '');
          if (inputEl.value !== nextStr) {
            inputEl.value = nextStr;
          }
        });
        inputEl.addEventListener('input', () => {
          targetSig.set(inputEl.value);
          if (ast.eventHandlers.changed) {
            executeFormaAction(ast.eventHandlers.changed, locals);
          }
        });
      }
    }

    // Event handlers: `clicked -> ...`
    for (const [evtName, actionExpr] of Object.entries(ast.eventHandlers)) {
      if (evtName === 'clicked') {
        el.addEventListener('click', () => {
          if (!(el as HTMLButtonElement).disabled) {
            executeFormaAction(actionExpr, locals);
          }
        });
      }
    }

    // Render children (and evaluate any property-group `match` blocks applied to this element!)
    for (const childAst of ast.children) {
      if (
        childAst.kind === 'match' &&
        childAst.matchArms &&
        childAst.matchArms.every((a) => a.childNodes.length === 0)
      ) {
        effect(() => {
          const val = evaluateFormaExpression(childAst.matchExpr!, localScope);
          const arm = childAst.matchArms!.find((a) => doesFormaPatternMatch(a.pattern, val));
          if (arm) {
            for (const [pKey, pExpr] of Object.entries(arm.propertyAssignments)) {
              const resolved = evaluateFormaExpression(pExpr, localScope);
              if (pKey === 'background') el.style.background = String(resolved ?? '');
              else if (pKey === 'color') el.style.color = String(resolved ?? '');
              else if (pKey === 'borderColor') el.style.borderColor = String(resolved ?? '');
              else if (pKey === 'text') el.textContent = String(resolved ?? '');
              else el.setAttribute(`data-${pKey.toLowerCase()}`, String(resolved ?? ''));
            }
          }
        });
      } else {
        insertChild(el, renderAstNode(childAst, locals));
      }
    }

    return el;
  };

  return {
    componentName,
    state: stateSignals,
    props: propSignals,
    computedLets,
    locale,
    isValid,
    actions,
    onEvent(eventName: string, handler: (...args: any[]) => void) {
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
      batch(() => {
        const nextAst =
          typeof nextUiSourceOrAst === 'string'
            ? parseFormaSource(nextUiSourceOrAst)
            : nextUiSourceOrAst;
        applyDocumentAst(nextAst, true);
        rebuildComputedLets();
        hmrRevision.update((r) => r + 1);
      });
    },
    render() {
      const frag = document.createDocumentFragment();
      const anchor = document.createComment(`svet-forma-hmr:${componentName}`);
      frag.appendChild(anchor);

      let currentNodes: Node[] = [];
      let disposeView: (() => void) | null = null;

      const clearCurrentView = () => {
        if (disposeView) {
          disposeView();
          disposeView = null;
        }
        for (const n of currentNodes) {
          if (n.parentNode) n.parentNode.removeChild(n);
        }
        currentNodes = [];
      };

      const mountCurrentView = () => {
        clearCurrentView();
        keyedElements.clear();
        createRoot((dispose) => {
          disposeView = dispose;
          const temp = document.createDocumentFragment();
          for (const nodeAst of rootAstNodes) {
            insertChild(temp, untrack(() => renderAstNode(nodeAst)));
          }
          if (shortcuts.length > 0 && typeof window !== 'undefined') {
            const onKeyDown = (e: KeyboardEvent) => {
              for (const sc of shortcuts) {
                const parts = sc.combo.split('+');
                const keyPart = parts[parts.length - 1];
                const needsMod = parts.includes('Mod') || parts.includes('Ctrl') || parts.includes('Meta');
                const modPressed = e.metaKey || e.ctrlKey;
                if (needsMod && !modPressed) continue;
                if (e.key.toLowerCase() === keyPart.toLowerCase()) {
                  e.preventDefault();
                  executeFormaAction(sc.action);
                }
              }
            };
            window.addEventListener('keydown', onKeyDown);
            onCleanup(() => window.removeEventListener('keydown', onKeyDown));
          }
          const list = Array.from(temp.childNodes);
          const parent = anchor.parentNode ?? frag;
          for (const n of list) {
            parent.insertBefore(n, anchor);
          }
          currentNodes = list;
        });
      };

      mountCurrentView();

      let firstRun = true;
      effect(() => {
        hmrRevision();
        if (firstRun) {
          firstRun = false;
          return;
        }
        mountCurrentView();
      });

      onCleanup(() => clearCurrentView());
      return frag;
    },
    mount(container: HTMLElement) {
      container.innerHTML = '';
      let disposeRoot!: () => void;
      const rootFrag = createRoot((dispose) => {
        disposeRoot = dispose;
        return this.render();
      });

      container.appendChild(rootFrag);
      return {
        unmount() {
          disposeRoot();
          container.innerHTML = '';
        },
      };
    },
    dispose() {
      const externalVmKeys = new Set(Object.keys(options?.viewModel ?? {}));
      for (const [k, sig] of Object.entries(stateSignals)) {
        if (!externalVmKeys.has(k)) {
          sig.dispose();
        }
      }
      for (const sig of Object.values(propSignals)) {
        sig.dispose();
      }
      for (const comp of Object.values(computedLets)) {
        comp.dispose();
      }
      activeI18n.dispose();
      isValid.dispose();
      locale.dispose();
      hmrRevision.dispose();
      eventListeners.clear();
      keyedElements.clear();
    },
  };
}

function parseLiteralValue(raw: string): any {
  const s = raw.trim();
  if (s === 'true') return true;
  if (s === 'false') return false;
  if (s === 'null') return null;
  if (/^-?\d+(\.\d+)?$/.test(s)) return Number(s);
  if ((s.startsWith("'") && s.endsWith("'")) || (s.startsWith('"') && s.endsWith('"'))) {
    return s.slice(1, -1);
  }
  if (s.startsWith('[') || s.startsWith('{')) {
    try {
      const normalizedJson = s.replace(/'/g, '"').replace(/([a-zA-Z0-9_]+)\s*:/g, '"$1":');
      return JSON.parse(normalizedJson, (key, val) =>
        FORBIDDEN_MEMBER_KEYS.has(key) ? undefined : val
      );
    } catch {
      return [];
    }
  }
  return s;
}

function structuredCloneIfObject<T>(val: T): T {
  if (val !== null && typeof val === 'object') {
    return JSON.parse(JSON.stringify(val));
  }
  return val;
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
      message: 'Отсутствует явный заголовок `component <Name> { ... }` (используется анонимный режим)',
      line: 1,
    });
  }

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
  const lines = uiSource.split('\n');
  lines.forEach((ln, idx) => {
    const s = ln.trim();
    if (s.startsWith('#[')) {
      if (!s.endsWith(']')) {
        diagnostics.push({
          severity: 'error',
          message: `Незакрытый атрибут контракта '${s}' на строке ${idx + 1}`,
          line: idx + 1,
        });
      } else {
        const inner = s.slice(2, -1).trim();
        const rMatch = inner.match(/^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/);
        if (rMatch && Number(rMatch[1]) > Number(rMatch[2])) {
          diagnostics.push({
            severity: 'error',
            message: `Некорректный контракт #[range]: минимум (${rMatch[1]}) больше максимума (${rMatch[2]})`,
            line: idx + 1,
          });
        }
        const pMatch = inner.match(/^pattern\(\s*['"](.+?)['"]\s*\)$/);
        if (pMatch) {
          if (!isSafePatternRegex(pMatch[1])) {
            diagnostics.push({
              severity: 'error',
              message: `Небезопасное регулярное выражение в #[pattern] (возможен ReDoS): ${pMatch[1]}`,
              line: idx + 1,
            });
          } else {
            try {
              new RegExp(pMatch[1]);
            } catch (e: any) {
              diagnostics.push({
                severity: 'error',
                message: `Ошибка регулярного выражения в #[pattern]: ${e?.message ?? pMatch[1]}`,
                line: idx + 1,
              });
            }
          }
        }
      }
    }
  });

  const propCount = (uiSource.match(/(?:^|\s)(?:required\s+)?prop\s+[a-zA-Z0-9_]+\s*:/gm) ?? []).length;
  const stateCount = (uiSource.match(/(?:^|\s)state\s+[a-zA-Z0-9_]+\s*:/gm) ?? []).length;
  const letCount = (uiSource.match(/(?:^|\s)let\s+[a-zA-Z0-9_]+\s*=/gm) ?? []).length;
  const has3DViewport = /\b(Scene3D|OrbitCube3D|Viewport3D)\s*\{/.test(uiSource);

  const hasError = diagnostics.some((d) => d.severity === 'error');
  return {
    valid: !hasError,
    componentName,
    diagnostics,
    propCount,
    stateCount,
    letCount,
    has3DViewport,
  };
}

/**
 * Generates a True Pre-Parsed AST AOT JavaScript/ES Module from a Forma `.ui` source file.
 * Both `uiAst` (`parseFormaSource`) and `uiValidation` (`validateFormaSource`) are computed
 * at build time so the browser runs `compileFormaAst(uiAst, options)` with 0ms of source parsing!
 */
export function compileFormaToJsModule(uiSource: string, importSource = 'svet-engine'): string {
  const report = validateFormaSource(uiSource);
  const precompiledAst = parseFormaSource(uiSource);
  return `import { compileFormaAst } from ${JSON.stringify(importSource)};
export const uiSource = ${JSON.stringify(uiSource)};
export const uiAst = ${JSON.stringify(precompiledAst)};
export const uiValidation = ${JSON.stringify(report)};
const _activeInstances = import.meta.hot ? (import.meta.hot.data.instances ??= new Set()) : new Set();
export default function createFormaView(options) {
  const inst = compileFormaAst(uiAst, options);
  _activeInstances.add(inst);
  const origMount = inst.mount.bind(inst);
  inst.mount = (container) => {
    _activeInstances.add(inst);
    const handle = origMount(container);
    return {
      unmount() {
        _activeInstances.delete(inst);
        handle.unmount();
      },
    };
  };
  return inst;
}
if (import.meta.hot) {
  import.meta.hot.accept((newMod) => {
    if (newMod && _activeInstances.size > 0) {
      for (const inst of _activeInstances) {
        inst.hotReload(newMod.uiAst ?? newMod.uiSource);
      }
    }
  });
}
`;
}

// ============================================================================
// 4. Reactive Springs, Tweens, Persisted Signals & Contract Forms
// ============================================================================

export interface SpringSignal extends ReadonlySignal<number> {
  readonly target: ReadonlySignal<number>;
  /** Sets the target value and steps the spring physics toward it */
  set(nextTarget: number, options?: { hard?: boolean; steps?: number; animate?: boolean }): void;
  /** Instantly snaps both target and current value to `nextTarget` with zero velocity */
  snap(nextTarget: number): void;
  /** Advances the spring simulation by `dtSeconds` (default `1/60`) */
  step(dtSeconds?: number): number;
  dispose(): void;
}

const activeAnimatedSprings = new Set<(dt: number) => boolean>();
let sharedSpringRafId: number | null = null;

function scheduleSharedSpringTick(): void {
  if (sharedSpringRafId !== null) return;
  if (typeof requestAnimationFrame !== 'function') return;
  sharedSpringRafId = requestAnimationFrame(() => {
    sharedSpringRafId = null;
    if (activeAnimatedSprings.size === 0) return;
    // Update all active springs in a SINGLE atomic WASM batch per frame!
    batch(() => {
      for (const tickSpring of Array.from(activeAnimatedSprings)) {
        const settled = tickSpring(1 / 60);
        if (settled) {
          activeAnimatedSprings.delete(tickSpring);
        }
      }
    });
    if (activeAnimatedSprings.size > 0) {
      scheduleSharedSpringTick();
    }
  });
}

/**
 * Steps all currently animating springs by `steps` frames inside a single atomic `batch()`.
 * Useful for deterministic testing or manual frame-rate synchronization.
 */
export function flushSpringFrame(steps = 1, dt = 1 / 60): void {
  if (activeAnimatedSprings.size === 0) return;
  for (let s = 0; s < steps; s++) {
    batch(() => {
      for (const tickSpring of Array.from(activeAnimatedSprings)) {
        const settled = tickSpring(dt);
        if (settled) {
          activeAnimatedSprings.delete(tickSpring);
        }
      }
    });
    if (activeAnimatedSprings.size === 0) break;
  }
}

/**
 * Creates a physics-backed harmonic oscillator Spring signal (`createSpring`)
 * with configurable `stiffness` and `damping` integrated into the WASM reactive graph
 * and driven by a shared, batched frame scheduler.
 */
export function createSpring(
  initialValue: number,
  options?: { stiffness?: number; damping?: number; precision?: number }
): SpringSignal {
  const stiffness = options?.stiffness ?? 170;
  const damping = options?.damping ?? 26;
  const precision = options?.precision ?? 0.001;

  const currentSig = signal(initialValue);
  const targetSig = signal(initialValue);
  let velocity = 0;

  const stepAndCheckSettled = (dt = 1 / 60): { value: number; settled: boolean } => {
    const cur = currentSig.peek();
    const tgt = targetSig.peek();
    const displacement = cur - tgt;
    const springForce = -stiffness * displacement;
    const dampingForce = -damping * velocity;
    const acceleration = springForce + dampingForce;

    velocity += acceleration * dt;
    const nextVal = cur + velocity * dt;

    if (Math.abs(velocity) < precision && Math.abs(tgt - nextVal) < precision) {
      velocity = 0;
      currentSig.set(tgt);
      return { value: tgt, settled: true };
    }
    currentSig.set(nextVal);
    return { value: nextVal, settled: false };
  };

  const tickCallback = (dt: number): boolean => stepAndCheckSettled(dt).settled;

  const step = (dt = 1 / 60): number => stepAndCheckSettled(dt).value;

  const getter = (() => currentSig()) as unknown as SpringSignal;
  (getter as any).id = currentSig.id;
  getter.peek = () => currentSig.peek();
  Object.defineProperty(getter, 'target', { value: targetSig, enumerable: true });

  getter.snap = (nextTarget: number) => {
    activeAnimatedSprings.delete(tickCallback);
    velocity = 0;
    batch(() => {
      targetSig.set(nextTarget);
      currentSig.set(nextTarget);
    });
  };

  getter.set = (
    nextTarget: number,
    setOpts?: { hard?: boolean; steps?: number; animate?: boolean }
  ) => {
    if (setOpts?.hard) {
      getter.snap(nextTarget);
      return;
    }
    targetSig.set(nextTarget);
    const simSteps = setOpts?.steps ?? 1;
    let isSettled = false;
    batch(() => {
      for (let i = 0; i < simSteps; i++) {
        const res = stepAndCheckSettled(1 / 60);
        if (res.settled) {
          isSettled = true;
          break;
        }
      }
    });
    if (!isSettled && setOpts?.animate !== false) {
      activeAnimatedSprings.add(tickCallback);
      scheduleSharedSpringTick();
    } else if (isSettled) {
      activeAnimatedSprings.delete(tickCallback);
    }
  };

  getter.step = step;
  getter.dispose = () => {
    activeAnimatedSprings.delete(tickCallback);
    currentSig.dispose();
    targetSig.dispose();
  };
  onCleanup(() => getter.dispose());

  return getter;
}

/**
 * Creates a reactive Signal persisted automatically to `localStorage` (or custom storage)
 * and synchronized across browser tabs via `storage` and `BroadcastChannel` events!
 * Properly closes its `BroadcastChannel` and removes its `storage` listener on `dispose()` or scope cleanup.
 */
export function createPersistedSignal<T>(
  key: string,
  initialValue: T,
  options?: {
    storage?: Pick<Storage, 'getItem' | 'setItem'>;
    channelName?: string;
  }
): Signal<T> {
  const storage =
    options?.storage ?? (typeof localStorage !== 'undefined' ? localStorage : undefined);

  let startVal = initialValue;
  if (storage) {
    try {
      const raw = storage.getItem(key);
      if (raw !== null) {
        startVal = JSON.parse(raw) as T;
      }
    } catch {
      // Fallback to initialValue
    }
  }

  const sig = signal<T>(startVal);
  let suppressBroadcast = false;
  let channel: BroadcastChannel | null = null;

  if (typeof BroadcastChannel !== 'undefined') {
    try {
      channel = new BroadcastChannel(options?.channelName ?? `svet_persisted_${key}`);
      channel.onmessage = (ev: MessageEvent) => {
        if (ev.data && ev.data.key === key) {
          suppressBroadcast = true;
          try {
            sig.set(ev.data.value as T);
          } finally {
            suppressBroadcast = false;
          }
        }
      };
    } catch {
      channel = null;
    }
  }

  const syncEffect = effect(() => {
    const val = sig();
    if (storage) {
      try {
        storage.setItem(key, JSON.stringify(val));
      } catch {
        // Ignore quota errors
      }
    }
    if (channel && !suppressBroadcast) {
      try {
        channel.postMessage({ key, value: val });
      } catch {
        // Ignore non-cloneable or closed channel errors
      }
    }
  });

  let onStorageListener: ((e: StorageEvent) => void) | null = null;
  if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
    onStorageListener = (e: StorageEvent) => {
      if (e.key === key && e.newValue !== null) {
        suppressBroadcast = true;
        try {
          sig.set(JSON.parse(e.newValue) as T);
        } catch {
          // Ignore malformed JSON
        } finally {
          suppressBroadcast = false;
        }
      }
    };
    window.addEventListener('storage', onStorageListener);
  }

  let cleanedUp = false;
  const cleanupPersisted = () => {
    if (cleanedUp) return;
    cleanedUp = true;
    syncEffect.dispose();
    if (onStorageListener && typeof window !== 'undefined' && typeof window.removeEventListener === 'function') {
      window.removeEventListener('storage', onStorageListener);
      onStorageListener = null;
    }
    if (channel) {
      try {
        channel.close();
      } catch {
        // Ignore
      }
      channel = null;
    }
  };

  const origDispose = sig.dispose.bind(sig);
  sig.dispose = () => {
    cleanupPersisted();
    origDispose();
  };
  onCleanup(cleanupPersisted);

  return sig;
}

export interface FormFieldRule<V = any, AllValues = Record<string, any>> {
  required?: boolean;
  min?: number;
  max?: number;
  minLength?: number;
  maxLength?: number;
  range?: [number, number];
  pattern?: RegExp;
  validate?: (value: V, allValues: AllValues) => string | null | undefined;
  validateAsync?: (
    value: V,
    options: { signal: AbortSignal; allValues: AllValues }
  ) => Promise<string | null | undefined>;
}

export interface ReactiveForm<T extends Record<string, any>> {
  readonly values: { [K in keyof T]: Signal<T[K]> };
  readonly errors: { [K in keyof T]: ReadonlySignal<string | null> };
  readonly touched: { [K in keyof T]: Signal<boolean> };
  readonly dirty: { [K in keyof T]: ReadonlySignal<boolean> };
  readonly isDirty: ReadonlySignal<boolean>;
  readonly isValidating: ReadonlySignal<boolean>;
  readonly isValid: ReadonlySignal<boolean>;
  setValue<K extends keyof T>(key: K, value: T[K]): void;
  setValues(partial: Partial<T>): void;
  validateAll(): Promise<boolean>;
  reset(): void;
  submit(onSubmit: (cleanValues: T) => void): boolean;
}

/**
 * Creates a reactive, per-field and cross-field validated Form (`createForm` / `createContractForm`)
 * supporting Forma's `required`, `range: [min, max]`, `min`, `max`, `minLength`, `maxLength`,
 * `pattern: RegExp`, synchronous cross-field `validate(val, allValues)`, and AbortController-backed
 * `validateAsync(val, { signal, allValues })` with atomic `batch()` updates.
 */
export function createForm<T extends Record<string, any>>(config: {
  initialValues: T;
  rules?: { [K in keyof T]?: FormFieldRule<T[K], T> };
}): ReactiveForm<T> {
  const keys = Object.keys(config.initialValues) as Array<keyof T>;
  const values = {} as { [K in keyof T]: Signal<T[K]> };
  const touched = {} as { [K in keyof T]: Signal<boolean> };
  const dirty = {} as { [K in keyof T]: ReadonlySignal<boolean> };
  const syncErrors = {} as { [K in keyof T]: ReadonlySignal<string | null> };
  const asyncErrors = {} as { [K in keyof T]: Signal<string | null> };
  const asyncValidating = {} as { [K in keyof T]: Signal<boolean> };
  const errors = {} as { [K in keyof T]: ReadonlySignal<string | null> };
  const activeControllers = new Map<keyof T, AbortController>();

  for (const key of keys) {
    const initVal = structuredCloneIfObject(config.initialValues[key]);
    values[key] = signal<T[keyof T]>(initVal);
    touched[key] = signal<boolean>(false);
    asyncErrors[key] = signal<string | null>(null);
    asyncValidating[key] = signal<boolean>(false);
    dirty[key] = computed<boolean>(() => !Object.is(values[key](), config.initialValues[key]));
  }

  const getAllValuesReactive = (): T => {
    const snap = {} as T;
    for (const k of keys) {
      snap[k] = values[k]();
    }
    return snap;
  };

  for (const key of keys) {
    const rule = config.rules?.[key];
    syncErrors[key] = computed<string | null>(() => {
      const val = values[key]();
      if (!rule) return null;
      if (rule.required && (val === null || val === undefined || val === '')) {
        return 'Обязательное поле';
      }
      if (typeof val === 'number') {
        const minBound = rule.range ? rule.range[0] : rule.min;
        const maxBound = rule.range ? rule.range[1] : rule.max;
        if (minBound !== undefined && maxBound !== undefined && (val < minBound || val > maxBound)) {
          return `Значение должно быть от ${minBound} до ${maxBound}`;
        }
        if (minBound !== undefined && val < minBound) {
          return `Минимум ${minBound}`;
        }
        if (maxBound !== undefined && val > maxBound) {
          return `Максимум ${maxBound}`;
        }
      }
      if (typeof val === 'string') {
        if (rule.minLength !== undefined && val.length < rule.minLength) {
          return `Минимальная длина: ${rule.minLength}`;
        }
        if (rule.maxLength !== undefined && val.length > rule.maxLength) {
          return `Максимальная длина: ${rule.maxLength}`;
        }
        if (rule.pattern && !rule.pattern.test(val)) {
          return 'Некорректный формат';
        }
      }
      if (rule.validate) {
        // Cross-field validation: only subscribes to other fields if `rule.validate` has >= 2 parameters!
        const allVals = rule.validate.length >= 2 ? getAllValuesReactive() : ({} as T);
        return rule.validate(val, allVals) ?? null;
      }
      return null;
    });

    errors[key] = computed<string | null>(() => syncErrors[key]() ?? asyncErrors[key]());

    if (rule?.validateAsync) {
      effect(() => {
        const val = values[key]();
        const sErr = syncErrors[key]();
        const prevCtrl = activeControllers.get(key);
        if (prevCtrl) {
          prevCtrl.abort();
          activeControllers.delete(key);
        }
        if (sErr !== null) {
          asyncErrors[key].set(null);
          asyncValidating[key].set(false);
          return;
        }
        const ctrl = new AbortController();
        activeControllers.set(key, ctrl);
        asyncValidating[key].set(true);
        const allVals = untrack(() => getAllValuesReactive());
        rule
          .validateAsync!(val, { signal: ctrl.signal, allValues: allVals })
          .then((msg) => {
            if (!ctrl.signal.aborted) {
              batch(() => {
                asyncErrors[key].set(msg ?? null);
                asyncValidating[key].set(false);
              });
            }
          })
          .catch(() => {
            if (!ctrl.signal.aborted) {
              asyncValidating[key].set(false);
            }
          });
      });
    }
  }

  const isDirty = computed<boolean>(() => {
    for (const key of keys) {
      if (dirty[key]()) return true;
    }
    return false;
  });

  const isValidating = computed<boolean>(() => {
    for (const key of keys) {
      if (asyncValidating[key]()) return true;
    }
    return false;
  });

  const isValid = computed<boolean>(() => {
    for (const key of keys) {
      if (errors[key]() !== null || asyncValidating[key]()) return false;
    }
    return true;
  });

  const setValue = <K extends keyof T>(key: K, value: T[K]): void => {
    batch(() => {
      touched[key]?.set(true);
      values[key]?.set(value);
    });
  };

  const setValues = (partial: Partial<T>): void => {
    batch(() => {
      for (const k of Object.keys(partial) as Array<keyof T>) {
        if (k in values) {
          touched[k].set(true);
          values[k].set(partial[k] as T[keyof T]);
        }
      }
    });
  };

  const validateAll = async (): Promise<boolean> => {
    batch(() => {
      for (const key of keys) {
        touched[key].set(true);
      }
    });
    const asyncTasks: Promise<void>[] = [];
    for (const key of keys) {
      const rule = config.rules?.[key];
      if (rule?.validateAsync && syncErrors[key]() === null) {
        const prevCtrl = activeControllers.get(key);
        if (prevCtrl) prevCtrl.abort();
        const ctrl = new AbortController();
        activeControllers.set(key, ctrl);
        asyncValidating[key].set(true);
        const val = values[key].peek();
        const allVals = untrack(() => getAllValuesReactive());
        asyncTasks.push(
          rule
            .validateAsync(val, { signal: ctrl.signal, allValues: allVals })
            .then((msg) => {
              if (!ctrl.signal.aborted) {
                batch(() => {
                  asyncErrors[key].set(msg ?? null);
                  asyncValidating[key].set(false);
                });
              }
            })
            .catch(() => {
              if (!ctrl.signal.aborted) {
                asyncValidating[key].set(false);
              }
            })
        );
      }
    }
    if (asyncTasks.length > 0) {
      await Promise.all(asyncTasks);
    }
    return isValid();
  };

  const reset = () => {
    for (const ctrl of activeControllers.values()) {
      ctrl.abort();
    }
    activeControllers.clear();
    batch(() => {
      for (const key of keys) {
        values[key].set(structuredCloneIfObject(config.initialValues[key]));
        touched[key].set(false);
        asyncErrors[key].set(null);
        asyncValidating[key].set(false);
      }
    });
  };

  const submit = (onSubmit: (cleanValues: T) => void): boolean => {
    batch(() => {
      for (const key of keys) {
        touched[key].set(true);
      }
    });
    if (!isValid()) return false;
    const snapshot = {} as T;
    for (const key of keys) {
      snapshot[key] = structuredCloneIfObject(values[key].peek());
    }
    onSubmit(snapshot);
    return true;
  };

  return {
    values,
    errors,
    touched,
    dirty,
    isDirty,
    isValidating,
    isValid,
    setValue,
    setValues,
    validateAll,
    reset,
    submit,
  };
}

export const createContractForm = createForm;

/**
 * First-class Svet JSX component (`<FormaHost source={...} />` or `<FormaHost instance={...} />`)
 * that embeds a Forma `.ui` interface directly inside a Svet application tree!
 */
export function FormaHost(props: {
  source?: string;
  instance?: FormaRuntimeInstance;
  props?: Record<string, any>;
  locale?: string;
  viewModel?: Record<string, FormaProperty<any> | Signal<any>>;
  slots?: FormaSlotMap;
  onReady?: (instance: FormaRuntimeInstance) => void;
}): DocumentFragment {
  const inst =
    props.instance ??
    compileFormaComponent(props.source ?? '', {
      props: props.props,
      locale: props.locale,
      viewModel: props.viewModel,
      slots: props.slots,
    });
  if (props.onReady) {
    props.onReady(inst);
  }
  return inst.render();
}
