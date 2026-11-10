/**
 * # Forma `.ui` AOT Validator & ES Module Generator (`forma-aot.ts`)
 *
 * Static validation (`validateFormaSource`) and zero-runtime-parse JS module
 * generation (`compileFormaToJsModule`) for `.ui` components.
 */

import { isSafePatternRegex } from './forma-expr.ts';
import {
  FORMA_PATTERN_CONTRACT,
  formaContractSyntaxError,
  parseFormaSource,
  readFormaContract,
  type FormaDocumentAst,
} from './forma-ast.ts';

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
  const componentName = compMatch?.[1] ?? 'AnonymousFormaComponent';
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
    const ch = uiSource.charAt(i);
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
    if (ch === '/' && uiSource.charAt(i + 1) === '/') {
      const nl = uiSource.indexOf('\n', i + 2);
      if (nl === -1) break;
      lineNum++;
      i = nl;
      continue;
    }
    if (ch === '#' && uiSource.charAt(i + 1) === '[') {
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
        if (pMatch && pMatch[2] !== undefined) {
          const patternText = pMatch[2];
          if (!isSafePatternRegex(patternText)) {
            diagnostics.push({
              severity: 'error',
              message: `Небезопасное регулярное выражение в #[pattern] (возможен ReDoS): ${patternText}`,
              line: idx + 1,
            });
          } else {
            try {
              new RegExp(patternText);
            } catch (e: unknown) {
              const errMessage = e instanceof Error ? e.message : patternText;
              diagnostics.push({
                severity: 'error',
                message: `Ошибка регулярного выражения в #[pattern]: ${errMessage}`,
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
