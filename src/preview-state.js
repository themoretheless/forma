// Writes only existing model fields. Repeated elements retain their actual item
// object in their lexical environment, so a sort cannot redirect an input edit.
export function writePreviewBinding(state, node, path, value, {createMissing = false} = {}) {
  const [root, ...parts] = path.split('.');
  let target = root === 'state' ? state : node.environment?.locals?.[root];
  if (!target || !parts.length) throw Error(`Недоступная привязка ${path}`);
  const field = parts.pop();
  for (const part of parts) {
    if (!Object.hasOwn(target, part) || target[part] == null) throw Error(`Нет значения ${path}`);
    target = target[part];
  }
  if (!Object.hasOwn(target, field) && !createMissing) throw Error(`Нет значения ${path}`);
  if (Object.is(target[field], value)) return false;
  target[field] = value;
  return true;
}
