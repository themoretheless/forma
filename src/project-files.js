// Changes to the set of project files made from the file tree. Each one refuses what would lose
// a file or leave the project unsaveable, and says why; none touches the DOM.
import { validateProject } from './browser-storage.js';

const refused = reason => ({ ok: false, reason });

/** Why `path` cannot name a project file, or `null`. */
function pathProblem(path) {
  try {
    validateProject({ [path]: '' });
    return null;
  } catch {
    return `Недопустимое имя: ${path}`;
  }
}

/** Why nothing can be put at `path`, or `null`. */
function placementProblem(files, path) {
  return pathProblem(path) ?? (Object.hasOwn(files, path) ? `Файл уже существует: ${path}` : null);
}

/** Adds a new file. An existing file is never replaced. */
export function createFile(files, path, content) {
  const problem = placementProblem(files, path);
  if (problem) return refused(problem);
  files[path] = content;
  return { ok: true };
}

/** Moves a file to another path. The destination must be free. */
export function renameFile(files, fromPath, toPath) {
  if (!Object.hasOwn(files, fromPath)) return refused(`Файл не найден: ${fromPath}`);
  const problem = placementProblem(files, toPath);
  if (problem) return refused(problem);
  files[toPath] = files[fromPath];
  delete files[fromPath];
  return { ok: true };
}

/** Copies a file next to itself under the first free `-copy` name and returns that name. */
export function duplicateFile(files, path) {
  if (!Object.hasOwn(files, path)) return refused(`Файл не найден: ${path}`);
  const name = path.slice(path.lastIndexOf('/') + 1),
    dot = name.lastIndexOf('.'),
    extension = dot > 0 ? name.slice(dot) : '',
    stem = path.slice(0, path.length - extension.length);
  let copy = `${stem}-copy${extension}`;
  for (let n = 2; Object.hasOwn(files, copy); n++) copy = `${stem}-copy-${n}${extension}`;
  files[copy] = files[path];
  return { ok: true, path: copy };
}

/** Where dropping `fromPath` on `targetDir` would put it, or `null` when it would not move. */
export function moveDestination(fromPath, targetDir) {
  const name = fromPath.split('/').at(-1),
    toPath = targetDir ? `${targetDir}/${name}` : name;
  return toPath === fromPath || toPath.startsWith(fromPath + '/') ? null : toPath;
}

/** Where the component called `name` lives in a project. */
export const componentPath = name => `components/${name}.ui`;

/**
 * A free component name to offer when `node` is extracted: its key with a capital first letter,
 * or `Custom<Type>`, numbered until no file has the name.
 */
export function suggestComponentName(files, node) {
  const key =
    typeof node?.props?.key === 'string' ? node.props.key.replace(/[^A-Za-z0-9_]/g, '') : '';
  const base = key ? key[0].toUpperCase() + key.slice(1) : `Custom${node?.type || 'Component'}`;
  let name = base;
  for (let n = 2; Object.hasOwn(files, componentPath(name)); n++) name = `${base}${n}`;
  return name;
}
