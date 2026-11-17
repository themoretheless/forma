/**
 * # CSP-Safe Forma AST Expression Parser & Evaluator (`forma-expr.ts`)
 *
 * Zero `new Function`, Zero `eval`, Zero `with (__env)`:
 * - Tokenizes & parses Forma expressions (including pipe syntax `|>`, string interpolation `${...}`,
 *   array literals, member/index access, calls, unary/binary/ternary operators) into cached `ExprAst` nodes.
 * - Evaluates `ExprAst` against a reactive `FormaEvalScope`, automatically tracking WASM `ReactiveGraph` reads.
 * - Pre-compiles and caches `match` pattern matchers (`compileFormaPatternMatcher`).
 */

import { formaStdlib, transformFormaPipes } from './forma-stdlib.ts';
import type { ReadonlySignal, Signal } from './reactivity.ts';

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

export type InterpPart = { literal: true; text: string } | { literal: false; ast: ExprAst };

export type ExprAst =
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

export const FORBIDDEN_MEMBER_KEYS = new Set([
  '__proto__',
  'prototype',
  'constructor',
  '__defineGetter__',
  '__defineSetter__',
  '__lookupGetter__',
  '__lookupSetter__',
]);

export function isSafePatternRegex(pattern: string): boolean {
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

    const ch = src.charAt(i);
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
  const n = template.length;
  let lastIdx = 0;
  let i = 0;

  while (i < n) {
    if (
      template.charAt(i) === '\\' &&
      template.charAt(i + 1) === '$' &&
      template.charAt(i + 2) === '{'
    ) {
      if (i > lastIdx) {
        parts.push({ literal: true, text: template.slice(lastIdx, i) });
      }
      parts.push({ literal: true, text: '${' });
      i += 3;
      lastIdx = i;
      continue;
    }

    if (template.charAt(i) === '$' && template.charAt(i + 1) === '{') {
      const exprStart = i + 2;
      let j = exprStart;
      let depth = 1;
      let quote: string | null = null;
      while (j < n && depth > 0) {
        const ch = template.charAt(j);
        if (quote) {
          if (ch === '\\') {
            j += 2;
            continue;
          }
          if (ch === quote) quote = null;
          j++;
          continue;
        }
        if (ch === "'" || ch === '"') {
          quote = ch;
          j++;
          continue;
        }
        if (ch === '{') depth++;
        else if (ch === '}') {
          depth--;
          if (depth === 0) break;
        }
        j++;
      }

      if (depth === 0) {
        if (i > lastIdx) {
          parts.push({ literal: true, text: template.slice(lastIdx, i) });
        }
        parts.push({ literal: false, ast: parseFormaExprAst(template.slice(exprStart, j).trim()) });
        i = j + 1;
        lastIdx = i;
        continue;
      }
    }

    i++;
  }

  if (lastIdx < n) {
    parts.push({ literal: true, text: template.slice(lastIdx) });
  }
  return parts;
}

export function parseFormaExprAst(raw: string): ExprAst {
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

export function evalAstNode(node: ExprAst, scope: FormaEvalScope): any {
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
      return node.elements.map(el => evalAstNode(el, scope));

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
        return (scope.computedLets[name] as (typeof scope.computedLets)[string])();
      }
      if (Object.prototype.hasOwnProperty.call(scope.state, name)) {
        return (scope.state[name] as (typeof scope.state)[string])();
      }
      if (Object.prototype.hasOwnProperty.call(scope.props, name)) {
        return (scope.props[name] as (typeof scope.props)[string])();
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
        if (
          !Object.prototype.hasOwnProperty.call(objVal, node.property) &&
          !(node.property in Array.prototype && Array.isArray(objVal)) &&
          !(node.property in String.prototype && typeof objVal === 'string')
        ) {
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
      const argVals = node.args.map(a => evalAstNode(a, scope));
      if (
        node.callee.type === 'ident' &&
        Object.prototype.hasOwnProperty.call(formaStdlib, node.callee.name)
      ) {
        return (formaStdlib[node.callee.name] as (typeof formaStdlib)[string])(...argVals);
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

export function isStaticFormaExprNode(node: ExprAst, locals?: Record<string, any>): boolean {
  switch (node.type) {
    case 'literal':
      return true;
    case 'interp':
      return node.parts.every(p => p.literal || isStaticFormaExprNode(p.ast, locals));
    case 'call':
      return (
        node.callee.type === 'ident' &&
        Object.prototype.hasOwnProperty.call(formaStdlib, node.callee.name) &&
        node.args.every(arg => isStaticFormaExprNode(arg, locals))
      );
    case 'member':
      if (isTokensRoot(node.object)) {
        return true;
      }
      return isStaticFormaExprNode(node.object, locals);
    case 'index':
      return (
        isStaticFormaExprNode(node.object, locals) && isStaticFormaExprNode(node.index, locals)
      );
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
      return node.elements.every(el => isStaticFormaExprNode(el, locals));
    default:
      return false;
  }
}

export function isStaticFormaExpr(rawExpr: string, locals?: Record<string, any>): boolean {
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

const _patternMatcherCache = new Map<string, (val: any) => boolean>();
const MAX_PATTERN_MATCHER_CACHE_SIZE = 2048;

function isExactQuotedLiteral(pat: string): boolean {
  if (pat.length < 2) return false;
  const q = pat.charAt(0);
  if ((q !== "'" && q !== '"') || pat.charAt(pat.length - 1) !== q) return false;
  for (let i = 1; i < pat.length - 1; i++) {
    const ch = pat.charAt(i);
    if (ch === '\\') {
      i++;
      if (i >= pat.length - 1) return false;
      continue;
    }
    if (ch === q) return false;
  }
  return true;
}

function unescapeQuotedLiteral(pat: string): string {
  const inner = pat.slice(1, -1);
  if (!inner.includes('\\')) return inner;
  let out = '';
  for (let i = 0; i < inner.length; i++) {
    const ch = inner.charAt(i);
    if (ch === '\\' && i + 1 < inner.length) {
      const next = inner.charAt(++i);
      if (next === 'n') out += '\n';
      else if (next === 'r') out += '\r';
      else if (next === 't') out += '\t';
      else out += next;
    } else {
      out += ch;
    }
  }
  return out;
}

function splitTopLevelPatternOr(pat: string): string[] | null {
  if (!pat.includes('|')) return null;
  const parts: string[] = [];
  let start = 0;
  let i = 0;
  const n = pat.length;
  while (i < n) {
    const ch = pat.charAt(i);
    if (ch === "'" || ch === '"') {
      const q = ch;
      i++;
      while (i < n && pat.charAt(i) !== q) {
        if (pat.charAt(i) === '\\') i++;
        i++;
      }
      i++;
      continue;
    }
    if (ch === '|') {
      parts.push(pat.slice(start, i).trim());
      start = i + 1;
    }
    i++;
  }
  if (parts.length === 0) return null;
  parts.push(pat.slice(start).trim());
  return parts;
}

export function compileFormaPatternMatcher(pattern: string): (val: any) => boolean {
  let cached = _patternMatcherCache.get(pattern);
  if (cached) return cached;

  const pat = pattern.trim();
  let matcher: (val: any) => boolean;

  if (pat === '_') {
    matcher = () => true;
  } else if (isExactQuotedLiteral(pat)) {
    const expected = unescapeQuotedLiteral(pat);
    matcher = (actualValue: any) => String(actualValue) === expected;
  } else {
    const unionParts = splitTopLevelPatternOr(pat);
    if (unionParts) {
      const subMatchers = unionParts.map(sub => compileFormaPatternMatcher(sub));
      matcher = (actualValue: any) => {
        for (const matches of subMatchers) {
          if (matches(actualValue)) return true;
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
  }

  if (_patternMatcherCache.size >= MAX_PATTERN_MATCHER_CACHE_SIZE) {
    const oldestKey = _patternMatcherCache.keys().next().value;
    if (oldestKey !== undefined) {
      _patternMatcherCache.delete(oldestKey);
    }
  }
  _patternMatcherCache.set(pattern, matcher);
  return matcher;
}

export function doesFormaPatternMatch(pattern: string, actualValue: any): boolean {
  return compileFormaPatternMatcher(pattern)(actualValue);
}
