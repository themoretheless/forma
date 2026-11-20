// Source locations are UTF-16 offsets, matching the editor (not Rust byte offsets).
// Attach before linking so inherited nodes keep their defining file.
export function attachSources(value, file) {
  if (!value || typeof value !== 'object') return value;
  for (const child of Object.values(value)) attachSources(child, file);
  if (value.propertyRanges)
    value.propertySources = Object.fromEntries(
      Object.entries(value.propertyRanges).map(([key, range]) => [key, { file, ...range }]),
    );
  if (value.type && Number.isInteger(value.start) && Number.isInteger(value.end)) {
    value.source = { file, from: value.start, to: value.end };
    value.propertySources = Object.fromEntries(
      Object.entries(value.propertyRanges ?? {}).map(([key, range]) => [key, { file, ...range }]),
    );
  }
  return value;
}

// IDs are local to this immutable compilation snapshot. A user key is metadata,
// never an array index or a globally unique ID. Reconciliation is a separate job.
// Most of a compilation's tree values are empty maps — no events, no bindings, no
// property sources — and every copy of one would be a frozen allocation that no
// reader can distinguish from its neighbours.
const emptyObject = Object.freeze({});
export function stampStableIds(nodes, file = '') {
  const prefix = file ? `${file}::` : '';
  function walk(list, parentPath) {
    const counts = new Map();
    for (const node of list ?? []) {
      const explicitKey =
        typeof node.props?.key === 'string' && node.props.key.length ? node.props.key : null;
      let path = parentPath;
      if (explicitKey) {
        node.stableId = `key:${explicitKey}`;
        path = parentPath ? `${parentPath}/key:${explicitKey}` : `${prefix}key:${explicitKey}`;
      } else {
        const ord = counts.get(node.type) ?? 0;
        counts.set(node.type, ord + 1);
        const seg = `${node.type}#${ord}`;
        path = parentPath ? `${parentPath}/${seg}` : `${prefix}${seg}`;
        node.stableId = path;
      }
      walk(node.children, path);
      walk(node.elseChildren, `${path}/else`);
      walk(node.emptyChildren, `${path}/empty`);
      if (Array.isArray(node.branches)) {
        node.branches.forEach((branch, index) => walk(branch.children, `${path}/branch#${index}`));
      }
    }
  }
  walk(nodes, '');
  return nodes;
}

const emptyArray = Object.freeze([]);
export function createElementTree(roots, file = '') {
  const prefix = file ? `${file}::` : '';
  const nodes = [];
  // Nodes of repeated instances share their definition values, so each distinct
  // value is copied once and reused. Copying and freezing in one pass keeps the
  // snapshot detached without the per-node structured-clone setup.
  const copies = new WeakMap();
  function copy(value) {
    if (value === null || typeof value !== 'object') return value;
    const known = copies.get(value);
    if (known !== undefined) return known;
    if (Array.isArray(value)) {
      if (!value.length) return emptyArray;
      const out = [];
      copies.set(value, out);
      for (let index = 0; index < value.length; index++) out[index] = copy(value[index]);
      return Object.freeze(out);
    }
    const names = Object.keys(value);
    if (!names.length) return emptyObject;
    const out = {};
    copies.set(value, out);
    for (const name of names) out[name] = copy(value[name]);
    return Object.freeze(out);
  }
  function add(element, parent, parentPath, counts) {
    const id = nodes.length;
    const explicitKey =
      typeof element.props?.key === 'string' && element.props.key.length ? element.props.key : null;
    let slotPath = parentPath;
    let stableId = element.stableId;
    if (explicitKey) {
      slotPath = parentPath ? `${parentPath}/key:${explicitKey}` : `${prefix}key:${explicitKey}`;
      stableId ??= `key:${explicitKey}`;
    } else {
      const ord = counts.get(element.type) ?? 0;
      counts.set(element.type, ord + 1);
      const seg = `${element.type}#${ord}`;
      slotPath = parentPath ? `${parentPath}/${seg}` : `${prefix}${seg}`;
      stableId ??= slotPath;
    }
    const node = {
      id,
      stableId,
      parent,
      type: element.type,
      key: element.props?.key ?? null,
      source: element.source ? copy(element.source) : null,
      propertySources: copy(element.propertySources ?? {}),
      props: copy(element.props ?? {}),
      events: copy(element.events ?? {}),
      bindings: copy(element.bindings ?? {}),
      children: [],
    };
    nodes.push(node);
    const childCounts = new Map();
    node.children = Object.freeze(
      (element.children ?? []).map(child => add(child, id, slotPath, childCounts)),
    );
    Object.freeze(node);
    return id;
  }
  const rootCounts = new Map();
  const rootIds = roots.map(root => add(root, null, '', rootCounts));
  return Object.freeze({ roots: Object.freeze(rootIds), nodes: Object.freeze(nodes) });
}

// The node a source offset names, depth-first; offsets are unique within one file's tree.
export function findNodeByStart(nodes, start) {
  for (const node of nodes ?? []) {
    if (node.start === start) return node;
    const found =
      findNodeByStart(node.children, start) ??
      findNodeByStart(node.elseChildren, start) ??
      findNodeByStart(node.emptyChildren, start) ??
      (Array.isArray(node.branches)
        ? (node.branches.map(b => findNodeByStart(b.children, start)).find(Boolean) ?? null)
        : null);
    if (found) return found;
  }
  return null;
}

export function findNodeByStableId(nodes, stableId) {
  if (!stableId) return null;
  for (const node of nodes ?? []) {
    if (node.stableId === stableId) return node;
    const found =
      findNodeByStableId(node.children, stableId) ??
      findNodeByStableId(node.elseChildren, stableId) ??
      findNodeByStableId(node.emptyChildren, stableId) ??
      (Array.isArray(node.branches)
        ? (node.branches.map(b => findNodeByStableId(b.children, stableId)).find(Boolean) ?? null)
        : null);
    if (found) return found;
  }
  return null;
}
