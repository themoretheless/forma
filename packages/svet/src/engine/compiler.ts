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

import type { Plugin } from 'vite';
import { compileFormaToJsModule } from './forma.ts';

const SKIP_ATTRIBUTES = new Set([
  'ref',
  'model',
  'key',
  'keyFn',
  'children',
]);

/**
 * Transforms JSX expression containers `{expr}` that contain reactive calls `ident()`
 * into `{() => expr}` automatically.
 */
export function transformSvetJsx(source: string): string {
  // Only process files that likely contain JSX tags
  if (!source.includes('<') || !source.includes('{')) {
    return source;
  }

  let out = '';
  let i = 0;
  const n = source.length;

  while (i < n) {
    const ch = source[i];

    // Skip single-line comments
    if (ch === '/' && source[i + 1] === '/') {
      const end = source.indexOf('\n', i + 2);
      if (end === -1) {
        out += source.slice(i);
        break;
      }
      out += source.slice(i, end + 1);
      i = end + 1;
      continue;
    }

    // Skip multi-line comments
    if (ch === '/' && source[i + 1] === '*') {
      const end = source.indexOf('*/', i + 2);
      if (end === -1) {
        out += source.slice(i);
        break;
      }
      out += source.slice(i, end + 2);
      i = end + 2;
      continue;
    }

    // Skip string literals (`'...'`, `"..."`, `` `...` ``) outside JSX expression containers
    if (ch === "'" || ch === '"' || ch === '`') {
      const quote = ch;
      out += ch;
      i++;
      while (i < n && source[i] !== quote) {
        if (source[i] === '\\' && i + 1 < n) {
          out += source[i] + source[i + 1];
          i += 2;
          continue;
        }
        out += source[i];
        i++;
      }
      if (i < n) {
        out += source[i];
        i++;
      }
      continue;
    }

    // Detect JSX expression container `>{...}` (child expression) or `attr={...}` (attribute expression)
    if (ch === '{') {
      const prevContext = getPrecedingJsxContext(out);
      if (prevContext.isJsxContainer) {
        const balanced = extractBalancedBraces(source, i);
        if (balanced) {
          const inner = balanced.content.trim();
          const recursivelyTransformed = transformSvetJsx(inner);
          if (shouldWrapExpression(recursivelyTransformed, prevContext.attrName)) {
            out += `{() => (${recursivelyTransformed})}`;
          } else {
            out += `{${recursivelyTransformed}}`;
          }
          i = balanced.endIndex;
          continue;
        }
      }
    }

    out += ch;
    i++;
  }

  return out;
}

function getPrecedingJsxContext(prefix: string): {
  isJsxContainer: boolean;
  attrName: string | null;
} {
  let idx = prefix.length - 1;
  while (idx >= 0 && /\s/.test(prefix[idx])) {
    idx--;
  }
  if (idx < 0) return { isJsxContainer: false, attrName: null };

  // Case 1: Attribute value `attrName={...}`
  if (prefix[idx] === '=' && idx > 0 && prefix[idx - 1] !== '=' && prefix[idx - 1] !== '!' && prefix[idx - 1] !== '<' && prefix[idx - 1] !== '>') {
    let nameEnd = idx - 1;
    while (nameEnd >= 0 && /\s/.test(prefix[nameEnd])) nameEnd--;
    let nameStart = nameEnd;
    while (nameStart >= 0 && /[a-zA-Z0-9_$-]/.test(prefix[nameStart])) nameStart--;
    const attrName = prefix.slice(nameStart + 1, nameEnd + 1);
    if (attrName.length > 0 && !['const', 'let', 'var', 'type'].includes(attrName)) {
      // Check if there is an unclosed `<Tag` before this attribute
      const lastOpenTag = prefix.lastIndexOf('<', nameStart);
      const lastCloseTag = prefix.lastIndexOf('>', nameStart);
      if (lastOpenTag > lastCloseTag) {
        return { isJsxContainer: true, attrName };
      }
    }
  }

  // Case 2: Child inside JSX element `> ... {expr}`
  const lastGt = prefix.lastIndexOf('>');
  if (lastGt !== -1 && prefix[lastGt - 1] !== '=') {
    const between = prefix.slice(lastGt + 1);
    // If between `>` and `{` there are no statement terminators `;` or `{` or `}`
    if (!/[;{}]/.test(between)) {
      return { isJsxContainer: true, attrName: null };
    }
  }

  return { isJsxContainer: false, attrName: null };
}

function extractBalancedBraces(
  source: string,
  startBraceIdx: number
): { content: string; endIndex: number } | null {
  let depth = 0;
  let i = startBraceIdx;
  const n = source.length;

  while (i < n) {
    const ch = source[i];
    if (ch === "'" || ch === '"' || ch === '`') {
      const quote = ch;
      i++;
      while (i < n && source[i] !== quote) {
        if (source[i] === '\\') i++;
        i++;
      }
      i++;
      continue;
    }
    if (ch === '{') {
      depth++;
    } else if (ch === '}') {
      depth--;
      if (depth === 0) {
        return {
          content: source.slice(startBraceIdx + 1, i),
          endIndex: i + 1,
        };
      }
    }
    i++;
  }
  return null;
}

function shouldWrapExpression(expr: string, attrName: string | null): boolean {
  if (!expr) return false;

  // Skip event handlers (`onClick`, `onInput`, etc.) and static binding attributes
  if (attrName) {
    if (attrName.startsWith('on') && attrName.length > 2) return false;
    if (SKIP_ATTRIBUTES.has(attrName)) return false;
  }

  // Already an arrow function or function expression
  if (/^(\([^)]*\)|[a-zA-Z_$][a-zA-Z0-9_$]*)\s*=>/.test(expr)) return false;
  if (expr.startsWith('function')) return false;

  // Spread `{...props}` or object literal `{{ color: 'red' }}`
  if (expr.startsWith('...') || expr.startsWith('{')) return false;

  // Contains at least one function/signal call `ident(...)`
  if (/[a-zA-Z_$][a-zA-Z0-9_$]*\s*\(/.test(expr)) {
    // Do not wrap component/DOM constructors like `h(...)` or `For(...)` or `Show(...)`
    if (/^(h|For|Show|Fragment|jsx|jsxs)\s*\(/.test(expr)) return false;
    return true;
  }

  return false;
}

/**
 * Vite plugin that enables zero-boilerplate reactive JSX expressions (`<span>{count()}</span>`)
 * and AOT + HMR compilation of Forma `.ui` files.
 */
export function svetVitePlugin(): Plugin {
  return {
    name: 'vite-plugin-svet-jsx',
    enforce: 'pre',
    handleHotUpdate({ file, server, read }) {
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
    transform(code, id) {
      const cleanId = id.split('?')[0];
      // 1. Direct import of Forma `.ui` files: `import CustomerCardUi from './CustomerCard.ui'`
      // Pre-parses `uiAst` and `uiValidation` at build time (True AOT — 0ms runtime source parsing!)
      if (cleanId.endsWith('.ui')) {
        return {
          code: compileFormaToJsModule(code, 'svet'),
          map: null,
        };
      }

      // 2. Auto-Reactive JSX Expression transformation for `.tsx` / `.jsx`
      if (cleanId.endsWith('.tsx') || cleanId.endsWith('.jsx')) {
        const transformed = transformSvetJsx(code);
        if (transformed !== code) {
          return {
            code: transformed,
            map: null,
          };
        }
      }
      return null;
    },
  };
}
