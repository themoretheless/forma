// @ts-check
// Backend-independent DisplayList IR summary helper for Studio diagnostics.

/**
 * Summarizes Rust `DisplayList::diagnostics_json()` output into aggregate counts,
 * resource links, clip hierarchy depth, and transformed bounds.
 * @param {string | object} input Raw JSON string or parsed diagnostics object
 */
export function summarizeDisplayListDiagnostics(input) {
  const data = typeof input === 'string' ? JSON.parse(input) : input;
  if (!data || typeof data !== 'object') {
    return {
      itemCount: 0,
      clipCount: 0,
      commandFloats: 0,
      edgeFloats: 0,
      maxClipDepth: 0,
      kinds: {},
      resourceIds: [],
    };
  }
  const clips = Array.isArray(data.clips) ? data.clips : [];
  const items = Array.isArray(data.items) ? data.items : [];
  const depthOf = id => {
    let depth = 0;
    let cur = id;
    const seen = new Set();
    while (cur != null && !seen.has(cur) && depth <= clips.length) {
      seen.add(cur);
      depth += 1;
      const node = clips[cur];
      cur = node && node.parent != null ? node.parent : null;
    }
    return depth;
  };
  let maxClipDepth = 0;
  for (let i = 0; i < clips.length; i++) {
    const d = depthOf(i);
    if (d > maxClipDepth) maxClipDepth = d;
  }
  const kinds = {};
  const resources = new Set();
  for (const item of items) {
    if (!item) continue;
    if (item.kind) kinds[item.kind] = (kinds[item.kind] ?? 0) + 1;
    if (item.resourceId != null) resources.add(item.resourceId);
  }
  return {
    itemCount: Number.isFinite(data.itemCount) ? data.itemCount : items.length,
    clipCount: Number.isFinite(data.clipCount) ? data.clipCount : clips.length,
    commandFloats: Number.isFinite(data.commandFloats) ? data.commandFloats : 0,
    edgeFloats: Number.isFinite(data.edgeFloats) ? data.edgeFloats : 0,
    maxClipDepth,
    kinds,
    resourceIds: [...resources],
  };
}
