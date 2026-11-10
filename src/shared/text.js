// Small text helpers with no dependencies, shared by Studio modules and testable under node.

const ESC_MAP = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const ESC_RE = /[&<>"']/g;

/** Escapes a value for use in HTML text and double- or single-quoted attributes. */
export const escapeHtml = value => String(value ?? '').replace(ESC_RE, c => ESC_MAP[c]);

/** The directory part of a slash-separated path, or '' for a top-level entry. */
export function parentDirOf(path = '') {
  const idx = path.lastIndexOf('/');
  return idx >= 0 ? path.slice(0, idx) : '';
}

/** One-based line number of offset `end` in `text`. */
export function countLines(text, end = text.length) {
  let n = 1;
  for (let i = 0; i < end; i++) if (text.charCodeAt(i) === 10) n++;
  return n;
}

/** Re-keys a path-to-text map by removing a common path prefix from every key. */
export const stripPathPrefix = (files, prefix) =>
  Object.fromEntries(
    Object.entries(files).map(([path, source]) => [path.slice(prefix.length), source]),
  );
