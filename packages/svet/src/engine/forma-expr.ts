/**
 * # CSP-Safe Forma AST Expression Parser & Evaluator (`forma-expr.ts`)
 *
 * Zero `new Function`, Zero `eval`, Zero `with (__env)`:
 * - Tokenizes & parses Forma expressions (including pipe syntax `|>`, string interpolation `${...}`,
 *   array literals, member/index access, calls, unary/binary/ternary operators) into cached `ExprAst` nodes.
 * - Evaluates `ExprAst` against a reactive `FormaEvalScope`, automatically tracking WASM `ReactiveGraph` reads.
 * - Pre-compiles and caches `match` pattern matchers (`compileFormaPatternMatcher`).
 */

export {
  type FormaEvalScope,
  type InterpPart,
  type ExprAst,
  FORBIDDEN_MEMBER_KEYS,
  isSafePatternRegex,
  parseFormaExprAst,
  evalAstNode,
  isStaticFormaExprNode,
  isStaticFormaExpr,
  evaluateFormaExpression,
} from './forma-expr-parser.ts';
export {
  compileFormaPatternMatcher,
  doesFormaPatternMatch,
  type CanonicalExpressionAst,
  type CanonicalFormaValue,
  canonicalValueToExprString,
  fromCanonicalFormaExpr,
} from './forma-expr-canonical.ts';
