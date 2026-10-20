import { expressionReferences } from './expressions.js';
export { displayProperty } from '@themoretheless/tokenizer-forma/property-origins';

const noOrigins = Object.freeze([]);
const declarationBySource = new WeakMap();
const declarationByRef = new Map();
const originKeys = new WeakMap();
const has = (origin, key) => Object.hasOwn(origin, key);
const sourceKeys = new WeakMap();

function sourceKey(source) {
  if (source === null || typeof source !== 'object') return JSON.stringify(source) ?? 'null';
  const known = sourceKeys.get(source);
  if (known !== undefined) return known;
  const result = JSON.stringify(source) ?? 'null';
  sourceKeys.set(source, result);
  return result;
}
function isCacheable(value) {
  return value !== null && (typeof value === 'object' || typeof value === 'function');
}
function declaration(source) {
  if (!isCacheable(source)) return { source, label: source?.label ?? 'Объявление' };
  const known = declarationBySource.get(source);
  if (known !== undefined) return known;
  const result = Object.freeze({ source, label: source.label ?? 'Объявление' });
  declarationBySource.set(source, result);
  return result;
}
function referenceOrigin(ref) {
  const known = declarationByRef.get(ref);
  if (known !== undefined) return known;
  if (declarationByRef.size >= 4096) declarationByRef.clear();
  const result = Object.freeze({ label: ref });
  declarationByRef.set(ref, result);
  return result;
}
function originKey(origin) {
  const known = originKeys.get(origin);
  if (known !== undefined) return known;
  const result = JSON.stringify([
    has(origin, 'source') ? sourceKey(origin.source) : null,
    has(origin, 'label') ? origin.label : null,
  ]);
  originKeys.set(origin, result);
  return result;
}
const originsBySource = new WeakMap();
const originsByValue = new WeakMap();
function singleOrigin(source) {
  if (!isCacheable(source)) return Object.freeze([declaration(source)]);
  const known = originsBySource.get(source);
  if (known !== undefined) return known;
  const result = Object.freeze([declaration(source)]);
  originsBySource.set(source, result);
  return result;
}
function singleValueOrigin(value, source) {
  const known = originsByValue.get(value);
  if (known !== undefined && known[0] === source) return known[1];
  const result = Object.freeze([declaration(source)]);
  originsByValue.set(value, [source, result]);
  return result;
}
const soleOrigin = (value, source) =>
  isCacheable(source) ? singleValueOrigin(value, source) : singleOrigin(source);

export function propertyOrigins(value, source, props = {}, sources = {}, traces = {}, seen) {
  if (value === null || typeof value !== 'object') return source ? singleOrigin(source) : noOrigins;
  const references = expressionReferences(value);
  if (references.length === 0) return source ? soleOrigin(value, source) : noOrigins;
  const result = source ? [declaration(source)] : [];
  for (let i = 0; i < references.length; i++) {
    const ref = references[i];
    if (ref.startsWith('props.')) {
      const key = ref.slice(6);
      if (seen?.has(key)) continue;
      if (traces[key]) {
        result.push(...traces[key]);
        continue;
      }
      seen ??= new Set();
      seen.add(key);
      try {
        result.push(...propertyOrigins(props[key], sources[key], props, sources, traces, seen));
      } finally {
        seen.delete(key);
      }
    } else result.push(referenceOrigin(ref));
  }
  if (result.length < 2) return result;
  const unique = [];
  const keys = new Set();
  for (let i = 0; i < result.length; i++) {
    const origin = result[i];
    const key = originKey(origin);
    if (keys.has(key)) continue;
    keys.add(key);
    unique.push(origin);
  }
  return unique;
}
