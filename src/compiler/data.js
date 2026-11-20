// @ts-check
// Plain-data helpers of the component compiler.

// Compiling clones a cached parse and the linked definitions on every run, and
// those graphs are plain object/array/scalar data. Copying them recursively with
// an identity map keeps shared subgraphs shared, the way structuredClone did,
// without paying the structured-clone setup on the hot path.
const isPlain = value => {
  if (value === null || typeof value !== 'object') return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
};
function copyData(value, seen) {
  if (value === null || typeof value !== 'object') return value;
  const known = seen.get(value);
  if (known !== undefined) return known;
  if (Array.isArray(value)) {
    const out = [];
    seen.set(value, out);
    for (const item of value) out.push(copyData(item, seen));
    return out;
  }
  if (!isPlain(value)) return structuredClone(value);
  const out = {};
  seen.set(value, out);
  for (const key of Object.keys(value)) out[key] = copyData(value[key], seen);
  return out;
}
export const clone = value =>
  typeof value === 'object' && value !== null ? copyData(value, new WeakMap()) : value;
/** Calls `fn` for every node of the tree, including the branches of control flow. */
export function visit(nodes, fn) {
  for (const n of nodes) {
    fn(n);
    visit(n.children ?? [], fn);
    visit(n.elseChildren ?? [], fn);
    visit(n.emptyChildren ?? [], fn);
    if (n.branches) for (const b of n.branches) visit(b.children ?? [], fn);
  }
}
