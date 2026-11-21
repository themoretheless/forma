/**
 * Pattern match compiler (`compileFormaPatternMatcher` / `doesFormaPatternMatch`)
 * and canonical Forma IR <-> `ExprAst` conversions.
 */

import {
  type ExprAst,
  FORBIDDEN_MEMBER_KEYS,
  type InterpPart,
  parseFormaExprAst,
} from './forma-expr-parser.ts';

const _patternMatcherCache = new Map<string, (val: unknown) => boolean>();
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

export function compileFormaPatternMatcher(pattern: string): (val: unknown) => boolean {
  const cached = _patternMatcherCache.get(pattern);
  if (cached) return cached;

  const pat = pattern.trim();
  let matcher: (val: unknown) => boolean;

  if (pat === '_') {
    matcher = () => true;
  } else if (isExactQuotedLiteral(pat)) {
    const expected = unescapeQuotedLiteral(pat);
    matcher = (actualValue: unknown) => String(actualValue) === expected;
  } else {
    const unionParts = splitTopLevelPatternOr(pat);
    if (unionParts) {
      const subMatchers = unionParts.map(sub => compileFormaPatternMatcher(sub));
      matcher = (actualValue: unknown) => {
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
        matcher = (actualValue: unknown) =>
          typeof actualValue === 'number' && actualValue >= min && actualValue <= max;
      } else {
        const excRange = pat.match(/^(-?\d+(?:\.\d+)?)\.\.(-?\d+(?:\.\d+)?)$/);
        if (excRange) {
          const min = Number(excRange[1]);
          const max = Number(excRange[2]);
          matcher = (actualValue: unknown) =>
            typeof actualValue === 'number' && actualValue >= min && actualValue < max;
        } else if (pat.includes('.')) {
          const shortVariant = pat.split('.').pop()!;
          matcher = (actualValue: unknown) => {
            const str = String(actualValue);
            return str === pat || str === shortVariant;
          };
        } else if (/^-?\d+(\.\d+)?$/.test(pat)) {
          const expectedNum = Number(pat);
          matcher = (actualValue: unknown) => Number(actualValue) === expectedNum;
        } else if (pat === 'true') {
          matcher = (actualValue: unknown) => actualValue === true;
        } else if (pat === 'false') {
          matcher = (actualValue: unknown) => actualValue === false;
        } else {
          matcher = (actualValue: unknown) => String(actualValue) === pat;
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

export function doesFormaPatternMatch(pattern: string, actualValue: unknown): boolean {
  return compileFormaPatternMatcher(pattern)(actualValue);
}

export interface CanonicalExpressionAst {
  kind: 'unary' | 'binary' | 'conditional' | 'member' | 'call';
  operator?: string | undefined;
  op?: string | undefined;
  argument?: CanonicalFormaValue | undefined;
  operand?: CanonicalFormaValue | undefined;
  arg?: CanonicalFormaValue | undefined;
  left?: CanonicalFormaValue | undefined;
  right?: CanonicalFormaValue | undefined;
  condition?: CanonicalFormaValue | undefined;
  yes?: CanonicalFormaValue | undefined;
  then?: CanonicalFormaValue | undefined;
  no?: CanonicalFormaValue | undefined;
  else?: CanonicalFormaValue | undefined;
  otherwise?: CanonicalFormaValue | undefined;
  object?: CanonicalFormaValue | undefined;
  property?: CanonicalFormaValue | undefined;
  optional?: boolean | undefined;
  callee?: CanonicalFormaValue | undefined;
  name?: string | undefined;
  args?: CanonicalFormaValue[] | undefined;
}

export type CanonicalFormaValue =
  | null
  | boolean
  | number
  | string
  | CanonicalFormaValue[]
  | { expr: string }
  | { interpolation: CanonicalFormaValue[] }
  | { expression: CanonicalExpressionAst }
  | { [key: string]: CanonicalFormaValue };

function escapeSingleQuotedLiteral(text: string): string {
  let out = "'";
  for (let i = 0; i < text.length; i++) {
    const ch = text.charAt(i);
    if (ch === '\\') out += '\\\\';
    else if (ch === "'") out += "\\'";
    else if (ch === '\n') out += '\\n';
    else if (ch === '\r') out += '\\r';
    else if (ch === '\t') out += '\\t';
    else if (ch === '$' && text.charAt(i + 1) === '{') out += '\\$';
    else out += ch;
  }
  return out + "'";
}

function escapeInterpText(text: string): string {
  let out = '';
  for (let i = 0; i < text.length; i++) {
    const ch = text.charAt(i);
    if (ch === '\\') out += '\\\\';
    else if (ch === '"') out += '\\"';
    else if (ch === '\n') out += '\\n';
    else if (ch === '\r') out += '\\r';
    else if (ch === '\t') out += '\\t';
    else if (ch === '$' && text.charAt(i + 1) === '{') out += '\\$';
    else out += ch;
  }
  return out;
}

function stripLetPropsPrefix(rawExpr: string, letNames?: ReadonlySet<string>): string {
  if (!letNames || letNames.size === 0) return rawExpr;
  if (rawExpr.startsWith('props.')) {
    const rest = rawExpr.slice(6);
    const root = rest.split(/[.[]/, 1)[0];
    if (root && letNames.has(root)) return rest;
  }
  if (rawExpr.startsWith('!props.')) {
    const rest = rawExpr.slice(7);
    const root = rest.split(/[.[]/, 1)[0];
    if (root && letNames.has(root)) return '!' + rest;
  }
  return rawExpr;
}

function unwrapCanonicalAst(value: CanonicalFormaValue): CanonicalExpressionAst | null {
  if (
    value &&
    typeof value === 'object' &&
    !Array.isArray(value) &&
    'expression' in value &&
    value.expression &&
    typeof value.expression === 'object'
  ) {
    return value.expression as CanonicalExpressionAst;
  }
  return null;
}

export function canonicalValueToExprString(
  value: CanonicalFormaValue,
  letNames?: ReadonlySet<string>,
): string {
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'boolean' || typeof value === 'number') return String(value);
  if (typeof value === 'string') return escapeSingleQuotedLiteral(value);
  if (Array.isArray(value)) {
    return `[${value.map(v => canonicalValueToExprString(v, letNames)).join(', ')}]`;
  }
  if ('expr' in value && typeof value.expr === 'string') {
    return stripLetPropsPrefix(value.expr, letNames);
  }
  if ('interpolation' in value && Array.isArray(value.interpolation)) {
    let out = '"';
    for (const part of value.interpolation) {
      if (typeof part === 'string') out += escapeInterpText(part);
      else out += '${' + canonicalValueToExprString(part, letNames) + '}';
    }
    return out + '"';
  }
  const ast = unwrapCanonicalAst(value);
  if (ast) {
    switch (ast.kind) {
      case 'unary': {
        const op = ast.operator ?? ast.op ?? '';
        const arg = canonicalValueToExprString(
          ast.argument ?? ast.operand ?? ast.arg ?? null,
          letNames,
        );
        return `(${op}${arg})`;
      }
      case 'binary': {
        const op = ast.operator ?? ast.op ?? '';
        const left = canonicalValueToExprString(ast.left ?? null, letNames);
        const right = canonicalValueToExprString(ast.right ?? null, letNames);
        return `(${left} ${op} ${right})`;
      }
      case 'conditional': {
        const cond = canonicalValueToExprString(ast.condition ?? null, letNames);
        const yes = canonicalValueToExprString(ast.yes ?? ast.then ?? null, letNames);
        const no = canonicalValueToExprString(
          ast.no ?? ast.else ?? ast.otherwise ?? null,
          letNames,
        );
        return `(${cond} ? ${yes} : ${no})`;
      }
      case 'member': {
        const obj = canonicalValueToExprString(ast.object ?? null, letNames);
        const prop = ast.property;
        if (typeof prop === 'string') {
          if (obj === 'props' && letNames?.has(prop)) return prop;
          return ast.optional ? `${obj}?.${prop}` : `${obj}.${prop}`;
        }
        if (prop && typeof prop === 'object' && 'expr' in prop && typeof prop.expr === 'string') {
          if (obj === 'props' && letNames?.has(prop.expr)) return prop.expr;
          return ast.optional ? `${obj}?.${prop.expr}` : `${obj}.${prop.expr}`;
        }
        const keyStr = canonicalValueToExprString(prop ?? null, letNames);
        return `${obj}[${keyStr}]`;
      }
      case 'call': {
        const args = (ast.args ?? []).map(a => canonicalValueToExprString(a, letNames)).join(', ');
        if (typeof ast.name === 'string') return `${ast.name}(${args})`;
        const callee = canonicalValueToExprString(ast.callee ?? null, letNames);
        return `${callee}(${args})`;
      }
    }
  }
  const entries = Object.entries(value).filter(([k]) => !FORBIDDEN_MEMBER_KEYS.has(k));
  return `{ ${entries.map(([k, v]) => `${k}: ${canonicalValueToExprString(v, letNames)}`).join(', ')} }`;
}

export function fromCanonicalFormaExpr(
  value: CanonicalFormaValue,
  letNames?: ReadonlySet<string>,
): ExprAst {
  if (
    value === null ||
    value === undefined ||
    typeof value === 'boolean' ||
    typeof value === 'number' ||
    typeof value === 'string'
  ) {
    return { type: 'literal', value: value ?? null };
  }
  if (Array.isArray(value)) {
    return { type: 'array', elements: value.map(v => fromCanonicalFormaExpr(v, letNames)) };
  }
  if ('expr' in value && typeof value.expr === 'string') {
    return parseFormaExprAst(stripLetPropsPrefix(value.expr, letNames));
  }
  if ('interpolation' in value && Array.isArray(value.interpolation)) {
    const parts: InterpPart[] = value.interpolation.map(part =>
      typeof part === 'string'
        ? { literal: true, text: part }
        : { literal: false, ast: fromCanonicalFormaExpr(part, letNames) },
    );
    return {
      type: 'interp',
      template: canonicalValueToExprString(value, letNames).slice(1, -1),
      parts,
    };
  }
  const ast = unwrapCanonicalAst(value);
  if (ast) {
    switch (ast.kind) {
      case 'unary':
        return {
          type: 'unary',
          op: ast.operator ?? ast.op ?? '',
          arg: fromCanonicalFormaExpr(ast.argument ?? ast.operand ?? ast.arg ?? null, letNames),
        };
      case 'binary':
        return {
          type: 'binary',
          op: ast.operator ?? ast.op ?? '',
          left: fromCanonicalFormaExpr(ast.left ?? null, letNames),
          right: fromCanonicalFormaExpr(ast.right ?? null, letNames),
        };
      case 'conditional':
        return {
          type: 'conditional',
          test: fromCanonicalFormaExpr(ast.condition ?? null, letNames),
          consequent: fromCanonicalFormaExpr(ast.yes ?? ast.then ?? null, letNames),
          alternate: fromCanonicalFormaExpr(ast.no ?? ast.else ?? ast.otherwise ?? null, letNames),
        };
      case 'member': {
        const objAst = fromCanonicalFormaExpr(ast.object ?? null, letNames);
        const prop = ast.property;
        if (typeof prop === 'string') {
          if (objAst.type === 'ident' && objAst.name === 'props' && letNames?.has(prop)) {
            return { type: 'ident', name: prop };
          }
          return {
            type: 'member',
            object: objAst,
            property: prop,
            ...(ast.optional ? { optional: true } : {}),
          };
        }
        if (prop && typeof prop === 'object' && 'expr' in prop && typeof prop.expr === 'string') {
          if (objAst.type === 'ident' && objAst.name === 'props' && letNames?.has(prop.expr)) {
            return { type: 'ident', name: prop.expr };
          }
          return {
            type: 'member',
            object: objAst,
            property: prop.expr,
            ...(ast.optional ? { optional: true } : {}),
          };
        }
        return {
          type: 'index',
          object: objAst,
          index: fromCanonicalFormaExpr(prop ?? null, letNames),
        };
      }
      case 'call':
        return {
          type: 'call',
          callee:
            typeof ast.name === 'string'
              ? { type: 'ident', name: ast.name }
              : fromCanonicalFormaExpr(ast.callee ?? null, letNames),
          args: (ast.args ?? []).map(a => fromCanonicalFormaExpr(a, letNames)),
        };
    }
  }
  return { type: 'literal', value };
}
