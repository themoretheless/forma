/**
 * CSP-safe Forma tokenizer, AST parser, static analyzer, and evaluator.
 * Zero `new Function`, zero `eval`, zero `with (__env)`.
 */

import { formaStdlib, transformFormaPipes } from './forma-stdlib.ts';
import type { ReadonlySignal } from './reactivity.ts';

export interface FormaEvalScope {
  state: Record<string, ReadonlySignal<unknown>>;
  props: Record<string, ReadonlySignal<unknown>>;
  computedLets: Record<string, ReadonlySignal<unknown>>;
  tokens: Record<string, unknown>;
  i18n: ReadonlySignal<Record<string, string>>;
  isValid: ReadonlySignal<boolean>;
  locals?: Record<string, unknown>;
  readState?: (key: string) => unknown;
}

export type InterpPart = { literal: true; text: string } | { literal: false; ast: ExprAst };

export type ExprAst =
  | { type: 'literal'; value: unknown }
  | { type: 'interp'; template: string; parts: InterpPart[] }
  | { type: 'ident'; name: string }
  | { type: 'member'; object: ExprAst; property: string; optional?: boolean | undefined }
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

function readQuotedToken(src: string, start: number): { token: ExprToken; nextIndex: number } {
  const n = src.length;
  const q = src.charAt(start);
  let s = '';
  let i = start + 1;
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
    if (src[i] === '$' && src[i + 1] === '{') {
      s += '${';
      i += 2;
      let depth = 1;
      let innerQuote: string | null = null;
      while (i < n && depth > 0) {
        const c = src.charAt(i);
        if (innerQuote !== null) {
          if (c === '\\' && i + 1 < n) {
            const escaped = src.charAt(i + 1);
            s += escaped === q ? escaped : c + escaped;
            i += 2;
            continue;
          }
          if (c === innerQuote) innerQuote = null;
          s += c;
          i++;
          continue;
        }
        if (c === "'" || c === '"') innerQuote = c;
        else if (c === '{') depth++;
        else if (c === '}') depth--;
        s += c;
        i++;
      }
      continue;
    }
    s += src[i++];
  }
  return { token: { kind: 'str', val: s }, nextIndex: i + 1 };
}

function readNumericToken(src: string, start: number): { token: ExprToken; nextIndex: number } {
  const n = src.length;
  let i = start;
  let seenDot = false;
  while (i < n) {
    const c = src.charCodeAt(i);
    if (isCharacterRange(c, 48, 57)) {
      i++;
    } else if (c === 46 && !seenDot && src.charCodeAt(i + 1) !== 46) {
      seenDot = true;
      i++;
    } else {
      break;
    }
  }
  const numStr = src.slice(start, i);
  // Consume optional Forma unit (`px`, `ms`, `s`, `deg`, `rad`, `rem`, `em`, `%`, `turn`)
  const unitStart = i;
  while (i < n) {
    const c = src.charCodeAt(i);
    if (isCharacterRange(c, 65, 90) || isCharacterRange(c, 97, 122) || c === 37) i++;
    else break;
  }
  const unit = src.slice(unitStart, i);
  const token: ExprToken =
    unit && unit !== 'px' && unit !== 'ms'
      ? { kind: 'str', val: numStr + unit }
      : { kind: 'num', val: numStr };
  return { token, nextIndex: i };
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
      const read = readQuotedToken(src, i);
      tokens.push(read.token);
      i = read.nextIndex;
      continue;
    }
    if (isCharacterRange(code, 48, 57)) {
      const read = readNumericToken(src, i);
      tokens.push(read.token);
      i = read.nextIndex;
      continue;
    }
    // Multi-char operators
    if (
      i + 2 < n &&
      (src.startsWith('===', i) || src.startsWith('!==', i) || src.startsWith('..=', i))
    ) {
      tokens.push({ kind: 'op', val: src.slice(i, i + 3) });
      i += 3;
      continue;
    }
    if (i + 1 < n) {
      const c2 = src.charCodeAt(i + 1);
      if (
        code === 63 &&
        c2 === 46 &&
        !isCharacterRange(i + 2 < n ? src.charCodeAt(i + 2) : 0, 48, 57)
      ) {
        tokens.push({ kind: 'punc', val: '?.' });
        i += 2;
        continue;
      }
      if (code === 46 && c2 === 46) {
        tokens.push({ kind: 'op', val: '..' });
        i += 2;
        continue;
      }
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
      const idStart = i++;
      while (i < n && isIdentContinue(src.charCodeAt(i))) i++;
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
      if (i > lastIdx) parts.push({ literal: true, text: template.slice(lastIdx, i) });
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
        if (ch === "'" || ch === '"') quote = ch;
        else if (ch === '{') depth++;
        else if (ch === '}') {
          depth--;
          if (depth === 0) break;
        }
        j++;
      }

      if (depth === 0) {
        if (i > lastIdx) parts.push({ literal: true, text: template.slice(lastIdx, i) });
        parts.push({ literal: false, ast: parseFormaExprAst(template.slice(exprStart, j).trim()) });
        i = j + 1;
        lastIdx = i;
        continue;
      }
    }
    i++;
  }

  if (lastIdx < n) parts.push({ literal: true, text: template.slice(lastIdx) });
  return parts;
}

export function parseFormaExprAst(raw: string): ExprAst {
  const cached = _exprAstCache.get(raw);
  if (cached) return cached;

  const tokens = tokenizeFormaExpr(transformFormaPipes(raw));
  let pos = 0;
  const peek = (): ExprToken | undefined => tokens[pos];
  const next = (): ExprToken | undefined => tokens[pos++];

  const parsePrimary = (): ExprAst => {
    const tok = next();
    if (!tok) return { type: 'literal', value: undefined };
    if (tok.kind === 'num') return { type: 'literal', value: Number(tok.val) };
    if (tok.kind === 'str') {
      return tok.val.includes('${')
        ? { type: 'interp', template: tok.val, parts: compileInterpParts(tok.val) }
        : { type: 'literal', value: tok.val };
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
      if (p.kind === 'punc' && (p.val === '.' || p.val === '?.')) {
        next();
        const propTok = next();
        if (propTok) {
          node = {
            type: 'member',
            object: node,
            property: propTok.val,
            ...(p.val === '?.' ? { optional: true } : {}),
          };
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

  const parseLeftAssoc = (ops: string[], lower: () => ExprAst): ExprAst => {
    let left = lower();
    while (peek() && ops.includes(peek()!.val)) {
      const op = next()!.val;
      left = { type: 'binary', op, left, right: lower() };
    }
    return left;
  };

  const parseMultiplicative = () => parseLeftAssoc(['*', '/', '%'], parseUnary);
  const parseAdditive = () => parseLeftAssoc(['+', '-'], parseMultiplicative);
  const parseRange = () => parseLeftAssoc(['..', '..='], parseAdditive);
  const parseRelational = () => parseLeftAssoc(['<', '>', '<=', '>='], parseRange);
  const parseEquality = () => parseLeftAssoc(['==', '!=', '===', '!=='], parseRelational);
  const parseLogicalAnd = () => parseLeftAssoc(['&&'], parseEquality);
  const parseLogicalOr = () => parseLeftAssoc(['||', '??'], parseLogicalAnd);

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
    if (oldestKey !== undefined) _exprAstCache.delete(oldestKey);
  }
  _exprAstCache.set(raw, ast);
  return ast;
}

function evalIdentExpr(name: string, scope: FormaEvalScope): unknown {
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

function evalMemberExpr(
  node: Extract<ExprAst, { type: 'member' }>,
  scope: FormaEvalScope,
): unknown {
  if (FORBIDDEN_MEMBER_KEYS.has(node.property)) return undefined;
  if (node.object.type === 'ident') {
    const rootName = node.object.name;
    if (rootName === 'state') {
      if (Object.prototype.hasOwnProperty.call(scope.state, node.property)) {
        const sig = scope.state[node.property];
        return typeof sig === 'function' ? sig() : sig;
      }
      return scope.readState ? scope.readState(node.property) : undefined;
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
  return (objVal as Record<string, unknown>)[node.property];
}

function evalCallExpr(node: Extract<ExprAst, { type: 'call' }>, scope: FormaEvalScope): unknown {
  const argVals = node.args.map(a => evalAstNode(a, scope));
  if (
    node.callee.type === 'ident' &&
    Object.prototype.hasOwnProperty.call(formaStdlib, node.callee.name)
  ) {
    return (formaStdlib[node.callee.name] as (...args: unknown[]) => unknown)(...argVals);
  }
  if (node.callee.type === 'member') {
    if (FORBIDDEN_MEMBER_KEYS.has(node.callee.property)) return undefined;
    const targetObj = evalAstNode(node.callee.object, scope);
    const method = (targetObj as Record<string, unknown> | null | undefined)?.[
      node.callee.property
    ];
    if (
      typeof method === 'function' &&
      method !== Function &&
      method !== eval &&
      method !== Object.prototype.constructor
    ) {
      return (method as (...args: unknown[]) => unknown).apply(targetObj, argVals);
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
    return (fnVal as (...args: unknown[]) => unknown)(...argVals);
  }
  return undefined;
}

function evalBinaryOp(node: Extract<ExprAst, { type: 'binary' }>, scope: FormaEvalScope): unknown {
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
      return (l as number) + (r as number);
    case '-':
      return Number(l) - Number(r);
    case '*':
      return Number(l) * Number(r);
    case '/':
      return Number(l) / Number(r);
    case '%':
      return Number(l) % Number(r);
    case '<':
      return (l as number) < (r as number);
    case '>':
      return (l as number) > (r as number);
    case '<=':
      return (l as number) <= (r as number);
    case '>=':
      return (l as number) >= (r as number);
    case '==':
    case '===':
      return l === r;
    case '!=':
    case '!==':
      return l !== r;
    case '..':
    case '..=': {
      const start = Math.trunc(Number(l));
      const end = Math.trunc(Number(r));
      if (!Number.isFinite(start) || !Number.isFinite(end)) return [];
      const stop = node.op === '..=' ? end + 1 : end;
      if (stop <= start) return [];
      return Array.from({ length: Math.min(4096, stop - start) }, (_, idx) => start + idx);
    }
    default:
      return undefined;
  }
}

export function evalAstNode(node: ExprAst, scope: FormaEvalScope): unknown {
  switch (node.type) {
    case 'literal':
      return node.value;
    case 'interp': {
      let out = '';
      for (const part of node.parts) {
        if (part.literal) out += part.text;
        else {
          const val = evalAstNode(part.ast, scope);
          if (val != null) out += String(val);
        }
      }
      return out;
    }
    case 'array':
      return node.elements.map(el => evalAstNode(el, scope));
    case 'ident':
      return evalIdentExpr(node.name, scope);
    case 'member':
      return evalMemberExpr(node, scope);
    case 'index': {
      const objVal = evalAstNode(node.object, scope);
      const idxVal = evalAstNode(node.index, scope);
      if (objVal == null || FORBIDDEN_MEMBER_KEYS.has(String(idxVal))) return undefined;
      if (
        typeof objVal === 'object' &&
        !Array.isArray(objVal) &&
        !Object.prototype.hasOwnProperty.call(objVal, idxVal as PropertyKey)
      ) {
        return undefined;
      }
      return (objVal as Record<string, unknown>)[idxVal as string];
    }
    case 'call':
      return evalCallExpr(node, scope);
    case 'unary': {
      const v = evalAstNode(node.arg, scope);
      if (node.op === '!') return !v;
      if (node.op === '-') return -Number(v);
      if (node.op === '+') return +Number(v);
      return v;
    }
    case 'binary':
      return evalBinaryOp(node, scope);
    case 'conditional': {
      const cond = evalAstNode(node.test, scope);
      return cond ? evalAstNode(node.consequent, scope) : evalAstNode(node.alternate, scope);
    }
  }
}

function isTokensRoot(node: ExprAst): boolean {
  if (node.type === 'ident') return node.name === 'tokens';
  if (node.type === 'member') return isTokensRoot(node.object);
  return false;
}

export function isStaticFormaExprNode(node: ExprAst, locals?: Record<string, unknown>): boolean {
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
      if (isTokensRoot(node.object)) return true;
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
    case 'ident':
      return false;
  }
}

export function isStaticFormaExpr(rawExpr: string, locals?: Record<string, unknown>): boolean {
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
export function evaluateFormaExpression(rawExpr: string, scope: FormaEvalScope): unknown {
  const expr = rawExpr.trim();
  if (!expr) return undefined;
  try {
    const ast = parseFormaExprAst(expr);
    return evalAstNode(ast, scope);
  } catch {
    return undefined;
  }
}
