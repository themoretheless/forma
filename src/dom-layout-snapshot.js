// The HTML preview described the way the vector preview describes its scene, so the canvas tools
// (selection frames, guides, drag targets) work the same over both renderers.

const CONTAINER_CLASSES = new Set(['frame', 'row', 'column', 'grid', 'stack', 'scroll', 'modal']);

/**
 * Measures every element of the preview that came from a source node. Bounds are in the preview's
 * own unscaled pixels, relative to its top left corner; `coordinateBox` is the parent's box, which
 * is what a child's `x` and `y` are counted from.
 *
 * @param {HTMLElement | null | undefined} preview
 */
export function domLayoutSnapshot(preview) {
  if (!preview) return null;
  const rootEl = preview.firstElementChild;
  const r = preview.getBoundingClientRect();
  const scale = r.width / (preview.offsetWidth || r.width || 1);
  const width = preview.offsetWidth || r.width / scale || 360,
    height = preview.offsetHeight || r.height / scale || 220;
  const box = rect => [
    (rect.left - r.left) / scale,
    (rect.top - r.top) / scale,
    rect.width / scale,
    rect.height / scale,
  ];
  const controls = [],
    containers = [];
  for (const el of preview.querySelectorAll('[data-start]')) {
    if (el === rootEl) continue;
    const start = Number(el.dataset.start);
    const bounds = box(el.getBoundingClientRect());
    const coordinateBox = box((el.parentElement ?? preview).getBoundingClientRect());
    const isContainer = [...el.classList].some(c => CONTAINER_CLASSES.has(c));
    if (isContainer) containers.push({ start, bounds, coordinateBox });
    else controls.push({ index: controls.length, start, bounds, coordinateBox });
  }
  return {
    width,
    height,
    clip: false,
    scrollable: false,
    scrollOffset: [0, 0],
    controls,
    containers,
  };
}

/**
 * Remembers the list built from the project's component files until one of them is added, removed
 * or changes length. The canvas asks for it on every pointer move.
 *
 * @template T
 * @param {(files: Record<string, string>) => T} build
 * @returns {(files: Record<string, string>) => T}
 */
export function createComponentCatalog(build) {
  let signature = null,
    catalog;
  return files => {
    const parts = [];
    for (const [path, source] of Object.entries(files))
      if (path.startsWith('components/') && path.endsWith('.ui'))
        parts.push(path + ':' + source.length);
    const next = parts.sort().join('|');
    if (next !== signature) {
      signature = next;
      catalog = build(files);
    }
    return catalog;
  };
}
