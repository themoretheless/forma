// @ts-check
// Undo and redo replace the whole file at once, so a canvas selection can only be carried across them
// by describing what each control *is* rather than where its text stood. Two descriptions survive an
// edit elsewhere in the tree: the design `key`, which is how the preview addresses a control already,
// and the slot the control occupies — its type plus the chain of child indexes from the page root,
// which holds however deeply inside a group the control sits.
export function selectionIdentities(root, starts) {
  if (!root) return [];
  const found = new Map();
  const walk = (node, path, slotPath) => {
    found.set(node.start, {
      key: typeof node.props?.key === 'string' ? node.props.key : null,
      type: node.type,
      path: [...path],
      stableId: slotPath,
    });
    const counts = new Map();
    (node.children ?? []).forEach((child, index) => {
      const ord = counts.get(child.type) ?? 0;
      counts.set(child.type, ord + 1);
      walk(child, [...path, index], `${slotPath}/${child.type}#${ord}`);
    });
  };
  walk(root, [], `${root.type}#0`);
  return [...new Set(starts)].map(start => found.get(start)).filter(Boolean);
}
// A control with a key answers to it wherever it stands; one without a key has to be the same type in
// the same slot, or at its unique structural ordinal within the same container when a sibling was
// inserted or removed. Identities come back in the order they were given.
export function restoreSelection(root, identities) {
  const nodes = [];
  let dropped = 0;
  if (!root) return { nodes, starts: [], dropped: identities.length };
  const byKey = new Map(),
    byPath = new Map(),
    byStableId = new Map();
  const walk = (node, path, slotPath) => {
    const key = node.props?.key;
    if (typeof key === 'string' && !byKey.has(key)) byKey.set(key, node);
    byPath.set(path.join('/'), node);
    if (key == null) {
      if (byStableId.has(slotPath)) byStableId.set(slotPath, null);
      else byStableId.set(slotPath, node);
    }
    const counts = new Map();
    (node.children ?? []).forEach((child, index) => {
      const ord = counts.get(child.type) ?? 0;
      counts.set(child.type, ord + 1);
      walk(child, [...path, index], `${slotPath}/${child.type}#${ord}`);
    });
  };
  walk(root, [], `${root.type}#0`);
  for (const step of identities) {
    let node;
    if (step.key !== null) {
      node = byKey.get(step.key);
    } else {
      const candidate = byPath.get(step.path.join('/'));
      if (candidate && candidate.type === step.type && candidate.props?.key == null) {
        node = candidate;
      } else {
        node = step.stableId ? byStableId.get(step.stableId) : undefined;
      }
    }
    if (node) {
      if (!nodes.includes(node)) nodes.push(node);
    } else dropped++;
  }
  return { nodes, starts: nodes.map(node => node.start), dropped };
}
