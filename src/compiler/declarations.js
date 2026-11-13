// What a document and the files it `use`s declare for the whole compile: enums and structs,
// design tokens, translations and styles.
import { own } from '../shared/object.js';
import { primitiveTypes } from './builtins.js';
import { clone, visit } from './data.js';

/**
 * @param {object} document the parsed entry document
 * @param {string} entry its path; `use` paths are relative to it
 * @param {Record<string, string>} files the project
 * @param {(source: string, path: string) => object} read the caching parser
 * @returns {{ enums: object, tokens: object, i18n: object, styles: object }} the used files first,
 *   then the document itself, so its own declarations win
 */
export function sharedDeclarations(document, entry, files, read) {
  const enums = {};
  const tokens = {};
  const i18n = {};
  const styles = {};
  if (document.uses?.length) {
    const resolveRel = (basePath, rel) => {
      const parts = basePath.includes('/') ? basePath.split('/').slice(0, -1) : [];
      for (const p of String(rel).split('/')) {
        if (!p || p === '.') continue;
        if (p === '..') parts.pop();
        else parts.push(p);
      }
      return parts.join('/');
    };
    for (const u of document.uses) {
      const uPath = resolveRel(entry, u);
      if (!own(files, uPath)) throw Error(`Импортируемый файл не найден: ${uPath}`);
      const uDoc = read(files[uPath], uPath);
      Object.assign(enums, uDoc.enums ?? {}, uDoc.structs ?? {});
      if (uDoc.structConstraints)
        enums._structConstraints = {
          ...(enums._structConstraints ?? {}),
          ...clone(uDoc.structConstraints),
        };
      Object.assign(tokens, uDoc.tokens ?? {});
      Object.assign(i18n, uDoc.i18n ?? {});
      Object.assign(styles, uDoc.styles ?? {});
    }
  }
  Object.assign(enums, document.enums, document.structs);
  if (document.structConstraints && Object.keys(document.structConstraints).length)
    enums._structConstraints = {
      ...(enums._structConstraints ?? {}),
      ...clone(document.structConstraints),
    };
  Object.assign(tokens, document.tokens);
  Object.assign(i18n, document.i18n);
  Object.assign(styles, document.styles);
  return { enums, tokens, i18n, styles };
}

/**
 * Adds what the components used in `scene` declare. An enum declared twice must agree; a token,
 * translation or style already present is kept.
 *
 * @param {{ enums: object, tokens: object, i18n: object, styles: object }} declarations updated in place
 * @param {object[]} scene
 * @param {(name: string) => object} link
 */
export function addComponentDeclarations({ enums, tokens, i18n, styles }, scene, link) {
  visit(scene, node => {
    if (
      !primitiveTypes.has(node.type) &&
      !['Scroll', 'Modal', 'If', 'For', 'Let', 'Match'].includes(node.type)
    ) {
      const definition = link(node.type);
      for (const [name, variants] of Object.entries(definition.enums ?? {})) {
        if (name === '_structConstraints') {
          enums._structConstraints = { ...(enums._structConstraints ?? {}), ...variants };
          continue;
        }
        if (own(enums, name) && JSON.stringify(enums[name]) !== JSON.stringify(variants))
          throw Error(`Конфликт enum ${name}`);
        enums[name] = variants;
      }
      for (const [name, val] of Object.entries(definition.tokens ?? {})) {
        if (!own(tokens, name)) tokens[name] = val;
      }
      for (const [name, val] of Object.entries(definition.i18n ?? {})) {
        if (!own(i18n, name)) i18n[name] = val;
      }
      for (const [name, val] of Object.entries(definition.styles ?? {})) {
        if (!own(styles, name)) styles[name] = val;
      }
    }
  });
}
