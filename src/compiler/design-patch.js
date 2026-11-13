// Applying design files to a scene: the overrides and the active design state of every
// `.design.ui` the document names.
import { validateDesign, designStatePatch } from '../language.js';
import { designReference, readDesignData } from '../design-data.js';
import { own } from '../shared/object.js';
import { visit } from './data.js';

/**
 * Patches the properties of the keyed nodes of `scene` in place.
 *
 * @param {object[]} scene nodes of the entry document, already cloned
 * @param {object} document the parsed entry document
 * @param {Record<string, string>} files the project
 * @param {string} entry path of the entry document
 * @param {(source: string, path: string) => object} read the caching parser
 * @param {string | null | undefined} designState the state to show; `undefined` picks the first
 */
export function applyDesigns(scene, document, files, entry, read, designState) {
  const merged = { overrides: {}, states: [] };
  const baseDir = entry.includes('/') ? entry.slice(0, entry.lastIndexOf('/') + 1) : '';
  for (const ref of document.designs) {
    const designPath = baseDir + String(ref).replace(/^\.\//, '');
    if (!own(files, designPath)) throw Error(`Дизайн-файл не найден: ${designPath}`);
    const design = read(files[designPath], designPath);
    if (Object.keys(design.overrides ?? {}).length || (design.states?.length ?? 0)) {
      validateDesign(document, design);
      for (const [key, props] of Object.entries(design.overrides ?? {})) {
        if (own(merged.overrides, key)) throw Error(`Повторный дизайн-key ${key}`);
        merged.overrides[key] = props;
      }
      for (const s of design.states ?? []) {
        if (merged.states.some(other => other.name === s.name))
          throw Error(`Повторное состояние ${s.name}`);
        merged.states.push({ ...s, file: designPath });
      }
    }
  }
  const activeState = designState === undefined ? (merged.states[0]?.name ?? null) : designState;
  const designPatch = designStatePatch(merged, activeState);
  const resolveDesignValue = value => {
    const ref = designReference(value);
    if (ref) return readDesignData(ref);
    if (Array.isArray(value)) return value.map(resolveDesignValue);
    if (value && typeof value === 'object' && !own(value, 'expr'))
      return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, resolveDesignValue(v)]));
    return value;
  };
  visit(scene, n => {
    const key = n.props?.key;
    if (typeof key === 'string' && own(designPatch, key)) {
      const resolvedPatch = Object.fromEntries(
        Object.entries(designPatch[key]).map(([k, v]) => [k, resolveDesignValue(v)]),
      );
      n.props = { ...n.props, ...resolvedPatch };
      if (n.bindings) for (const k of Object.keys(resolvedPatch)) delete n.bindings[k];
    }
  });
}
