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
import { compileFormaToJsModule } from './forma.ts';

const SKIP_ATTRIBUTES = new Set(['ref', 'model', 'key', 'keyFn', 'children']);
const CONSTRUCTORS = new Set(['h', 'For', 'Show', 'Fragment', 'jsx', 'jsxs']);

type AstNode = {
  type: string;
  start?: number | null;
  end?: number | null;
  [key: string]: unknown;
};

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
  while (['TSAsExpression', 'TSSatisfiesExpression', 'TSNonNullExpression', 'TSTypeAssertion', 'ParenthesizedExpression'].includes(node.type) && isNode(node.expression)) {
    node = node.expression;
  }
  return node;
}

function hasEagerCall(node: AstNode): boolean {
  node = unwrap(node);
  // Calls in callbacks and nested JSX are evaluated by their own runtime scopes.
  if (['ObjectMethod', 'ClassMethod'].includes(node.type)) {
    return node.computed === true && isNode(node.key) && hasEagerCall(node.key);
  }
  if (node.type.startsWith('JSX') || node.type.startsWith('TS') ||
      ['ArrowFunctionExpression', 'FunctionExpression', 'FunctionDeclaration'].includes(node.type)) return false;
  if (['CallExpression', 'OptionalCallExpression', 'NewExpression', 'TaggedTemplateExpression'].includes(node.type)) return true;
  return children(node).some(hasEagerCall);
}

function shouldWrapExpression(expression: AstNode, parent: AstNode | null): boolean {
  const node = unwrap(expression);
  if (parent?.type === 'JSXAttribute' && isNode(parent.name)) {
    const name = parent.name.name;
    if (typeof name === 'string' && (SKIP_ATTRIBUTES.has(name) || (name.startsWith('on') && name.length > 2))) return false;
  }
  if (['CallExpression', 'OptionalCallExpression'].includes(node.type) && isNode(node.callee) &&
      node.callee.type === 'Identifier' && CONSTRUCTORS.has(String(node.callee.name))) return false;
  return hasEagerCall(node);
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
    return { code: source, map: output.generateMap({ source: filename, includeContent: true, hires: true }) };
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
    throw new SvetJsxCompileError(diagnostic.message, filename, diagnostic.loc?.line ?? 1, (diagnostic.loc?.column ?? 0) + 1);
  }
  const visit = (node: AstNode, parent: AstNode | null) => {
    if (node.type === 'JSXExpressionContainer' && isNode(node.expression) &&
        shouldWrapExpression(node.expression, parent)) {
      const expression = node.expression;
      if (expression.start != null && expression.end != null) {
        output.appendLeft(expression.start, '() => (');
        output.appendLeft(expression.end, ')');
      }
    }
    for (const child of children(node)) visit(child, node);
  };
  visit(ast, null);
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
  /** Runtime package used by generated Forma modules. */
  runtimeImportSource?: string;
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
  return {
    name: 'vite-plugin-svet-jsx',
    enforce: 'pre' as const,
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
      const cleanId = id.split('?')[0];
      // 1. Direct import of Forma `.ui` files: `import CustomerCardUi from './CustomerCard.ui'`
      // Pre-parses `uiAst` and `uiValidation` at build time (True AOT — 0ms runtime source parsing!)
      if (cleanId.endsWith('.ui')) {
        return {
          code: compileFormaToJsModule(code, options.runtimeImportSource ?? 'svet-engine'),
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
