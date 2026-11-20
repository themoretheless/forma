// @ts-check
export function similarName(name, names) {
  const distance = (a, b) => {
    let row = Array.from({ length: b.length + 1 }, (_, i) => i);
    for (let i = 0; i < a.length; i++) {
      const next = [i + 1];
      for (let j = 0; j < b.length; j++)
        next.push(Math.min(next[j] + 1, row[j + 1] + 1, row[j] + (a[i] === b[j] ? 0 : 1)));
      row = next;
    }
    return row[b.length];
  };
  const ranked = [...new Set(names)]
    .map(value => ({ value, score: distance(name.toLowerCase(), value.toLowerCase()) }))
    .sort((a, b) => a.score - b.score || a.value.localeCompare(b.value));
  return ranked[0]?.score <= Math.max(1, Math.floor(name.length / 3)) &&
    ranked[0]?.score !== ranked[1]?.score
    ? ranked[0].value
    : null;
}
export function sourceError(message, source, related) {
  const error = new Error(message);
  error.diagnostic = { message, source, related };
  return error;
}

/**
 * Maps an error thrown by the WASM runtime back onto the `.ui` source location in `sourceMap`.
 */
export function mapRuntimeErrorToUiDiagnostic(rawError, sourceMap) {
  const message =
    typeof rawError === 'string' ? rawError : String(rawError?.message ?? rawError ?? '');
  if (rawError && typeof rawError === 'object' && rawError.diagnostic) return rawError;
  if (!sourceMap) return rawError instanceof Error ? rawError : new Error(message);
  const quoted = [...message.matchAll(/['"]([^'"]+)['"]/g)].map(m => m[1]);
  const controls = sourceMap.controls ?? [];
  const root = sourceMap.root ?? { file: sourceMap.entry, from: 0, to: 0, propertySources: {} };

  for (const token of quoted) {
    const ctrl = controls.find(c => c.key === token);
    if (ctrl) {
      const prop = quoted.find(q => q !== token && Object.hasOwn(ctrl.propertySources ?? {}, q));
      const src = (prop && ctrl.propertySources[prop]) || {
        file: ctrl.file,
        from: ctrl.from,
        to: ctrl.to,
      };
      return sourceError(message, src, ctrl.definition ?? undefined);
    }
  }
  for (const token of quoted) {
    const ctrl = controls.find(c => Object.hasOwn(c.propertySources ?? {}, token));
    if (ctrl) {
      return sourceError(message, ctrl.propertySources[token], ctrl.definition ?? undefined);
    }
    if (Object.hasOwn(root.propertySources ?? {}, token)) {
      return sourceError(message, root.propertySources[token]);
    }
  }
  if (controls.length === 1) {
    const only = controls[0];
    return sourceError(
      message,
      { file: only.file, from: only.from, to: only.to },
      only.definition ?? undefined,
    );
  }
  return sourceError(message, { file: root.file, from: root.from, to: root.to });
}
