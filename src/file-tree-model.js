// What the file tree shows for a project: which files, in which folders, and the markup of the
// rows. Pure: it reads the file set and returns data or text, and touches no DOM.
import { escapeHtml } from './shared/text.js';

/**
 * The files the tree lists. A project that has application forms under `ui/` keeps its library
 * (`components/`) and its images (`assets/`) out of the tree; `filter` narrows the rest by a
 * case-insensitive substring of the path.
 */
export function visibleTreePaths(files, filter = '') {
  const all = Object.keys(files),
    hasAppUi = all.some(path => path.startsWith('ui/')),
    shown = hasAppUi
      ? all.filter(path => !path.startsWith('components/') && !path.startsWith('assets/'))
      : all,
    needle = filter.toLowerCase();
  return needle ? shown.filter(path => path.toLowerCase().includes(needle)) : shown;
}

/** Splits paths into folders, in first-seen order, and the files at the project root. */
export function groupTreePaths(paths) {
  const folders = new Map(),
    rootFiles = [];
  for (const path of paths) {
    const slash = path.lastIndexOf('/');
    if (slash < 0) {
      rootFiles.push(path);
      continue;
    }
    const folder = path.slice(0, slash);
    if (!folders.has(folder)) folders.set(folder, []);
    folders.get(folder).push(path);
  }
  return { folders: [...folders], rootFiles };
}

function fileRow(path, selected, root) {
  const rust = path.endsWith('.rs'),
    config = path.endsWith('.toml');
  return `<button class="file ${root ? 'root-file ' : ''}${path === selected ? 'active' : ''}" draggable="true" data-path="${escapeHtml(path)}"><span class="${rust || config ? 'rust' : 'ui'}">${rust ? 'R' : config ? '⚙' : '◇'}</span>${escapeHtml(path.split('/').at(-1))}</button>`;
}

/** The markup of the tree rows for `paths`, with `selected` marked active. */
export function fileTreeHtml(paths, selected) {
  const { folders, rootFiles } = groupTreePaths(paths);
  return (
    folders
      .map(
        ([folder, items]) =>
          `<div class="folder">⌄ &nbsp; ${escapeHtml(folder)}</div>${items.map(path => fileRow(path, selected, false)).join('')}`,
      )
      .join('') + rootFiles.map(path => fileRow(path, selected, true)).join('')
  );
}
