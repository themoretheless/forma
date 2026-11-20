import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const packageRoot = fileURLToPath(new URL('..', import.meta.url));
const distDir = `${packageRoot}dist/package`;

/** Root identifier of a call target: `a` for `a()`, `a.b.c()` and `a[0]()`. */
function calleeRoot(expression: ts.Expression): string | undefined {
  let node = expression;
  while (ts.isPropertyAccessExpression(node) || ts.isElementAccessExpression(node)) {
    node = node.expression;
  }
  return ts.isIdentifier(node) ? node.text : undefined;
}

/**
 * Modules a bundler must keep even when none of their exports is used: the targets of bare
 * imports, and modules whose top level calls into another module (a registration). A top-level
 * statement that only touches the module's own state is not observable once the module is gone.
 */
function modulesThatMustRun(): string[] {
  const found = new Set<string>();
  for (const file of readdirSync(distDir).filter(name => name.endsWith('.js'))) {
    const source = ts.createSourceFile(
      file,
      readFileSync(`${distDir}/${file}`, 'utf8'),
      ts.ScriptTarget.Latest,
      true,
    );
    const imported = new Set<string>();
    for (const statement of source.statements) {
      if (ts.isImportDeclaration(statement)) {
        const clause = statement.importClause;
        const specifier = statement.moduleSpecifier;
        if (!clause) {
          if (ts.isStringLiteral(specifier) && specifier.text.startsWith('./')) {
            found.add(specifier.text.slice(2));
          }
          continue;
        }
        if (clause.name) imported.add(clause.name.text);
        const bindings = clause.namedBindings;
        if (bindings && ts.isNamespaceImport(bindings)) imported.add(bindings.name.text);
        if (bindings && ts.isNamedImports(bindings)) {
          for (const element of bindings.elements) imported.add(element.name.text);
        }
        continue;
      }
      if (!ts.isExpressionStatement(statement) || !ts.isCallExpression(statement.expression)) {
        continue;
      }
      const root = calleeRoot(statement.expression.expression);
      if (root !== undefined && imported.has(root)) found.add(file);
    }
  }
  return [...found].sort();
}

describe('package sideEffects', () => {
  it('lists every module that registers something when imported', () => {
    const manifest = JSON.parse(readFileSync(`${packageRoot}package.json`, 'utf8')) as {
      sideEffects: string[];
    };
    const listed = new Set(manifest.sideEffects);
    const required = modulesThatMustRun();

    expect(required).toContain('wasm-embedded.js');
    expect(required).toContain('forma-3d.js');
    expect(required.filter(file => !listed.has(`./dist/package/${file}`))).toEqual([]);
    expect(
      required.filter(file => !listed.has(`./src/engine/${file.replace(/\.js$/, '.ts')}`)),
    ).toEqual([]);
  });
});
