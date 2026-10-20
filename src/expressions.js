export {
  pureFunctions,
  tokenize,
  parseExpression,
  parseString,
  equalValues,
  evaluateExpression,
  matchesPattern,
  serializeValue,
} from '@themoretheless/tokenizer-forma/expressions';

const referenceCache = new WeakMap();
const empty = Object.freeze([]);
const dottedIdent = /^!?[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+$/;

function collectReferences(value, seen, out) {
  if (value === null || typeof value !== 'object') return;
  if (typeof value.expr === 'string') {
    const raw = value.expr;
    if (dottedIdent.test(raw)) {
      const ref = raw.charCodeAt(0) === 33 ? raw.slice(1) : raw;
      if (!seen.has(ref)) {
        seen.add(ref);
        out.push(ref);
      }
    }
  }
  if (Array.isArray(value)) {
    for (let i = 0; i < value.length; i++) {
      collectReferences(value[i], seen, out);
    }
    return;
  }
  const keys = Object.keys(value);
  for (let i = 0; i < keys.length; i++) {
    const k = keys[i];
    if (k !== 'expr') {
      collectReferences(value[k], seen, out);
    }
  }
}

export function expressionReferences(value) {
  if (value === null || typeof value !== 'object') return empty;
  let refs = referenceCache.get(value);
  if (refs === undefined) {
    const out = [];
    collectReferences(value, new Set(), out);
    refs = out.length ? Object.freeze(out) : empty;
    referenceCache.set(value, refs);
  }
  return refs;
}
