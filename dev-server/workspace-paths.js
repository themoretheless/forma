import path from 'node:path';

// Containment checks for the dev server's workspace file API. They compare path segments, not
// string prefixes, so `..config` is an ordinary name and Windows separators are understood.

/** Whether a `path.relative(root, target)` result points outside `root`. */
export function escapesRoot(relative, paths = path) {
  return relative === '..' || relative.startsWith(`..${paths.sep}`) || paths.isAbsolute(relative);
}

/** Whether `child` lies strictly inside the directory `parent`. */
export function isInsideDir(parent, child, paths = path) {
  const relative = paths.relative(parent, child);
  return relative !== '' && !escapesRoot(relative, paths);
}
