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
  readState?: (key: string) => any;
}

// ============================================================================
// CSP-Safe AST Expression Parser & Evaluator (Zero `new Function` / Zero `with`)
// ============================================================================

type InterpPart = { literal: true; text: string } | { literal: false; ast: ExprAst };

type ExprAst =
  | { type: 'literal'; value: any }
  | { type: 'interp'; template: string; parts: InterpPart[] }
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

function isCharacterRange(code: number, min: number, max: number): boolean {
  return code >= min && code <= max;
}

function isAsciiWhitespace(code: number): boolean {
  return code === 32 || code === 9 || code === 10 || code === 13 || code === 12 || code === 11;
}

function isIdentStart(code: number): boolean {
  return (
    isCharacterRange(code, 65, 90) || // A-Z
    isCharacterRange(code, 97, 122) || // a-z
    code === 95 || // _
    code === 36 || // $
    code === 35 // #
  );
}

function isIdentContinue(code: number): boolean {
  return (
    isIdentStart(code) ||
    isCharacterRange(code, 48, 57) || // 0-9
    code === 45 // -
  );
}

function tokenizeFormaExpr(src: string): ExprToken[] {
  const tokens: ExprToken[] = [];
  let i = 0;
  const n = src.length;

  while (i < n) {
    const code = src.charCodeAt(i);
    if (isAsciiWhitespace(code)) {
      i++;
      continue;
    }

    const ch = src[i];
    if (code === 39 || code === 34) {
      const q = ch;
      let s = '';
      i++;
      while (i < n && src[i] !== q) {
        if (src.charCodeAt(i) === 92 && i + 1 < n) {
          if (src[i + 1] === '$') {
            s += '\\$';
            i += 2;
            continue;
          }
          const nextCh = src[i + 1];
          i += 2;
          if (nextCh === 'n') s += '\n';
          else if (nextCh === 'r') s += '\r';
          else if (nextCh === 't') s += '\t';
          else s += nextCh;
          continue;
        }
        s += src[i++];
      }
      i++;
      tokens.push({ kind: 'str', val: s });
      continue;
    }

    if (isCharacterRange(code, 48, 57)) {
      const numStart = i;
      while (i < n) {
        const c = src.charCodeAt(i);
        if (isCharacterRange(c, 48, 57) || c === 46) i++;
        else break;
      }
      const numStr = src.slice(numStart, i);
      // Consume optional Forma unit (`px`, `ms`, `s`, `deg`, `rad`, `rem`, `em`, `%`, `turn`)
      const unitStart = i;
      while (i < n) {
        const c = src.charCodeAt(i);
        if (isCharacterRange(c, 65, 90) || isCharacterRange(c, 97, 122) || c === 37) i++;
        else break;
      }
      const unit = src.slice(unitStart, i);
      if (unit && unit !== 'px' && unit !== 'ms') {
        tokens.push({ kind: 'str', val: numStr + unit });
      } else {
        tokens.push({ kind: 'num', val: numStr });
      }
      continue;
    }

    // Multi-char operators
    if (i + 2 < n && (src.startsWith('===', i) || src.startsWith('!==', i))) {
      tokens.push({ kind: 'op', val: src.slice(i, i + 3) });
      i += 3;
      continue;
    }
    if (i + 1 < n) {
      const c2 = src.charCodeAt(i + 1);
      if (
        (code === 61 && c2 === 61) || // ==
        (code === 33 && c2 === 61) || // !=
        (code === 60 && c2 === 61) || // <=
        (code === 62 && c2 === 61) || // >=
        (code === 38 && c2 === 38) || // &&
        (code === 124 && c2 === 124) || // ||
        (code === 63 && c2 === 63) // ??
      ) {
        tokens.push({ kind: 'op', val: src.slice(i, i + 2) });
        i += 2;
        continue;
      }
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

    if (isIdentStart(code)) {
      const idStart = i;
      i++;
      while (i < n && isIdentContinue(src.charCodeAt(i))) {
        i++;
      }
      tokens.push({ kind: 'ident', val: src.slice(idStart, i) });
      continue;
    }

    i++;
  }

  return tokens;
}

function compileInterpParts(template: string): InterpPart[] {
  const parts: InterpPart[] = [];
  const re = /\\?\$\{([^}]+)\}/g;
  let lastIdx = 0;
  let match: RegExpExecArray | null;
  while ((match = re.exec(template)) !== null) {
    if (match.index > lastIdx) {
      parts.push({ literal: true, text: template.slice(lastIdx, match.index) });
    }
    if (match[0].startsWith('\\')) {
      parts.push({ literal: true, text: match[0].slice(1) });
    } else {
      parts.push({ literal: false, ast: parseFormaExprAst(match[1].trim()) });
    }
    lastIdx = re.lastIndex;
  }
  if (lastIdx < template.length) {
    parts.push({ literal: true, text: template.slice(lastIdx) });
  }
  return parts;
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
        return { type: 'interp', template: tok.val, parts: compileInterpParts(tok.val) };
      }
      return { type: 'literal', value: tok.val };
    }
    if (tok.kind === 'ident') {
      if (tok.val === 'true') return { type: 'literal', value: true };
      if (tok.val === 'false') return { type: 'literal', value: false };
      if (tok.val === 'null' || tok.val === 'none') return { type: 'literal', value: null };
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

    case 'interp': {
      let out = '';
      for (const part of node.parts) {
        if (part.literal) {
          out += part.text;
        } else {
          const val = evalAstNode(part.ast, scope);
          if (val != null) out += String(val);
        }
      }
      return out;
    }

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
          if (Object.prototype.hasOwnProperty.call(scope.state, node.property)) {
            const sig = scope.state[node.property];
            return typeof sig === 'function' ? sig() : sig;
          }
          if (scope.readState) {
            return scope.readState(node.property);
          }
          return undefined;
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

function isTokensRoot(node: ExprAst): boolean {
  if (node.type === 'ident') {
    return node.name === 'tokens';
  }
  if (node.type === 'member') {
    return isTokensRoot(node.object);
  }
  return false;
}

function isStaticFormaExprNode(node: ExprAst, locals?: Record<string, any>): boolean {
  switch (node.type) {
    case 'literal':
      return true;
    case 'interp':
      return node.parts.every((p) => p.literal || isStaticFormaExprNode(p.ast, locals));
    case 'call':
      return (
        node.callee.type === 'ident' &&
        Object.prototype.hasOwnProperty.call(formaStdlib, node.callee.name) &&
        node.args.every((arg) => isStaticFormaExprNode(arg, locals))
      );
    case 'member':
      if (isTokensRoot(node.object)) {
        return true;
      }
      return isStaticFormaExprNode(node.object, locals);
    case 'index':
      return isStaticFormaExprNode(node.object, locals) && isStaticFormaExprNode(node.index, locals);
    case 'unary':
      return isStaticFormaExprNode(node.arg, locals);
    case 'binary':
      return isStaticFormaExprNode(node.left, locals) && isStaticFormaExprNode(node.right, locals);
    case 'conditional':
      return (
        isStaticFormaExprNode(node.test, locals) &&
        isStaticFormaExprNode(node.consequent, locals) &&
        isStaticFormaExprNode(node.alternate, locals)
      );
    case 'array':
      return node.elements.every((el) => isStaticFormaExprNode(el, locals));
    default:
      return false;
  }
}

function isStaticFormaExpr(rawExpr: string, locals?: Record<string, any>): boolean {
  const expr = rawExpr.trim();
  if (!expr) return true;
  try {
    return isStaticFormaExprNode(parseFormaExprAst(expr), locals);
  } catch {
    return false;
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

    if (src.startsWith('#[', i)) throw new Error('Forma contract annotation cannot follow visual declarations');

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

const _patternMatcherCache = new Map<string, (val: any) => boolean>();

export function compileFormaPatternMatcher(pattern: string): (val: any) => boolean {
  let cached = _patternMatcherCache.get(pattern);
  if (cached) return cached;

  const pat = pattern.trim();
  let matcher: (val: any) => boolean;

  if (pat === '_') {
    matcher = () => true;
  } else if (pat.includes('|')) {
    const subMatchers = pat.split('|').map((sub) => compileFormaPatternMatcher(sub.trim()));
    matcher = (actualValue: any) => {
      for (let i = 0; i < subMatchers.length; i++) {
        if (subMatchers[i](actualValue)) return true;
      }
      return false;
    };
  } else {
    const incRange = pat.match(/^(-?\d+(?:\.\d+)?)\.\.=(-?\d+(?:\.\d+)?)$/);
    if (incRange) {
      const min = Number(incRange[1]);
      const max = Number(incRange[2]);
      matcher = (actualValue: any) =>
        typeof actualValue === 'number' && actualValue >= min && actualValue <= max;
    } else {
      const excRange = pat.match(/^(-?\d+(?:\.\d+)?)\.\.(-?\d+(?:\.\d+)?)$/);
      if (excRange) {
        const min = Number(excRange[1]);
        const max = Number(excRange[2]);
        matcher = (actualValue: any) =>
          typeof actualValue === 'number' && actualValue >= min && actualValue < max;
      } else if (
        (pat.startsWith("'") && pat.endsWith("'")) ||
        (pat.startsWith('"') && pat.endsWith('"'))
      ) {
        const expected = pat.slice(1, -1);
        matcher = (actualValue: any) => String(actualValue) === expected;
      } else if (pat.includes('.')) {
        const shortVariant = pat.split('.').pop()!;
        matcher = (actualValue: any) => {
          const str = String(actualValue);
          return str === pat || str === shortVariant;
        };
      } else if (/^-?\d+(\.\d+)?$/.test(pat)) {
        const expectedNum = Number(pat);
        matcher = (actualValue: any) => Number(actualValue) === expectedNum;
      } else if (pat === 'true') {
        matcher = (actualValue: any) => actualValue === true;
      } else if (pat === 'false') {
        matcher = (actualValue: any) => actualValue === false;
      } else {
        matcher = (actualValue: any) => String(actualValue) === pat;
      }
    }
  }

  _patternMatcherCache.set(pattern, matcher);
  return matcher;
}

export function doesFormaPatternMatch(pattern: string, actualValue: any): boolean {
  return compileFormaPatternMatcher(pattern)(actualValue);
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

/** Read one contract token, including inline forms and quoted closing brackets. */
function readFormaContract(source: string, start: number): { content: string; end: number } | null {
  let quote: string | null = null;
  for (let i = start + 2; i < source.length; i++) {
    const ch = source[i];
    if (quote) {
      if (ch === '\\') { i++; continue; }
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
      if (depth !== 0) throw new Error('Forma contract annotation is unsupported inside nested blocks');
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
  if (/^range\b/.test(content) && !/^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/.test(content)) {
    return 'Malformed Forma contract annotation: expected range(min, max)';
  }
  if (/^pattern\b/.test(content) && !FORMA_PATTERN_CONTRACT.test(content)) {
    return 'Malformed Forma contract annotation: expected pattern(quoted regex)';
  }
  const range = content.match(/^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/);
  const pattern = content.match(FORMA_PATTERN_CONTRACT);
  if (!range && !pattern) return 'Unsupported Forma contract annotation: ' + content;
  if (range) {
    const min = Number(range[1]), max = Number(range[2]);
    if (!Number.isFinite(min) || !Number.isFinite(max) || min > max) {
      return 'Invalid Forma contract annotation: range requires finite ordered bounds';
    }
  }
  if (pattern) {
    if (!isSafePatternRegex(pattern[2])) return 'Unsafe Forma pattern contract annotation';
    try { new RegExp(pattern[2]); }
    catch { return 'Invalid regular expression in Forma pattern contract annotation'; }
  }
  return undefined;
}

const FORMA_AST_EXIT = Symbol('forma-ast-exit');
const FORMA_AST_STRINGS = ['tag','slotName','conditionExpr','itemVar','listExpr','keyExpr','matchExpr'] as const;
const FORMA_AST_REQUIRED: Partial<Record<FormaAstNode['kind'], Array<keyof FormaAstNode>>> = {
  element: ['tag'], if: ['conditionExpr'], for: ['itemVar', 'listExpr'],
  match: ['matchExpr'], slot: ['slotName'],
};
/** Iterative DFS distinguishes cycles from reusable acyclic node references. */
function assertFormaAstTree(ast: FormaDocumentAst): void {
  const fail = (reason: string): never => { throw new Error('Invalid Forma AST tree: ' + reason); };
  const stringMap = (value: unknown): boolean => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
    for (const key in value) if (Object.hasOwn(value, key) && typeof (value as Record<string, unknown>)[key] !== 'string') return false;
    return true;
  };
  const strings = (value: unknown): boolean => {
    if (!Array.isArray(value)) return false;
    for (const entry of value) if (typeof entry !== 'string') return false;
    return true;
  };
  if (!Array.isArray(ast.rootAstNodes)) fail('expected rootAstNodes array');
  const active = new Set<FormaAstNode>(), done = new Set<FormaAstNode>();
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
      active.delete(finished); done.add(finished); continue;
    }
    if (!node || typeof node !== 'object') fail('expected node object');
    if (active.has(node)) fail('cyclic node reference');
    if (done.has(node)) continue;
    switch (node.kind) {
      case 'element': case 'if': case 'for': case 'match': case 'slot': case 'contentPresenter': break;
      default: fail('unknown node kind');
    }
    if (!stringMap(node.properties) || !stringMap(node.twoWayBindings) || !stringMap(node.eventHandlers)) fail('expected expression maps');
    if (!strings(node.appliedStyles) || !strings(node.forwardedProps)) fail('expected style/forward arrays');
    if (!Array.isArray(node.children)) fail('expected children array');
    if (node.transition !== undefined && !stringMap(node.transition)) fail('invalid transition map');
    for (const key of FORMA_AST_STRINGS) {
      if (node[key] !== undefined && typeof node[key] !== 'string') fail('invalid ' + key);
    }
    for (const key of FORMA_AST_REQUIRED[node.kind] ?? []) {
      const value = node[key];
      if (typeof value !== 'string' || !value.trim()) fail('missing ' + node.kind + '.' + key);
    }
    if (node.keyExpr !== undefined && !node.keyExpr.trim()) fail('empty key expression');
    if (node.kind === 'match' && !Array.isArray(node.matchArms)) fail('missing match arms');
    active.add(node); stack.push(node, FORMA_AST_EXIT);
    pushBranch(node.children); pushBranch(node.elseChildren); pushBranch(node.emptyChildren);
    if (node.matchArms !== undefined) {
      if (!Array.isArray(node.matchArms)) fail('expected match arms array');
      for (const arm of node.matchArms) {
        if (!arm || typeof arm.pattern !== 'string' || !stringMap(arm.propertyAssignments) || !Array.isArray(arm.childNodes)) fail('invalid match arm');
        pushBranch(arm.childNodes);
      }
    }
  }
}

function assertFormaAstMetadata(ast: FormaDocumentAst): void {
  const fail = (reason: string): never => { throw new Error('Invalid Forma AST document: ' + reason); };
  const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === 'object' && !Array.isArray(value);
  if (typeof ast.componentName !== 'string' || !ast.componentName.trim()) fail('missing component name');
  if (!record(ast.tokens) || !record(ast.initialStateValues)) fail('expected token/state maps');
  for (const map of [ast.styles, ast.i18nDictionaries]) {
    if (!record(map)) fail('expected style/i18n maps');
    for (const entry of Object.values(map)) {
      if (!record(entry) || Object.values(entry).some(value => typeof value !== 'string')) fail('expected style/i18n string maps');
    }
  }
  if (!Array.isArray(ast.letExpressions) || !Array.isArray(ast.shortcuts)) fail('expected let/shortcut arrays');
  for (const name of Object.keys(ast.initialStateValues)) {
    if (!/^[A-Za-z0-9_]+$/.test(name) || FORBIDDEN_MEMBER_KEYS.has(name)) fail('invalid state name');
  }
  const letNames = new Set<string>();
  for (const entry of ast.letExpressions) {
    if (!entry || typeof entry.name !== 'string' || !/^[A-Za-z0-9_]+$/.test(entry.name) || FORBIDDEN_MEMBER_KEYS.has(entry.name) || typeof entry.expr !== 'string' || !entry.expr.trim()) fail('invalid let declaration');
    if (letNames.has(entry.name)) fail('duplicate let ' + entry.name);
    letNames.add(entry.name);
  }
  for (const entry of ast.shortcuts) {
    if (!entry || typeof entry.combo !== 'string' || !entry.combo.trim() || typeof entry.action !== 'string' || !entry.action.trim()) fail('invalid shortcut declaration');
  }
}

/** Validate AST contract data before construction or document mutation. */
function assertFormaAstContracts(ast: FormaDocumentAst): void {
  const fail = (reason: string): never => { throw new Error('Invalid Forma AST contract: ' + reason); };
  if (!ast || !Array.isArray(ast.propConstraints)) fail('expected propConstraints array');
  const names = new Set<string>();
  for (const c of ast.propConstraints) {
    if (!c || typeof c.name !== 'string' || !/^[A-Za-z0-9_]+$/.test(c.name) || FORBIDDEN_MEMBER_KEYS.has(c.name)) fail('invalid prop name');
    if (names.has(c.name)) fail('duplicate prop ' + c.name);
    names.add(c.name);
    if (typeof c.type !== 'string' || !c.type.trim() || typeof c.required !== 'boolean') fail('invalid prop metadata');
    if (c.range !== undefined) {
      if (!Array.isArray(c.range) || c.range.length !== 2 ||
          typeof c.range[0] !== 'number' || !Number.isFinite(c.range[0]) ||
          typeof c.range[1] !== 'number' || !Number.isFinite(c.range[1]) || c.range[0] > c.range[1]) {
        fail('range requires two finite ordered bounds');
      }
    }
    if (c.patternSource !== undefined) {
      if (typeof c.patternSource !== 'string' || !c.patternSource.length || !isSafePatternRegex(c.patternSource)) fail('invalid or unsafe pattern');
      try { new RegExp(c.patternSource); } catch { fail('invalid pattern regex'); }
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
      result = { kind: node.kind, tag: node.tag, slotName: node.slotName,
        conditionExpr: node.conditionExpr, itemVar: node.itemVar, listExpr: node.listExpr,
        keyExpr: node.keyExpr, matchExpr: node.matchExpr, properties: { ...node.properties }, twoWayBindings: { ...node.twoWayBindings },
        eventHandlers: { ...node.eventHandlers }, appliedStyles: [...node.appliedStyles],
        forwardedProps: [...node.forwardedProps], children: [],
        transition: node.transition === undefined ? undefined : { ...node.transition } };
      copies.set(node, result); pending.push(node);
    }
    return result;
  };
  const rootAstNodes = ast.rootAstNodes.map(copy);
  while (pending.length) {
    const node = pending.pop()!, result = copies.get(node)!;
    result.children = node.children.map(copy);
    result.elseChildren = node.elseChildren?.map(copy);
    result.emptyChildren = node.emptyChildren?.map(copy);
    result.matchArms = node.matchArms?.map(arm => ({ pattern: arm.pattern,
      propertyAssignments: { ...arm.propertyAssignments }, childNodes: arm.childNodes.map(copy) }));
  }
  const snapshot: FormaDocumentAst = { componentName: ast.componentName, rootAstNodes,
    tokens: structuredCloneIfObject(ast.tokens), styles: structuredCloneIfObject(ast.styles),
    i18nDictionaries: structuredCloneIfObject(ast.i18nDictionaries),
    initialStateValues: Object.fromEntries(Object.entries(ast.initialStateValues).map(([key, value]) => [key, structuredCloneIfObject(value)])),
    propConstraints: ast.propConstraints.map(c => ({ name: c.name, type: c.type, required: c.required, patternSource: c.patternSource,
      range: c.range === undefined ? undefined : [c.range[0], c.range[1]],
      defaultValue: structuredCloneIfObject(c.defaultValue) })),
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
      const rangeMatch = token.content.match(/^range\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$/);
      if (rangeMatch) {
        if (pendingRange !== undefined) throw new Error('Duplicate Forma range contract annotation');
        pendingRange = [Number(rangeMatch[1]), Number(rangeMatch[2])];
      }
      const patternMatch = token.content.match(FORMA_PATTERN_CONTRACT);
      if (patternMatch && pendingPatternSource !== undefined) throw new Error('Duplicate Forma pattern contract annotation');
      if (patternMatch && isSafePatternRegex(patternMatch[2])) {
        try { new RegExp(patternMatch[2]); pendingPatternSource = patternMatch[2]; }
        catch { pendingPatternSource = undefined; }
      }
      i = token.end;
      continue;
    }

    if ((pendingRange !== undefined || pendingPatternSource !== undefined) && !/^(?:required\s+)?prop\s+/.test(src.slice(i))) {
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
      if (FORBIDDEN_MEMBER_KEYS.has(name)) throw new Error('Invalid Forma AST document: reserved declaration name');
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
      if (FORBIDDEN_MEMBER_KEYS.has(name)) throw new Error('Invalid Forma AST document: reserved declaration name');
      const initVal = parseLiteralValue(stateMatch[3].trim());
      initialStateValues[name] = structuredCloneIfObject(initVal);
      continue;
    }

    // `let <name> = <expr>`
    const letMatch = stmt.match(/^let\s+([a-zA-Z0-9_]+)\s*=\s*(.+)$/);
    if (letMatch) {
      const letName = letMatch[1].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(letName)) throw new Error('Invalid Forma AST document: reserved let name');
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
  options?: FormaCompileOptions
): FormaRuntimeInstance {
  return compileFormaComponent(astOrSource, options);
}

// Synchronous factory construction owns every completed Forma instance it creates,
// including instances whose handle is lost when the factory throws.
type FormaConstructionDomain = { active: Set<FormaRuntimeInstance> | null };
const formaConstructionHost = globalThis as unknown as Record<symbol, FormaConstructionDomain | undefined>;
const formaConstructionDomain = formaConstructionHost[Symbol.for('svet.forma.construction.v1')] ??= { active: null };
function constructFormaChild(factory: () => FormaRuntimeInstance): FormaRuntimeInstance {
  const previous = formaConstructionDomain.active;
  const created = new Set<FormaRuntimeInstance>();
  const owner = getCurrentOwner();
  formaConstructionDomain.active = created;
  const release = () => {
    const instances = [...created].reverse();
    created.clear();
    for (const instance of instances) {
      try { instance.dispose(); } catch { /* Drain siblings after a cleanup failure. */ }
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
  } finally { formaConstructionDomain.active = previous; }
}

/** A retained terminal public handle must not retain the compiler's native context. */
function createFormaPublicHandle(implementation: FormaRuntimeInstance): FormaRuntimeInstance {
  const cell: { current: FormaRuntimeInstance | null; closing: boolean; closed: any; leases: Set<{ stop: (() => void) | null }> } = {
    current: implementation, closing: false, closed: null, leases: new Set(),
  };
  const componentName = implementation.componentName;
  implementation = null!;
  const handle = {} as FormaRuntimeInstance;
  const live = () => { if (!cell.current || cell.closing) throw new Error('Forma instance is disposed'); return cell.current; };
  const actions = {} as FormaStateMutator;
  const actionSlots: Array<{ original: Function | null; override: Function | null }> = [];
  for (const key of ['toggle','set','increment','decrement','push','remove','reset'] as const) {
    const slot = { original: cell.current!.actions[key] as Function | null, override: null as Function | null };
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
  Object.defineProperty(handle,'componentName',{enumerable:true,value:componentName});
  Object.defineProperty(handle,'actions',{enumerable:true,value:actions});
  for (const key of ['state','props','computedLets','locale','isValid'] as const) {
    Object.defineProperty(handle,key,{enumerable:true,get:() => cell.current ? cell.current[key] : cell.closed[key]});
  }
  for (const key of ['setState','emitEvent','restoreHmrSnapshot','hotReload','render'] as const) {
    (handle as any)[key] = (...args: any[]) => (live()[key] as Function).apply(handle,args);
  }
  handle.getElementByKey = key => cell.current?.getElementByKey(key);
  handle.exportHmrSnapshot = () => {
    if (cell.current) return cell.current.exportHmrSnapshot();
    return { componentName, locale: cell.closed.locale.peek(),
      state: Object.fromEntries(Object.entries(cell.closed.state).map(([key,value]) => [key, structuredCloneIfObject((value as Signal<any>).peek())])),
      props: Object.fromEntries(Object.entries(cell.closed.props).map(([key,value]) => [key, structuredCloneIfObject((value as Signal<any>).peek())])) };
  };
  handle.mount = container => {
    const mounted = live().mount.call(handle,container);
    return { unmount: leaseFormaCleanup(mounted.unmount,cell.leases) };
  };
  handle.onEvent = (name,callback) => leaseFormaCleanup(live().onEvent(name,callback),cell.leases);
  handle.dispose = () => {
    if (!cell.current || cell.closing) return;
    cell.closing = true;
    const current = cell.current;
    let failed = false, failure: unknown;
    try { current.dispose.call(handle); } catch (error) { failed = true; failure = error; }
    try { cell.closed = snapshotClosedForma(current); }
    catch (error) {
      cell.closed = { state: Object.create(null), props: Object.create(null), computedLets: Object.create(null), locale: cachedFormaValue(''), isValid: cachedFormaValue(undefined) };
      if (!failed) { failed = true; failure = error; }
    } finally {
      cell.current = null; cell.closing = false;
      for (const slot of actionSlots) { slot.original = null; slot.override = null; }
      for (const lease of cell.leases) lease.stop = null;
      cell.leases.clear();
    }
    if (failed) throw failure;
  };
  return handle;
}
function leaseFormaCleanup(stop: () => void, leases: Set<{ stop: (() => void) | null }>): () => void {
  const lease = { stop: stop as (() => void) | null }; leases.add(lease); stop = null!;
  return () => { const current = lease.stop; if (!current) return; lease.stop = null; leases.delete(lease); current(); };
}
function cachedFormaValue(value: any): Signal<any> {
  const getter = (() => value) as Signal<any>;
  Object.assign(getter,{id:-1,__svet_sig:true,peek:() => value,set:() => {},update:() => {},dispose:() => {}});
  return getter;
}
function snapshotClosedForma(instance: FormaRuntimeInstance) {
  const copies = new WeakMap<object,Signal<any>>();
  const copy = (resource: any, allowSignalPeek: boolean) => {
    let result = copies.get(resource);
    if (!result) {
      // Signal.peek is a cached read. Computed.peek can execute user code and
      // must never be used as a fallback while closing an instance.
      const value = typeof resource.__svet_cached_value === 'function'
        ? resource.__svet_cached_value()
        : allowSignalPeek ? resource.peek() : undefined;
      result = cachedFormaValue(value);
      copies.set(resource,result);
    }
    return result;
  };
  const map = (values: Record<string, any>, allowSignalPeek: boolean) =>
    Object.fromEntries(Object.entries(values).map(([key,value]) => [key,copy(value,allowSignalPeek)]));
  return { state:map(instance.state,true), props:map(instance.props,true),
    computedLets:map(instance.computedLets,false), locale:copy(instance.locale,true),
    isValid:copy(instance.isValid,false) };
}

/**
 * Compiles a Forma `.ui` source file or pre-parsed `FormaDocumentAst` into a live, reactive **Svet + WASM** `FormaRuntimeInstance`.
 */
export function compileFormaComponent(
  uiSourceOrAst: string | FormaDocumentAst,
  options?: FormaCompileOptions
): FormaRuntimeInstance {
  let constructing = true, disposed = false;
  const constructionDisposers: Array<() => void> = [];
  const trackConstruction = <T extends { dispose(): void }>(value: T): T => {
    if (disposed) {
      try { value.dispose(); } finally { throw new Error('Forma instance is disposed'); }
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
  const initialAst = typeof uiSourceOrAst === 'string'
    ? parseFormaSource(uiSourceOrAst) : snapshotFormaAst(uiSourceOrAst);
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
  const assertLive = () => { if (disposed) throw new Error('Forma instance is disposed'); };
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
      if (valOrSig && typeof valOrSig === 'object' && 'signal' in valOrSig && typeof (valOrSig as any).signal === 'function') {
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

  const applyDocumentAst = (docAst: FormaDocumentAst, isHotReload = false, readOptionProps = true) => {
    for (const k of Object.keys(tokens)) delete tokens[k];
    Object.assign(tokens, structuredCloneIfObject(docAst.tokens));

    for (const k of Object.keys(i18nDictionaries)) delete i18nDictionaries[k];
    Object.assign(i18nDictionaries, structuredCloneIfObject(docAst.i18nDictionaries));

    for (const k of Object.keys(styles)) delete styles[k];
    Object.assign(styles, structuredCloneIfObject(docAst.styles));

    propConstraints.length = 0;
    for (const c of docAst.propConstraints) {
      const defVal =
        readOptionProps && options?.props && Object.prototype.hasOwnProperty.call(options.props, c.name)
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
    if (retirePrevious) for (const oldComp of ownedComputedLets) {
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
      ensureStateSignal(field, false).update((v) => !v);
    },
    set(field: string, value: any) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, value).set(value);
    },
    increment(field: string, delta = 1) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, 0).update((v) => Number(v || 0) + delta);
    },
    decrement(field: string, delta = 1) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, 0).update((v) => Number(v || 0) - delta);
    },
    push(field: string, item: any) {
      if (FORBIDDEN_MEMBER_KEYS.has(field)) return;
      ensureStateSignal(field, []).update((arr) => (Array.isArray(arr) ? [...arr, item] : [item]));
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

  const executeFormaAction = (actionStr: string, locals?: Record<string, any>, domEvent?: Event) => {
    const trimmed = actionStr.trim();
    const callScope = baseScopeWithLocals(locals);

    // Parse action expression via `parseFormaExprAst` so commas inside strings `'Иванов, И.И.'`
    // or arrays `['a', 'b']` or nested calls `clamp(1, 2, 3)` are handled with 100% accuracy!
    const ast = parseFormaExprAst(trimmed);
    if (ast.type === 'call' && ast.callee.type === 'member' && ast.callee.object.type === 'ident') {
      const targetNamespace = ast.callee.object.name;
      const methodName = ast.callee.property;
      const parsedArgs = ast.args.map((argNode) => evalAstNode(argNode, callScope));

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
      const parsedArgs = ast.args.map((argNode) => evalAstNode(argNode, callScope));
      options?.actions?.[methodName]?.(...parsedArgs, domEvent);
      options?.onAction?.(methodName, parsedArgs, domEvent);
      emitEvent(methodName, ...parsedArgs);
    }
  };

  const baseScopeWithLocals = (locals?: Record<string, any>): FormaEvalScope =>
    locals === undefined ? evalScope : { ...evalScope, locals };

  const renderAstChildren = (children: FormaAstNode[], locals?: Record<string, any>): DocumentFragment => {
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
      const compiledArms = (ast.matchArms ?? []).map((arm) => ({
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
              ? (item) => {
                  keyLocals[itemVar] = item;
                  return evalAstNode(keyAst, keyScope!);
                }
              : (_item, idx) => idx,
            children: (_item, _index, current) => {
              const childLocals = locals
                ? Object.defineProperties({}, Object.getOwnPropertyDescriptors(locals))
                : {};
              Object.defineProperty(childLocals, itemVar, { get: current, enumerable: true, configurable: true });
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

    // Declarative 3D GPU Widget inside Forma `.ui`: `Scene3D { rotX: ...; rotY: ...; rotZ: ...; scale: ...; width: ...; height: ...; }`
    if (tag === 'Scene3D' || tag === 'OrbitCube3D' || tag === 'Viewport3D') {
      const widthAst = mergedProps.width ? parseFormaExprAst(mergedProps.width) : null;
      const heightAst = mergedProps.height ? parseFormaExprAst(mergedProps.height) : null;
      const rotXAst = mergedProps.rotX ? parseFormaExprAst(mergedProps.rotX) : null;
      const rotYAst = mergedProps.rotY ? parseFormaExprAst(mergedProps.rotY) : null;
      const rotZAst = mergedProps.rotZ ? parseFormaExprAst(mergedProps.rotZ) : null;
      const scaleAst = mergedProps.scale ? parseFormaExprAst(mergedProps.scale) : null;
      const width = () => (widthAst ? Number(evalAstNode(widthAst, localScope)) || 320 : 320);
      const height = () => (heightAst ? Number(evalAstNode(heightAst, localScope)) || 180 : 180);
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
        const rx = rotXAst ? Number(evalAstNode(rotXAst, localScope)) || 0 : 0.4;
        const ry = rotYAst ? Number(evalAstNode(rotYAst, localScope)) || 0 : 0.6;
        const rz = rotZAst ? Number(evalAstNode(rotZAst, localScope)) || 0 : 0.1;
        transform.rotation.set([rx, ry, rz]);
      });

      if (scaleAst) {
        effect(() => {
          const s = Number(evalAstNode(scaleAst, localScope)) || 1;
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
          : compileFormaComponent(customCompDef, {
              props: initialChildProps,
              locale: locale.peek(),
              components: options?.components,
            }));

      // A callback may retire this view before returning its newly created child.
      // A closed owner cannot register cleanup, so release that child immediately.
      if (disposed || isOwnerScopeDisposed(childOwner)) {
        try { childInst.dispose(); }
        finally { assertLive(); }
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
      Boolean(mergedProps.multiline && evaluateFormaExpression(mergedProps.multiline, localScope));
    if (isButtonTag) domTag = 'button';
    else if (tag === 'TextInput' || tag === 'TextField') domTag = isMultilineInput ? 'textarea' : 'input';
    else if (tag === 'Slider') domTag = 'input';
    else if (tag === 'Text' || tag === 'Span' || tag === 'Badge' || tag === 'Tag' || tag === 'Chip' || tag === 'Spacer') domTag = 'span';
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
        ? (document.createElementNS('http://www.w3.org/2000/svg', 'svg') as unknown as HTMLElement)
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
      const strokeWidthAst = mergedProps.strokeWidth ? parseFormaExprAst(mergedProps.strokeWidth) : null;
      const fillAst = mergedProps.fill ? parseFormaExprAst(mergedProps.fill) : null;
      const path1Ast = mergedProps.path ? parseFormaExprAst(mergedProps.path) : null;
      const path2Ast = mergedProps.path2 ? parseFormaExprAst(mergedProps.path2) : null;
      const applySvg = () => {
        const size = Number(sizeAst ? evalAstNode(sizeAst, localScope) : 15) || 15;
        const strokeWidth = Number(strokeWidthAst ? evalAstNode(strokeWidthAst, localScope) : 2) || 2;
        const fill = String(fillAst ? evalAstNode(fillAst, localScope) ?? 'none' : 'none');
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
        (astNode) => !astNode || isStaticFormaExprNode(astNode, locals)
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

    const hasChildren = ast.children.length > 0;
    let leadingTextNode: Text | null = null;

    // Apply reactive properties
    for (const [propName, rawExpr] of Object.entries(mergedProps)) {
      if (propName === 'key') continue;
      if (tag === 'SvgIcon' && (propName === 'path' || propName === 'path2' || propName === 'size' || propName === 'strokeWidth' || propName === 'fill')) {
        continue;
      }

      if (propName === 'text' || (propName === 'label' && tag !== 'Checkbox')) {
        if (hasChildren) {
          if (!leadingTextNode) {
            leadingTextNode = document.createTextNode('');
            el.appendChild(leadingTextNode);
          }
          const tNode = leadingTextNode;
          bindExpr(rawExpr, (val) => {
            tNode.nodeValue = val == null ? '' : String(val);
          });
        } else {
          bindExpr(rawExpr, (val) => {
            el.textContent = val == null ? '' : String(val);
          });
        }
      } else if (propName === 'label' && tag === 'Checkbox' && checkboxLabelNode) {
        const lNode = checkboxLabelNode;
        bindExpr(rawExpr, (val) => {
          lNode.nodeValue = val ? ` ${String(val)}` : '';
        });
      } else if (propName === 'checked' && tag === 'Checkbox' && checkboxInput) {
        const inp = checkboxInput;
        bindExpr(rawExpr, (val) => {
          inp.checked = Boolean(val);
        });
      } else if (propName === 'value' && (domTag === 'input' || domTag === 'textarea')) {
        bindExpr(rawExpr, (val) => {
          const strVal =
            tag === 'Slider' && typeof val === 'string' && val.endsWith('%')
              ? val.slice(0, -1)
              : val == null
                ? ''
                : String(val);
          (el as HTMLInputElement | HTMLTextAreaElement).value = strVal;
        });
      } else if ((propName === 'min' || propName === 'max' || propName === 'step') && tag === 'Slider') {
        bindExpr(rawExpr, (v) => {
          if (v !== undefined && v !== null) {
            el.setAttribute(propName, String(v));
          }
        });
      } else if (propName === 'readOnly' && (domTag === 'input' || domTag === 'textarea')) {
        bindExpr(rawExpr, (v) => {
          const isRo = Boolean(v);
          (el as HTMLInputElement | HTMLTextAreaElement).readOnly = isRo;
          if (isRo) el.setAttribute('readonly', '');
          else el.removeAttribute('readonly');
        });
      } else if (propName === 'wrap' && (tag === 'Text' || tag === 'Span' || tag === 'Paragraph' || domTag === 'textarea')) {
        bindExpr(rawExpr, (v) => {
          const shouldWrap = Boolean(v);
          if (domTag === 'textarea') {
            el.setAttribute('wrap', shouldWrap ? 'soft' : 'off');
          }
          el.style.whiteSpace = shouldWrap ? 'pre-wrap' : 'nowrap';
          el.style.overflowWrap = shouldWrap ? 'break-word' : 'normal';
        });
      } else if (propName === 'fontSize' || propName === 'font.size') {
        bindExpr(rawExpr, (v) => {
          el.style.fontSize = typeof v === 'number' ? `${v}px` : String(v ?? '');
        });
      } else if ((propName === 'value' || propName === 'max') && domTag === 'progress') {
        bindExpr(rawExpr, (v) => {
          const val = Number(v ?? (propName === 'max' ? 100 : 0));
          el.setAttribute(propName, String(val));
          (el as HTMLProgressElement)[propName] = val;
        });
      } else if (propName === 'disabled') {
        bindExpr(rawExpr, (v) => {
          const isDis = Boolean(v);
          (el as HTMLButtonElement).disabled = isDis;
          if (isDis) el.setAttribute('disabled', '');
          else el.removeAttribute('disabled');
        });
      } else if (propName === 'class' || propName === 'className') {
        bindExpr(rawExpr, (v) => {
          userClassStr = v == null ? '' : String(v).trim();
          syncClassAttribute();
        });
      } else if (propName === 'active') {
        bindExpr(rawExpr, (v) => {
          isActiveClass = Boolean(v);
          syncClassAttribute();
        });
      } else if (propName === 'id' || propName === 'role' || propName === 'title' || propName === 'tooltip') {
        const attrName = propName === 'tooltip' ? 'title' : propName;
        bindExpr(rawExpr, (v) => {
          if (v !== undefined && v !== null && v !== '') {
            el.setAttribute(attrName, String(v));
          } else {
            el.removeAttribute(attrName);
          }
        });
      } else if (propName === 'tabindex') {
        bindExpr(rawExpr, (v) => {
          if (v !== undefined && v !== null) {
            el.setAttribute('tabindex', String(Number(v)));
          } else {
            el.removeAttribute('tabindex');
          }
        });
      } else if (propName === 'testId') {
        bindExpr(rawExpr, (v) => {
          if (v !== undefined && v !== null && v !== '') {
            el.setAttribute('data-testid', String(v));
          } else {
            el.removeAttribute('data-testid');
          }
        });
      } else if (propName.startsWith('aria') && propName.length > 4) {
        const ariaAttr = `aria-${propName.slice(4).toLowerCase()}`;
        bindExpr(rawExpr, (v) => {
          if (v !== undefined && v !== null && v !== '') {
            el.setAttribute(ariaAttr, String(v));
          } else {
            el.removeAttribute(ariaAttr);
          }
        });
      } else if (propName === 'placeholder' && (domTag === 'input' || domTag === 'textarea')) {
        bindExpr(rawExpr, (v) => {
          (el as HTMLInputElement | HTMLTextAreaElement).placeholder = String(v ?? '');
        });
      } else if (propName === 'gap') {
        bindExpr(rawExpr, (v) => {
          el.style.gap = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else if (propName === 'padding') {
        bindExpr(rawExpr, (v) => {
          el.style.padding = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else if (propName === 'background') {
        bindExpr(rawExpr, (v) => {
          el.style.background = String(v ?? '');
        });
      } else if (propName === 'color') {
        bindExpr(rawExpr, (v) => {
          el.style.color = String(v ?? '');
        });
      } else if (propName === 'cornerRadius') {
        bindExpr(rawExpr, (v) => {
          el.style.borderRadius = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else if (propName === 'width' || propName === 'height') {
        bindExpr(rawExpr, (v) => {
          (el.style as any)[propName] = typeof v === 'number' ? `${v}px` : String(v);
        });
      } else {
        bindExpr(rawExpr, (v) => {
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
      } else if (evtName === 'changed' && !('value' in ast.twoWayBindings) && !('checked' in ast.twoWayBindings)) {
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
        childAst.matchArms.every((a) => a.childNodes.length === 0)
      ) {
        const propMatchAst = parseFormaExprAst(childAst.matchExpr!);
        const compiledPropArms = childAst.matchArms.map((a) => ({
          matches: compileFormaPatternMatcher(a.pattern),
          assignments: Object.entries(a.propertyAssignments).map(
            ([pKey, pExpr]) => [pKey, parseFormaExprAst(pExpr)] as [string, ExprAst]
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
        const nextAst = typeof nextUiSourceOrAst === 'string'
          ? parseFormaSource(nextUiSourceOrAst) : snapshotFormaAst(nextUiSourceOrAst);
        assertLive();
        const previousAst: FormaDocumentAst = {
          componentName, tokens: { ...tokens }, i18nDictionaries: { ...i18nDictionaries }, styles: { ...styles },
          propConstraints: [...propConstraints], initialStateValues: { ...initialStateValues },
          letExpressions: [...letExpressions], shortcuts: [...shortcuts], rootAstNodes: [...rootAstNodes],
        };
        const previousLets = { ...computedLets };
        const previousOwnedLets = new Set(ownedComputedLets);
        const previousState = Object.fromEntries(Object.entries(stateSignals).map(([key, sig]) => [key, sig.peek()]));
        const previousProps = Object.fromEntries(Object.entries(propSignals).map(([key, sig]) => [key, sig.peek()]));
        const previousLocale = locale.peek();
        const prepared: PreparedView[] = [];
        let committing = false, updateFailed = false, previousLetsDetached = false;
        try {
          assertLive();
          batch(() => {
            try {
              applyDocumentAst(nextAst, true);
              assertLive();
              previousLetsDetached = true;
              rebuildComputedLets(false);
              assertLive();
              hmrRevision.update((r) => r + 1);
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
                    try { owned?.dispose(); } catch { /* Keep the preparation failure while draining owned candidates. */ }
                    delete stateSignals[key];
                  } else sig.set(previousState[key]);
                }
                for (const [key, sig] of Object.entries(propSignals)) {
                  if (disposed) throw error;
                  if (!(key in previousProps)) {
                    const owned = ownedPropSignals.get(key);
                    ownedPropSignals.delete(key);
                    try { owned?.dispose(); } catch { /* Drain owned props without replacing the preparation failure. */ }
                    delete propSignals[key];
                  } else sig.set(previousProps[key]);
                }
                if (disposed) throw error;
                for (const comp of ownedComputedLets) {
                  if (!previousOwnedLets.has(comp)) {
                    try { comp.dispose(); } catch { /* Preserve the preparation failure and drain siblings. */ }
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
                hmrRevision.update((r) => r + 1);
              }
              throw error;
            }
          });
        } catch (error) {
          updateFailed = true;
          throw error;
        } finally {
          if ((committing || disposed) && previousLetsDetached) {
            let cleanupFailed = false, cleanupError: unknown;
            for (const comp of previousOwnedLets) {
              try { comp.dispose(); }
              catch (error) { if (!cleanupFailed) cleanupError = error; cleanupFailed = true; }
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
        const previousKeys = keyedElements, nextKeys = new Map<string, HTMLElement>();
        keyedElements = nextKeys;
        try {
          createRoot((dispose) => {
            stage.dispose = dispose; pendingViewDispose = dispose;
            const temp = document.createDocumentFragment();
            for (const nodeAst of rootAstNodes) {
              insertChild(temp, untrack(() => renderAstNode(nodeAst)));
              assertViewLive();
            }
            if (shortcuts.length > 0 && typeof window !== 'undefined') {
              const compiledShortcuts = shortcuts.map((sc) => {
                const parts = sc.combo.split('+');
                const keyLower = parts[parts.length - 1].toLowerCase();
                const needsMod = parts.includes('Mod') || parts.includes('Ctrl') || parts.includes('Meta');
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
              settled = true; abortPrepared = null;
              disposeView = stage.dispose;
              currentNodes = stagedNodes;
              keyedElements = nextKeys;
              currentViewKeys = nextKeys; viewKeyMaps.add(nextKeys);
            },
            abort() {
              if (settled) return;
              settled = true; abortPrepared = null; stage.dispose?.();
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
        } finally { pendingViewDispose = null; }
      };

      mountCurrentView().commit();
      prepareViews.add(mountCurrentView);

      const retireRender = () => {
        if (renderRetired) return;
        renderRetired = true; renderDisposers.delete(retireRender);
        prepareViews.delete(mountCurrentView);
        abortPrepared?.(); pendingViewDispose?.(); clearCurrentView(); anchor.remove();
      };
      renderDisposers.add(retireRender);
      onCleanup(retireRender);
      return frag;
    },
    mount(container: HTMLElement) {
      assertLive();
      container.innerHTML = '';
      let disposeRoot!: () => void;
      const rootFrag = createRoot((dispose) => {
        disposeRoot = dispose;
        return this.render();
      });

      let unmounted = false;
      const unmount = () => {
        if (unmounted) return;
        unmounted = true; mountDisposers.delete(unmount);
        disposeRoot();
      };
      mountDisposers.add(unmount);
      try { container.appendChild(rootFrag); assertLive(); }
      catch (error) { unmount(); throw error; }
      return { unmount };
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      let failed = false, firstError: unknown;
      const recordFailure = (error: unknown) => { if (!failed) firstError = error; failed = true; };
      const release = (resource: { dispose(): void }) => {
        try { resource.dispose(); } catch (error) { recordFailure(error); }
      };
      for (const stop of [...mountDisposers]) { try { stop(); } catch (error) { recordFailure(error); } }
      for (const stop of [...renderDisposers]) { try { stop(); } catch (error) { recordFailure(error); } }
      for (const sig of ownedStateSignals.values()) release(sig);
      ownedStateSignals.clear();
      for (const sig of ownedPropSignals.values()) release(sig);
      ownedPropSignals.clear();
      for (const comp of ownedComputedLets) release(comp);
      ownedComputedLets.clear();
      release(activeI18n); release(isValid); release(locale); release(hmrRevision);
      eventListeners.clear(); keyedElements.clear(); viewKeyMaps.clear();
      mountDisposers.clear(); renderDisposers.clear(); prepareViews.clear();
      if (failed) throw firstError;
    },
  };
  const publicInstance = createFormaPublicHandle(instance);
  formaConstructionDomain.active?.add(publicInstance);
  constructing = false; constructionDisposers.length = 0;
  return publicInstance;
  } catch (error) {
    constructing = false;
    for (const stop of constructionDisposers.reverse()) {
      try { stop(); } catch { /* Preserve the constructor error while draining allocations. */ }
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
      if (!token) { contracts.push({ text: uiSource.slice(i), line: lineNum }); break; }
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
        if (syntaxError) { diagnostics.push({ severity: 'error', message: syntaxError, line }); return; }
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
    try { precompiledAst = parseFormaSource(uiSource); }
    catch (error) {
      diagnostics.push({ severity: 'error', message: error instanceof Error ? error.message : String(error), line: contracts[0]?.line ?? 1 });
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

  const hasError = diagnostics.some((d) => d.severity === 'error');
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
export function compileFormaToJsModule(uiSource: string, importSource = 'svet-engine'): string {
  const report = validateFormaSource(uiSource);
  const precompiledAst = (report as FormaValidationReport & { _ast?: FormaDocumentAst })._ast ?? parseFormaSource(uiSource);
  return `import { compileFormaAst, parseFormaSource } from ${JSON.stringify(importSource)};
export const uiSource = ${JSON.stringify(uiSource)};
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
      const nextAst = newMod.uiAst ?? parseFormaSource(newMod.uiSource);
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
