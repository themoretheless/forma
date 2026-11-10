/**
 * # Forma `.ui` AST & Parser (`forma-ast.ts`)
 *
 * Pre-parses Forma `.ui` source text into a validated, serializable `FormaDocumentAst`
 * and owns admission snapshots (`snapshotFormaAst`, `assertFormaAstContracts`).
 */

import { FORBIDDEN_MEMBER_KEYS, isSafePatternRegex } from './forma-expr.ts';
import { structuredCloneIfObject } from './forma-reactive.ts';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type FormaUntypedValue = any;

export interface FormaPropConstraint {
  name: string;
  type: string;
  required: boolean;
  defaultValue: FormaUntypedValue;
  range?: [number, number] | undefined;
  patternSource?: string | undefined;
}

export interface FormaMatchArm {
  pattern: string;
  propertyAssignments: Record<string, string>;
  childNodes: FormaAstNode[];
}

export interface FormaAstNode {
  kind: 'element' | 'if' | 'for' | 'match' | 'slot' | 'contentPresenter';
  tag?: string | undefined;
  slotName?: string | undefined;
  properties: Record<string, string>;
  twoWayBindings: Record<string, string>;
  eventHandlers: Record<string, string>;
  appliedStyles: string[];
  forwardedProps: string[];
  transition?: Record<string, string> | undefined;
  children: FormaAstNode[];
  // For `if`
  conditionExpr?: string | undefined;
  elseChildren?: FormaAstNode[] | undefined;
  // For `for`
  itemVar?: string | undefined;
  listExpr?: string | undefined;
  keyExpr?: string | undefined;
  emptyChildren?: FormaAstNode[] | undefined;
  // For `match`
  matchExpr?: string | undefined;
  matchArms?: FormaMatchArm[] | undefined;
}

export interface FormaDocumentAst {
  componentName: string;
  tokens: Record<string, FormaUntypedValue>;
  i18nDictionaries: Record<string, Record<string, string>>;
  styles: Record<string, Record<string, string>>;
  propConstraints: FormaPropConstraint[];
  initialStateValues: Record<string, FormaUntypedValue>;
  letExpressions: Array<{ name: string; expr: string }>;
  shortcuts: Array<{ combo: string; action: string }>;
  rootAstNodes: FormaAstNode[];
}

function setNestedObjectKey(obj: Record<string, unknown>, dottedKey: string, value: unknown): void {
  const parts = dottedKey.split('.');
  if (parts.length === 0) return;
  for (const p of parts) {
    if (FORBIDDEN_MEMBER_KEYS.has(p)) {
      return;
    }
  }
  let curr = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    const p = parts[i];
    if (p === undefined) continue;
    const existing = Object.prototype.hasOwnProperty.call(curr, p) ? curr[p] : undefined;
    if (!existing || typeof existing !== 'object' || Array.isArray(existing)) {
      const nextObj = Object.create(null) as Record<string, unknown>;
      curr[p] = nextObj;
      curr = nextObj;
    } else {
      curr = existing as Record<string, unknown>;
    }
  }
  const leaf = parts[parts.length - 1];
  if (leaf !== undefined) {
    curr[leaf] = value;
  }
}

export function extractBraceBlock(
  source: string,
  openBraceIdx: number,
): { body: string; endIdx: number } {
  let depth = 0;
  let i = openBraceIdx;
  const n = source.length;
  while (i < n) {
    const ch = source.charAt(i);
    if (ch === "'" || ch === '"') {
      const q = ch;
      i++;
      while (i < n && source.charAt(i) !== q) {
        if (source.charAt(i) === '\\') i++;
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

export function parseFormaUiBody(bodyStr: string): FormaAstNode[] {
  const nodes: FormaAstNode[] = [];
  let i = 0;
  const src = bodyStr.trim();
  const n = src.length;

  while (i < n) {
    while (i < n && /\s/.test(src.charAt(i))) i++;
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
      while (i < n && /\s/.test(src.charAt(i))) i++;

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
    if (slotDeclMatch && slotDeclMatch[1] !== undefined) {
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
      while (i < n && /\s/.test(src.charAt(i))) i++;

      let emptyChildren: FormaAstNode[] = [];
      if (src.startsWith('empty', i)) {
        const emptyBrace = src.indexOf('{', i + 5);
        if (emptyBrace !== -1) {
          const emptyBlock = extractBraceBlock(src, emptyBrace);
          emptyChildren = parseFormaUiBody(emptyBlock.body);
          i = emptyBlock.endIdx;
        }
      }

      if (forMatch && forMatch[1] !== undefined && forMatch[2] !== undefined) {
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
    if (tagMatch && tagMatch[1] !== undefined) {
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

function parseMatchArms(armsSource: string): FormaMatchArm[] {
  const arms: FormaMatchArm[] = [];

  let i = 0;
  const src = armsSource.trim();
  const n = src.length;

  while (i < n) {
    while (i < n && /\s/.test(src.charAt(i))) i++;
    if (i >= n) break;

    const arrowIdx = src.indexOf('=>', i);
    if (arrowIdx === -1) break;
    const pattern = src.slice(i, arrowIdx).trim();
    i = arrowIdx + 2;
    while (i < n && /\s/.test(src.charAt(i))) i++;

    if (src.charAt(i) === '{') {
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
      if (tagMatch && tagMatch[1] !== undefined) {
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
    while (i < n && /\s/.test(src.charAt(i))) i++;
    if (i >= n) break;

    if (src.startsWith('//', i)) {
      const nl = src.indexOf('\n', i + 2);
      i = nl === -1 ? n : nl + 1;
      continue;
    }

    // Check `forward props { a, b, c };`
    const forwardMatch = src.slice(i).match(/^forward\s+props\s*\{([^}]*)\}\s*;?/);
    if (forwardMatch && forwardMatch[1] !== undefined) {
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
    const ch = src.charAt(i);
    if (ch === "'" || ch === '"') {
      const q = ch;
      i++;
      while (i < n && src.charAt(i) !== q) {
        if (src.charAt(i) === '\\') i++;
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

/** Read one contract token, including inline forms and quoted closing brackets. */
export function readFormaContract(
  source: string,
  start: number,
): { content: string; end: number } | null {
  let quote: string | null = null;
  for (let i = start + 2; i < source.length; i++) {
    const ch = source.charAt(i);
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
    const ch = source.charAt(i);
    if (ch === "'" || ch === '"') {
      const quote = ch;
      for (i++; i < source.length; i++) {
        if (source.charAt(i) === '\\') i++;
        else if (source.charAt(i) === quote) break;
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

export const FORMA_PATTERN_CONTRACT = /^pattern\(\s*(['"])((?:\\[\s\S]|(?!\1)[^\\])+)\1\s*\)$/;

/** Known contracts must have the same complete grammar in parser and validator. */
export function formaContractSyntaxError(content: string): string | undefined {
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
  if (pattern && pattern[2] !== undefined) {
    const regexSource = pattern[2];
    if (!isSafePatternRegex(regexSource)) return 'Unsafe Forma pattern contract annotation';
    try {
      new RegExp(regexSource);
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
    const node = stack.pop();
    if (node === FORMA_AST_EXIT) {
      const finished = stack.pop() as FormaAstNode;
      active.delete(finished);
      done.add(finished);
      continue;
    }
    if (!node || typeof node !== 'object') {
      fail('expected node object');
    } else {
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
export function assertFormaAstContracts(ast: FormaDocumentAst): void {
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
export function snapshotFormaAstImpl(ast: FormaDocumentAst): FormaDocumentAst {
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
    const node = pending.pop();
    if (!node) continue;
    const result = copies.get(node);
    if (!result) continue;
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

function parseLiteralValue(raw: string): unknown {
  const s = raw.trim();
  if (s === 'true') return true;
  if (s === 'false') return false;
  if (s === 'null') return null;
  if (/^-?\d+(\.\d+)?$/.test(s)) return Number(s);
  if ((s.startsWith("'") && s.endsWith("'")) || (s.startsWith('"') && s.endsWith('"'))) {
    const inner = s.slice(1, -1);
    let out = '';
    for (let i = 0; i < inner.length; i++) {
      if (inner.charAt(i) === '\\' && i + 1 < inner.length) {
        const next = inner.charAt(++i);
        if (next === 'n') out += '\n';
        else if (next === 'r') out += '\r';
        else if (next === 't') out += '\t';
        else out += next;
      } else {
        out += inner.charAt(i);
      }
    }
    return out;
  }
  if (s.startsWith('[') || s.startsWith('{')) {
    try {
      const normalizedJson = s.replace(/'/g, '"').replace(/([a-zA-Z0-9_]+)\s*:/g, '"$1":');
      return JSON.parse(normalizedJson, (key, val: unknown) =>
        FORBIDDEN_MEMBER_KEYS.has(key) ? undefined : val,
      ) as unknown;
    } catch {
      return [];
    }
  }
  return s;
}

/**
 * Pre-parses a Forma `.ui` source string into a serializable `FormaDocumentAst` at build time (AOT)
 * so the browser can instantiate `compileFormaAst(ast, options)` with 0ms of runtime source parsing!
 */
export function parseFormaSource(rawUiSource: string): FormaDocumentAst {
  const cMatch = rawUiSource.match(/component\s+([A-Za-z0-9_]+)(?:\s*:\s*[A-Za-z0-9_]+)?\s*\{/);
  const componentName = cMatch?.[1] ?? 'AnonymousFormaComponent';
  const cBraceIdx =
    cMatch && cMatch.index !== undefined ? rawUiSource.indexOf('{', cMatch.index) : -1;
  const compBody = cBraceIdx !== -1 ? extractBraceBlock(rawUiSource, cBraceIdx).body : rawUiSource;
  assertFormaContractDepth(compBody);

  const tokens: Record<string, unknown> = {};
  const i18nDictionaries: Record<string, Record<string, string>> = {};
  const styles: Record<string, Record<string, string>> = {};
  const propConstraints: FormaPropConstraint[] = [];
  const initialStateValues = Object.create(null) as Record<string, unknown>;
  const letExpressions: Array<{ name: string; expr: string }> = [];
  const shortcuts: Array<{ combo: string; action: string }> = [];
  const rootAstNodes: FormaAstNode[] = [];

  let i = 0;
  const src = compBody.trim();
  const n = src.length;
  let pendingRange: [number, number] | undefined;
  let pendingPatternSource: string | undefined;

  while (i < n) {
    while (i < n && /\s/.test(src.charAt(i))) i++;
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
      if (patternMatch && patternMatch[2] !== undefined && isSafePatternRegex(patternMatch[2])) {
        const candidatePattern = patternMatch[2];
        try {
          new RegExp(candidatePattern);
          pendingPatternSource = candidatePattern;
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
    if (i18nMatch && i18nMatch[1] !== undefined) {
      const loc = i18nMatch[1];
      const bIdx = src.indexOf('{', i);
      const blk = extractBraceBlock(src, bIdx);
      i = blk.endIdx;
      const dict = (i18nDictionaries[loc] ??= {});
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
          dict[k] = v;
        }
      }
      continue;
    }

    // `style <name> { ... }`
    const styleMatch = src.slice(i).match(/^style\s+([a-zA-Z0-9_]+)\s*\{/);
    if (styleMatch && styleMatch[1] !== undefined) {
      const styleName = styleMatch[1];
      const bIdx = src.indexOf('{', i);
      const blk = extractBraceBlock(src, bIdx);
      i = blk.endIdx;
      const styleMap: Record<string, string> = {};
      styles[styleName] = styleMap;
      for (const rawLine of blk.body.split(';')) {
        const line = rawLine.trim();
        if (!line) continue;
        const cIdx = line.indexOf(':');
        if (cIdx !== -1) {
          styleMap[line.slice(0, cIdx).trim()] = line.slice(cIdx + 1).trim();
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
    if (propMatch && propMatch[2] !== undefined && propMatch[3] !== undefined) {
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
    if (stateMatch && stateMatch[1] !== undefined && stateMatch[3] !== undefined) {
      const name = stateMatch[1].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(name))
        throw new Error('Invalid Forma AST document: reserved declaration name');
      const initVal = parseLiteralValue(stateMatch[3].trim());
      initialStateValues[name] = structuredCloneIfObject(initVal);
      continue;
    }

    // `let <name> = <expr>`
    const letMatch = stmt.match(/^let\s+([a-zA-Z0-9_]+)\s*=\s*(.+)$/);
    if (letMatch && letMatch[1] !== undefined && letMatch[2] !== undefined) {
      const letName = letMatch[1].trim();
      if (FORBIDDEN_MEMBER_KEYS.has(letName))
        throw new Error('Invalid Forma AST document: reserved let name');
      letExpressions.push({ name: letName, expr: letMatch[2].trim() });
      continue;
    }

    // `shortcut '<combo>' -> <action>`
    const shortcutMatch = stmt.match(/^shortcut\s+['"](.+?)['"]\s*->\s*(.+)$/);
    if (shortcutMatch && shortcutMatch[1] !== undefined && shortcutMatch[2] !== undefined) {
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
