/**
 * # Svet Automatic JSX Expression Compiler (`compiler.ts`)
 *
 * Eliminates the need to write manual arrow thunks `{() => count()}` in JSX!
 * When developers write natural JSX expressions containing reactive signal calls:
 *   `<span>{count()}</span>` or `<div title={userName()}>{count() * 2}</div>`
 *
 * `transformSvetJsx(code)` automatically transforms reactive JSX expression containers into
 * lazy thunks `{() => count()}` before JSX compilation, while keeping event handlers
 * (`onClick={...}`), `ref={...}`, `model={...}`, `keyFn={...}`, and already-wrapped functions untouched!
 */

import { parse } from '@babel/parser';
import MagicString from 'magic-string';
import { compileFormaToJsModule } from './forma-aot.ts';

const SKIP_ATTRIBUTES = new Set(['ref', 'model', 'key', 'keyFn', 'children']);
const CONSTRUCTORS = new Set(['h', 'For', 'Show', 'Fragment', 'jsx', 'jsxs']);

const PURE_GLOBAL_FUNCTIONS = new Set([
  'String',
  'Number',
  'Boolean',
  'BigInt',
  'parseInt',
  'parseFloat',
  'isFinite',
  'isNaN',
  'encodeURI',
  'encodeURIComponent',
  'decodeURI',
  'decodeURIComponent',
]);

const PURE_GLOBAL_VALUES = new Set(['undefined', 'NaN', 'Infinity']);

const PURE_MATH_METHODS = new Set([
  'abs',
  'acos',
  'acosh',
  'asin',
  'asinh',
  'atan',
  'atan2',
  'atanh',
  'cbrt',
  'ceil',
  'clz32',
  'cos',
  'cosh',
  'exp',
  'expm1',
  'floor',
  'fround',
  'hypot',
  'imul',
  'log',
  'log10',
  'log1p',
  'log2',
  'max',
  'min',
  'pow',
  'round',
  'sign',
  'sin',
  'sinh',
  'sqrt',
  'tan',
  'tanh',
  'trunc',
]);

const PURE_MATH_CONSTANTS = new Set([
  'E',
  'LN2',
  'LN10',
  'LOG2E',
  'LOG10E',
  'PI',
  'SQRT1_2',
  'SQRT2',
]);

const PURE_NUMBER_METHODS = new Set([
  'isFinite',
  'isInteger',
  'isNaN',
  'isSafeInteger',
  'parseFloat',
  'parseInt',
]);

const PURE_NUMBER_CONSTANTS = new Set([
  'EPSILON',
  'MAX_SAFE_INTEGER',
  'MAX_VALUE',
  'MIN_SAFE_INTEGER',
  'MIN_VALUE',
  'NaN',
  'NEGATIVE_INFINITY',
  'POSITIVE_INFINITY',
]);

const PURE_STRING_NAMESPACE_METHODS = new Set(['fromCharCode', 'fromCodePoint']);
const PURE_OBJECT_INSPECTION_METHODS = new Set(['keys', 'values', 'entries']);
const PURE_STRING_INSTANCE_METHODS = new Set([
  'toUpperCase',
  'toLowerCase',
  'trim',
  'trimStart',
  'trimEnd',
  'slice',
  'substring',
  'padStart',
  'padEnd',
  'repeat',
  'charAt',
  'charCodeAt',
  'codePointAt',
  'includes',
  'startsWith',
  'endsWith',
  'indexOf',
  'lastIndexOf',
]);
const PURE_NUMBER_INSTANCE_METHODS = new Set([
  'toFixed',
  'toPrecision',
  'toExponential',
  'toString',
]);
const PURE_ARRAY_INSTANCE_METHODS = new Set(['join', 'slice', 'includes', 'indexOf', 'concat']);
const FORBIDDEN_STATIC_OBJECT_KEYS = new Set(['toJSON', 'valueOf', 'toString', '__proto__']);

const LITERAL_NODE_TYPES = new Set([
  'NumericLiteral',
  'StringLiteral',
  'BooleanLiteral',
  'NullLiteral',
  'BigIntLiteral',
  'RegExpLiteral',
]);

const PURE_UNARY_OPERATORS = new Set(['+', '-', '!', '~', 'typeof', 'void']);

type AstNode = {
  type: string;
  start?: number | null;
  end?: number | null;
  [key: string]: unknown;
};

interface LexicalScope {
  shadowed: ReadonlySet<string>;
  pureConsts: ReadonlySet<string>;
}

function isNode(value: unknown): value is AstNode {
  return !!value && typeof value === 'object' && typeof (value as AstNode).type === 'string';
}

function children(node: AstNode): AstNode[] {
  const result: AstNode[] = [];
  for (const value of Object.values(node)) {
    if (isNode(value)) result.push(value);
    else if (Array.isArray(value)) for (const child of value) if (isNode(child)) result.push(child);
  }
  return result;
}

function unwrap(node: AstNode): AstNode {
  while (
    [
      'TSAsExpression',
      'TSSatisfiesExpression',
      'TSNonNullExpression',
      'TSTypeAssertion',
      'ParenthesizedExpression',
    ].includes(node.type) &&
    isNode(node.expression)
  ) {
    node = node.expression;
  }
  return node;
}

function collectPatternNames(node: AstNode, names: Set<string>): void {
  switch (node.type) {
    case 'Identifier':
      if (typeof node.name === 'string') names.add(node.name);
      return;
    case 'AssignmentPattern':
      if (isNode(node.left)) collectPatternNames(node.left, names);
      return;
    case 'RestElement':
      if (isNode(node.argument)) collectPatternNames(node.argument, names);
      return;
    case 'ArrayPattern':
      if (Array.isArray(node.elements)) {
        for (const el of node.elements) if (isNode(el)) collectPatternNames(el, names);
      }
      return;
    case 'ObjectPattern':
      if (Array.isArray(node.properties)) {
        for (const prop of node.properties) {
          if (!isNode(prop)) continue;
          if (prop.type === 'ObjectProperty' && isNode(prop.value)) {
            collectPatternNames(prop.value, names);
          } else if (prop.type === 'RestElement' && isNode(prop.argument)) {
            collectPatternNames(prop.argument, names);
          }
        }
      }
      return;
    default:
      return;
  }
}

function collectDeclaratorNames(decl: AstNode, names: Set<string>): void {
  if (!Array.isArray(decl.declarations)) return;
  for (const declarator of decl.declarations) {
    if (isNode(declarator) && isNode(declarator.id)) {
      collectPatternNames(declarator.id, names);
    }
  }
}

function collectHoistedVars(node: AstNode, names: Set<string>): void {
  if (
    [
      'FunctionDeclaration',
      'FunctionExpression',
      'ArrowFunctionExpression',
      'ObjectMethod',
      'ClassMethod',
    ].includes(node.type)
  ) {
    return;
  }
  if (node.type === 'VariableDeclaration' && node.kind === 'var') {
    collectDeclaratorNames(node, names);
    return;
  }
  for (const child of children(node)) {
    collectHoistedVars(child, names);
  }
}

function collectBlockDeclarations(
  statements: AstNode[],
  names: Set<string>,
  isFunctionBody: boolean,
): void {
  for (const stmt of statements) {
    if (stmt.type === 'VariableDeclaration') {
      collectDeclaratorNames(stmt, names);
    } else if (
      (stmt.type === 'FunctionDeclaration' ||
        stmt.type === 'ClassDeclaration' ||
        stmt.type === 'TSEnumDeclaration') &&
      isNode(stmt.id) &&
      typeof stmt.id.name === 'string'
    ) {
      names.add(stmt.id.name);
    } else if (stmt.type === 'ImportDeclaration' && Array.isArray(stmt.specifiers)) {
      for (const spec of stmt.specifiers) {
        if (isNode(spec) && isNode(spec.local) && typeof spec.local.name === 'string') {
          names.add(spec.local.name);
        }
      }
    } else if (stmt.type === 'ExportNamedDeclaration' && isNode(stmt.declaration)) {
      collectBlockDeclarations([stmt.declaration], names, isFunctionBody);
    } else if (isFunctionBody) {
      collectHoistedVars(stmt, names);
    }
  }
}

function isPlainStaticObject(node: AstNode, scope: LexicalScope): boolean {
  const inner = unwrap(node);
  if (inner.type !== 'ObjectExpression' || !Array.isArray(inner.properties)) return false;
  for (const prop of inner.properties) {
    if (
      !isNode(prop) ||
      prop.type !== 'ObjectProperty' ||
      prop.computed === true ||
      !isNode(prop.key)
    ) {
      return false;
    }
    const keyName =
      prop.key.type === 'Identifier'
        ? String(prop.key.name)
        : prop.key.type === 'StringLiteral'
          ? String(prop.key.value)
          : prop.key.type === 'NumericLiteral'
            ? String(prop.key.value)
            : null;
    if (keyName === null || FORBIDDEN_STATIC_OBJECT_KEYS.has(keyName)) return false;
    if (!isNode(prop.value) || !isPureStaticValue(prop.value, scope)) return false;
  }
  return true;
}

function isPlainStaticArray(node: AstNode, scope: LexicalScope): boolean {
  const inner = unwrap(node);
  if (inner.type !== 'ArrayExpression' || !Array.isArray(inner.elements)) return false;
  for (const el of inner.elements) {
    if (el === null) continue;
    if (!isNode(el) || el.type === 'SpreadElement' || !isPureStaticValue(el, scope)) return false;
  }
  return true;
}

function isStaticStringExpression(node: AstNode, scope: LexicalScope): boolean {
  const inner = unwrap(node);
  if (inner.type === 'StringLiteral') return true;
  if (inner.type === 'TemplateLiteral' && Array.isArray(inner.expressions)) {
    return inner.expressions.every(expr => isNode(expr) && isPureStaticValue(expr, scope));
  }
  if (inner.type !== 'CallExpression' || inner.optional === true || !isNode(inner.callee)) {
    return false;
  }
  if (!isPureStaticCall(inner, scope)) return false;
  const callee = unwrap(inner.callee);
  if (callee.type === 'Identifier') return String(callee.name) === 'String';
  if (
    callee.type === 'MemberExpression' &&
    callee.computed !== true &&
    callee.optional !== true &&
    isNode(callee.object) &&
    isNode(callee.property) &&
    callee.property.type === 'Identifier'
  ) {
    const method = String(callee.property.name);
    const recv = unwrap(callee.object);
    if (recv.type === 'Identifier' && String(recv.name) === 'String') {
      return PURE_STRING_NAMESPACE_METHODS.has(method);
    }
    if (recv.type === 'Identifier' && String(recv.name) === 'JSON' && method === 'stringify') {
      return true;
    }
    if (isStaticStringExpression(recv, scope)) {
      return (
        method !== 'charCodeAt' &&
        method !== 'codePointAt' &&
        method !== 'includes' &&
        method !== 'startsWith' &&
        method !== 'endsWith' &&
        method !== 'indexOf' &&
        method !== 'lastIndexOf'
      );
    }
  }
  return false;
}

function isPureStaticValue(node: AstNode, scope: LexicalScope): boolean {
  const inner = unwrap(node);
  if (LITERAL_NODE_TYPES.has(inner.type)) return true;
  if (inner.type === 'Identifier') {
    const name = String(inner.name);
    return (
      (PURE_GLOBAL_VALUES.has(name) && !scope.shadowed.has(name)) || scope.pureConsts.has(name)
    );
  }
  if (inner.type === 'TemplateLiteral' && Array.isArray(inner.expressions)) {
    return inner.expressions.every(expr => isNode(expr) && isPureStaticValue(expr, scope));
  }
  if (inner.type === 'UnaryExpression') {
    return (
      typeof inner.operator === 'string' &&
      PURE_UNARY_OPERATORS.has(inner.operator) &&
      isNode(inner.argument) &&
      isPureStaticValue(inner.argument, scope)
    );
  }
  if (inner.type === 'BinaryExpression' || inner.type === 'LogicalExpression') {
    return (
      inner.operator !== 'in' &&
      inner.operator !== 'instanceof' &&
      isNode(inner.left) &&
      isNode(inner.right) &&
      isPureStaticValue(inner.left, scope) &&
      isPureStaticValue(inner.right, scope)
    );
  }
  if (inner.type === 'ConditionalExpression') {
    return (
      isNode(inner.test) &&
      isNode(inner.consequent) &&
      isNode(inner.alternate) &&
      isPureStaticValue(inner.test, scope) &&
      isPureStaticValue(inner.consequent, scope) &&
      isPureStaticValue(inner.alternate, scope)
    );
  }
  if (inner.type === 'ArrayExpression') {
    return isPlainStaticArray(inner, scope);
  }
  if (inner.type === 'ObjectExpression') {
    return isPlainStaticObject(inner, scope);
  }
  if (
    inner.type === 'MemberExpression' &&
    inner.computed !== true &&
    inner.optional !== true &&
    isNode(inner.object) &&
    isNode(inner.property) &&
    inner.property.type === 'Identifier'
  ) {
    const obj = unwrap(inner.object);
    const prop = String(inner.property.name);
    if (obj.type === 'Identifier') {
      const objName = String(obj.name);
      if (objName === 'Math' && !scope.shadowed.has('Math')) {
        return PURE_MATH_CONSTANTS.has(prop);
      }
      if (objName === 'Number' && !scope.shadowed.has('Number')) {
        return PURE_NUMBER_CONSTANTS.has(prop);
      }
    }
    if (
      prop === 'length' &&
      (isStaticStringExpression(obj, scope) || isPlainStaticArray(obj, scope))
    ) {
      return true;
    }
    return false;
  }
  if (inner.type === 'CallExpression') {
    return isPureStaticCall(inner, scope);
  }
  return false;
}

function isUndefinedLiteral(node: AstNode, scope: LexicalScope): boolean {
  const inner = unwrap(node);
  return (
    inner.type === 'NullLiteral' ||
    (inner.type === 'Identifier' &&
      String(inner.name) === 'undefined' &&
      !scope.shadowed.has('undefined'))
  );
}

function isPureStaticCall(node: AstNode, scope: LexicalScope): boolean {
  if (node.type !== 'CallExpression' || node.optional === true || !isNode(node.callee)) {
    return false;
  }
  if (!Array.isArray(node.arguments)) return false;
  const args: AstNode[] = [];
  for (const arg of node.arguments) {
    if (!isNode(arg) || arg.type === 'SpreadElement' || !isPureStaticValue(arg, scope)) {
      return false;
    }
    args.push(arg);
  }
  const callee = unwrap(node.callee);
  if (callee.type === 'Identifier') {
    const name = String(callee.name);
    return !scope.shadowed.has(name) && PURE_GLOBAL_FUNCTIONS.has(name);
  }
  if (
    callee.type !== 'MemberExpression' ||
    callee.computed === true ||
    callee.optional === true ||
    !isNode(callee.object) ||
    !isNode(callee.property) ||
    callee.property.type !== 'Identifier'
  ) {
    return false;
  }
  const method = String(callee.property.name);
  const receiver = unwrap(callee.object);
  if (receiver.type === 'Identifier') {
    const ns = String(receiver.name);
    if (!scope.shadowed.has(ns)) {
      if (ns === 'Math') return PURE_MATH_METHODS.has(method);
      if (ns === 'Number') return PURE_NUMBER_METHODS.has(method);
      if (ns === 'String') return PURE_STRING_NAMESPACE_METHODS.has(method);
      if (ns === 'Array') return method === 'isArray';
      if (ns === 'Object' && PURE_OBJECT_INSPECTION_METHODS.has(method)) {
        return (
          args.length === 1 &&
          (isPlainStaticObject(args[0]!, scope) || isPlainStaticArray(args[0]!, scope))
        );
      }
      if (ns === 'JSON') {
        if (method === 'parse') return args.length === 1;
        if (method === 'stringify') {
          return (
            args.length === 1 ||
            ((args.length === 2 || args.length === 3) && isUndefinedLiteral(args[1]!, scope))
          );
        }
      }
    }
  }
  if (isStaticStringExpression(receiver, scope)) {
    if (PURE_STRING_INSTANCE_METHODS.has(method)) return true;
    if (method === 'split')
      return args.length <= 2 && (args.length === 0 || unwrap(args[0]!).type === 'StringLiteral');
    if (method === 'replace' || method === 'replaceAll') {
      return (
        args.length === 2 &&
        unwrap(args[0]!).type === 'StringLiteral' &&
        unwrap(args[1]!).type === 'StringLiteral'
      );
    }
    return false;
  }
  if (receiver.type === 'NumericLiteral') {
    return PURE_NUMBER_INSTANCE_METHODS.has(method);
  }
  if (isPlainStaticArray(receiver, scope)) {
    return PURE_ARRAY_INSTANCE_METHODS.has(method);
  }
  return false;
}

function extendScopeWithStatements(
  statements: AstNode[],
  parent: LexicalScope,
  isFunctionBody: boolean,
): LexicalScope {
  const declared = new Set<string>();
  collectBlockDeclarations(statements, declared, isFunctionBody);
  const shadowed = new Set(parent.shadowed);
  const pureConsts = new Set(parent.pureConsts);
  for (const name of declared) {
    shadowed.add(name);
    pureConsts.delete(name);
  }
  const blockScope: LexicalScope = { shadowed, pureConsts };
  for (const stmt of statements) {
    const decl =
      stmt.type === 'ExportNamedDeclaration' && isNode(stmt.declaration) ? stmt.declaration : stmt;
    if (
      decl.type !== 'VariableDeclaration' ||
      decl.kind !== 'const' ||
      !Array.isArray(decl.declarations)
    ) {
      continue;
    }
    for (const declarator of decl.declarations) {
      if (
        isNode(declarator) &&
        isNode(declarator.id) &&
        declarator.id.type === 'Identifier' &&
        typeof declarator.id.name === 'string' &&
        isNode(declarator.init) &&
        isPureStaticValue(declarator.init, blockScope)
      ) {
        pureConsts.add(declarator.id.name);
      }
    }
  }
  return blockScope;
}

function childScopeForNode(node: AstNode, scope: LexicalScope): LexicalScope {
  if (node.type === 'Program' && Array.isArray(node.body)) {
    return extendScopeWithStatements(node.body.filter(isNode), scope, true);
  }
  if (node.type === 'BlockStatement' && Array.isArray(node.body)) {
    return extendScopeWithStatements(node.body.filter(isNode), scope, false);
  }
  if (
    [
      'FunctionDeclaration',
      'FunctionExpression',
      'ArrowFunctionExpression',
      'ObjectMethod',
      'ClassMethod',
    ].includes(node.type)
  ) {
    const names = new Set<string>();
    if (node.type === 'FunctionExpression' && isNode(node.id) && typeof node.id.name === 'string') {
      names.add(node.id.name);
    }
    if (Array.isArray(node.params)) {
      for (const param of node.params) if (isNode(param)) collectPatternNames(param, names);
    }
    const shadowed = new Set(scope.shadowed);
    const pureConsts = new Set(scope.pureConsts);
    for (const name of names) {
      shadowed.add(name);
      pureConsts.delete(name);
    }
    const paramScope: LexicalScope = { shadowed, pureConsts };
    if (isNode(node.body) && node.body.type === 'BlockStatement' && Array.isArray(node.body.body)) {
      return extendScopeWithStatements(node.body.body.filter(isNode), paramScope, true);
    }
    return paramScope;
  }
  if (node.type === 'CatchClause' && isNode(node.param)) {
    const names = new Set<string>();
    collectPatternNames(node.param, names);
    const shadowed = new Set(scope.shadowed);
    const pureConsts = new Set(scope.pureConsts);
    for (const name of names) {
      shadowed.add(name);
      pureConsts.delete(name);
    }
    return { shadowed, pureConsts };
  }
  if (
    (node.type === 'ForStatement' ||
      node.type === 'ForInStatement' ||
      node.type === 'ForOfStatement') &&
    isNode(node.init ?? node.left)
  ) {
    const bindingNode = (node.init ?? node.left) as AstNode;
    if (bindingNode.type === 'VariableDeclaration' && bindingNode.kind !== 'var') {
      const names = new Set<string>();
      collectDeclaratorNames(bindingNode, names);
      const shadowed = new Set(scope.shadowed);
      const pureConsts = new Set(scope.pureConsts);
      for (const name of names) {
        shadowed.add(name);
        pureConsts.delete(name);
      }
      return { shadowed, pureConsts };
    }
  }
  return scope;
}

function hasEagerCall(node: AstNode, scope: LexicalScope): boolean {
  node = unwrap(node);
  // Calls in callbacks and nested JSX are evaluated by their own runtime scopes.
  if (['ObjectMethod', 'ClassMethod'].includes(node.type)) {
    return node.computed === true && isNode(node.key) && hasEagerCall(node.key, scope);
  }
  if (
    node.type.startsWith('JSX') ||
    node.type.startsWith('TS') ||
    ['ArrowFunctionExpression', 'FunctionExpression', 'FunctionDeclaration'].includes(node.type)
  )
    return false;
  if (node.type === 'CallExpression') {
    return !isPureStaticCall(node, scope);
  }
  if (['OptionalCallExpression', 'NewExpression', 'TaggedTemplateExpression'].includes(node.type))
    return true;
  return children(node).some(child => hasEagerCall(child, scope));
}

function shouldWrapExpression(
  expression: AstNode,
  parent: AstNode | null,
  scope: LexicalScope,
): boolean {
  const node = unwrap(expression);
  if (parent?.type === 'JSXAttribute' && isNode(parent.name)) {
    const name = parent.name.name;
    if (
      typeof name === 'string' &&
      (SKIP_ATTRIBUTES.has(name) || (name.startsWith('on') && name.length > 2))
    )
      return false;
  }
  if (
    ['CallExpression', 'OptionalCallExpression'].includes(node.type) &&
    isNode(node.callee) &&
    node.callee.type === 'Identifier' &&
    CONSTRUCTORS.has(String(node.callee.name))
  )
    return false;
  return hasEagerCall(node, scope);
}

export class SvetJsxCompileError extends SyntaxError {
  readonly filename: string;
  readonly line: number;
  readonly column: number;

  constructor(message: string, filename: string, line: number, column: number) {
    super(`${filename}:${line}:${column}: ${message}`);
    this.name = 'SvetJsxCompileError';
    this.filename = filename;
    this.line = line;
    this.column = column;
  }
}

/** Parses TSX/JSX and inserts reactive thunks while preserving source locations. */
export function transformSvetJsxWithMap(source: string, filename = 'component.tsx') {
  const output = new MagicString(source);
  if (!source.includes('<')) {
    return {
      code: source,
      map: output.generateMap({ source: filename, includeContent: true, hires: true }),
    };
  }
  let ast: AstNode;
  try {
    ast = parse(source, {
      sourceType: 'unambiguous',
      plugins: ['jsx', 'typescript'],
      allowAwaitOutsideFunction: true,
      allowReturnOutsideFunction: true,
      sourceFilename: filename,
    }) as unknown as AstNode;
  } catch (error) {
    const diagnostic = error as { message: string; loc?: { line: number; column: number } };
    throw new SvetJsxCompileError(
      diagnostic.message,
      filename,
      diagnostic.loc?.line ?? 1,
      (diagnostic.loc?.column ?? 0) + 1,
    );
  }
  const visit = (node: AstNode, parent: AstNode | null, scope: LexicalScope) => {
    const nextScope = childScopeForNode(node, scope);
    if (
      node.type === 'JSXExpressionContainer' &&
      isNode(node.expression) &&
      shouldWrapExpression(node.expression, parent, nextScope)
    ) {
      const expression = node.expression;
      if (expression.start != null && expression.end != null) {
        output.appendLeft(expression.start, '() => (');
        output.appendLeft(expression.end, ')');
      }
    }
    if (
      node.type === 'BlockStatement' &&
      parent !== null &&
      [
        'FunctionDeclaration',
        'FunctionExpression',
        'ArrowFunctionExpression',
        'ObjectMethod',
        'ClassMethod',
      ].includes(parent.type)
    ) {
      for (const child of children(node)) visit(child, node, scope);
      return;
    }
    for (const child of children(node)) visit(child, node, nextScope);
  };
  visit(ast, null, { shadowed: new Set(), pureConsts: new Set() });
  return {
    code: output.toString(),
    map: output.generateMap({ source: filename, includeContent: true, hires: true }),
  };
}

/** Automatically makes eagerly called JSX expressions reactive. */
export function transformSvetJsx(source: string): string {
  return transformSvetJsxWithMap(source).code;
}

export interface SvetVitePluginOptions {
  /** Runtime package used by generated Forma modules. Use `svet-engine/slim` together with the slim entry. */
  runtimeImportSource?: string;
  /**
   * Keep the `.ui` text in generated modules as `uiSource`. Defaults to `true` while serving and
   * `false` for `vite build`, where only the pre-parsed AST is needed.
   */
  embedFormaSource?: boolean;
}

// Structural adapter types keep consumers of the runtime declarations independent
// of Vite. Module identities flow through the generic HMR hook unchanged.
interface SvetHotUpdateContext<TModule> {
  file: string;
  read(): string | Promise<string>;
  server: {
    moduleGraph: { getModulesByFile(file: string): Set<TModule> | undefined };
    ws: { send(payload: { type: 'custom'; event: string; data: unknown }): void };
  };
}

/**
 * Vite plugin that enables zero-boilerplate reactive JSX expressions (`<span>{count()}</span>`)
 * and AOT + HMR compilation of Forma `.ui` files.
 */
export function svetVitePlugin(options: SvetVitePluginOptions = {}) {
  let production = false;
  return {
    name: 'vite-plugin-svet-jsx',
    enforce: 'pre' as const,
    configResolved(config: { command?: string }) {
      production = config.command === 'build';
    },
    handleHotUpdate<TModule>({ file, server, read }: SvetHotUpdateContext<TModule>) {
      if (file.endsWith('.ui')) {
        const mods = server.moduleGraph.getModulesByFile(file);
        if (mods && mods.size > 0) {
          return Array.from(mods);
        }
        Promise.resolve(read())
          .then((uiSource: string) => {
            server.ws.send({
              type: 'custom',
              event: 'svet:forma-ui-update',
              data: { file, uiSource },
            });
          })
          .catch(() => {});
        return [];
      }
    },
    transform(code: string, id: string) {
      const cleanId = id.split('?')[0]!;
      // 1. Direct import of Forma `.ui` files: `import CustomerCardUi from './CustomerCard.ui'`
      // Pre-parses `uiAst` and `uiValidation` at build time (True AOT — 0ms runtime source parsing!)
      if (cleanId.endsWith('.ui')) {
        return {
          code: compileFormaToJsModule(code, options.runtimeImportSource ?? 'svet-engine', {
            embedSource: options.embedFormaSource ?? !production,
          }),
          map: null,
        };
      }

      // 2. Auto-Reactive JSX Expression transformation for `.tsx` / `.jsx`
      if (cleanId.endsWith('.tsx') || cleanId.endsWith('.jsx')) {
        const transformed = transformSvetJsxWithMap(code, cleanId);
        if (transformed.code !== code) return transformed;
      }
      return null;
    },
  };
}
