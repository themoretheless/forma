// @ts-check
// Linking component definitions: resolving a component name to its definition with the base
// component, overrides and contracts folded in.

import { validateTypeName } from './component-semantics.js';
import { similarName, sourceError } from './diagnostics.js';
import { own } from '../shared/object.js';
import { builtinSources } from './builtins.js';
import { clone, visit } from './data.js';
import { contentProps } from './vocabulary.js';

function substitute(v, key, base) {
  if (v?.expr?.startsWith('base.')) {
    if (v.expr !== `base.${key}`) throw Error(`В override ${key} допустим только base.${key}`);
    return clone(base);
  }
  if (Array.isArray(v)) return v.map(x => substitute(x, key, base));
  if (v && typeof v === 'object')
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, substitute(x, key, base)]));
  return v;
}

/**
 * @param {{ files: Record<string, string>, read: Function, links?: { get: Function, set: Function } }} project
 *   the project files, the caching parser, and the cache of linked definitions kept between compiles
 */
export function createLinker({ files, read, links }) {
  const cache = new Map(),
    loading = [],
    dependencies = new Map(),
    depths = new Map();
  function link(name) {
    if (cache.has(name)) {
      if (loading.length + depths.get(name) > 32)
        throw Error('Глубина наследования компонентов превышает 32');
      return cache.get(name);
    }
    if (loading.includes(name)) throw Error(`Цикл наследования: ${[...loading, name].join(' → ')}`);
    if (loading.length >= 32) throw Error('Глубина наследования компонентов превышает 32');
    const saved = links?.get(name, files);
    if (saved) {
      if (loading.length + saved.depth > 32)
        throw Error('Глубина наследования компонентов превышает 32');
      cache.set(name, saved.linked);
      dependencies.set(name, saved.dependencies);
      depths.set(name, saved.depth);
      return saved.linked;
    }
    const path = `components/${name}.ui`,
      source = own(files, path) ? files[path] : builtinSources[name];
    if (source === undefined) throw Error(`Компонент не найден: ${path}`);
    const inputs = links ? new Map([[path, files[path]]]) : null;
    const c = read(source, path);
    if (c.name !== name) throw Error(`${path}: ожидался component ${name}`);
    if (c.designs.length)
      throw Error('Дизайн-атрибут компонента пока не поддерживается векторным runtime');
    loading.push(name);
    const linked = c.base ? clone(link(c.base)) : { nodes: [], defaults: {} };
    if (links && c.base)
      for (const [file, source] of dependencies.get(c.base)) inputs.set(file, source);
    if (c.base && c.nodes.length) throw Error('Наследник меняет визуальное дерево через override');
    if (!c.base) linked.nodes = clone(c.nodes);
    linked.defaults = { ...linked.defaults, ...clone(c.defaults) };
    linked.defaultSources = {
      ...linked.defaultSources,
      ...Object.fromEntries(
        Object.entries(c.defaultRanges ?? {}).map(([key, range]) => [
          key,
          { file: path, ...range, label: c.name },
        ]),
      ),
    };
    linked.propDefinitions = { ...linked.propDefinitions, ...clone(c.propDefinitions ?? {}) };
    linked.stateDefinitions = { ...linked.stateDefinitions, ...clone(c.stateDefinitions ?? {}) };
    linked.eventDefinitions = { ...linked.eventDefinitions, ...clone(c.eventDefinitions ?? {}) };
    linked.enums = { ...linked.enums };
    linked.tokens = { ...linked.tokens };
    linked.i18n = { ...linked.i18n };
    linked.styles = { ...linked.styles };
    if (c.uses?.length) {
      const resolveRel = (basePath, rel) => {
        const parts = basePath.includes('/') ? basePath.split('/').slice(0, -1) : [];
        for (const p of String(rel).split('/')) {
          if (!p || p === '.') continue;
          if (p === '..') parts.pop();
          else parts.push(p);
        }
        return parts.join('/');
      };
      for (const u of c.uses) {
        const uPath = resolveRel(path, u);
        if (!own(files, uPath)) throw Error(`Импортируемый файл не найден: ${uPath}`);
        inputs?.set(uPath, files[uPath]);
        const uDoc = read(files[uPath], uPath);
        Object.assign(linked.enums, clone(uDoc.enums ?? {}), clone(uDoc.structs ?? {}));
        if (uDoc.structConstraints)
          linked.enums._structConstraints = {
            ...(linked.enums._structConstraints ?? {}),
            ...clone(uDoc.structConstraints),
          };
        Object.assign(linked.tokens, clone(uDoc.tokens ?? {}));
        Object.assign(linked.i18n, clone(uDoc.i18n ?? {}));
        Object.assign(linked.styles, clone(uDoc.styles ?? {}));
      }
    }
    Object.assign(linked.enums, clone(c.enums ?? {}), clone(c.structs ?? {}));
    if (c.structConstraints && Object.keys(c.structConstraints).length)
      linked.enums._structConstraints = {
        ...(linked.enums._structConstraints ?? {}),
        ...clone(c.structConstraints),
      };
    Object.assign(linked.tokens, clone(c.tokens ?? {}));
    Object.assign(linked.i18n, clone(c.i18n ?? {}));
    Object.assign(linked.styles, clone(c.styles ?? {}));
    linked.slotDefinitions = { ...linked.slotDefinitions, ...clone(c.slotDefinitions ?? {}) };
    linked.matches = [...(linked.matches ?? []), ...clone(c.matches ?? [])];
    for (const definition of Object.values(linked.propDefinitions))
      validateTypeName(definition.type, linked.enums);
    for (const definition of Object.values(linked.stateDefinitions))
      validateTypeName(definition.type, linked.enums);
    for (const args of Object.values(linked.eventDefinitions))
      for (const arg of args) validateTypeName(arg.type, linked.enums);
    if (c.forward?.length)
      throw Error('forward применяется к дочернему элементу, не к объявлению component');
    const presenters = new Map();
    visit(linked.nodes, n => {
      if (n.type === 'ContentPresenter') {
        const key = n.props.key;
        if (typeof key !== 'string' || !key) throw Error('ContentPresenter требует key');
        if (presenters.has(key)) throw Error(`Повторная точка расширения ${key}`);
        presenters.set(key, n);
      }
    });
    const patchProperties = (target, patch, key) => {
      if (
        patch.base ||
        patch.children.length ||
        patch.matches?.length ||
        patch.forward?.length ||
        Object.keys(patch.slots).length ||
        Object.keys(patch.events).length ||
        Object.keys(patch.bindings).length
      )
        throw Error('Блок override переопределяет только свойства');
      if (own(patch.props, 'key')) throw Error('override не меняет key целевого элемента');
      const allowed = contentProps.get(target.type);
      if (allowed && target.type !== 'Rectangle')
        for (const name of Object.keys(patch.props))
          if (!allowed.has(name)) {
            const suggestion = similarName(name, allowed);
            throw sourceError(
              `override ${key}: узел ${target.type} не поддерживает свойство ${name}.${suggestion ? ` Возможно, вы имели в виду ${suggestion}.` : ''}`,
              patch.propertySources?.[name] ?? patch.source,
              target.source,
            );
          }
      Object.assign(target.props, clone(patch.props));
      target.propertySources = {
        ...target.propertySources,
        ...Object.fromEntries(
          Object.entries(patch.propertySources ?? {}).map(([name, origin]) => [
            name,
            { ...origin, label: `${c.name} · override ${key}` },
          ]),
        ),
      };
    };
    for (const [key, value] of Object.entries(c.slots)) {
      if (value?.propertyOverride) {
        if (!c.base) throw Error('override требует базового компонента');
        const targets = [],
          keys = [];
        const find = value => {
          if (!value || typeof value !== 'object') return;
          if (value.type && typeof value.props?.key === 'string') {
            keys.push(value.props.key);
            if (value.props.key === key) targets.push(value);
          }
          for (const child of Object.values(value)) find(child);
        };
        find(linked.nodes);
        if (targets.length !== 1) {
          const suggestion = similarName(key, keys);
          throw sourceError(
            `override ${key}: ожидался один элемент, найдено ${targets.length}.${!targets.length && suggestion ? ` Возможно, вы имели в виду ${suggestion}.` : ''}`,
            value.patch.source,
          );
        }
        const target = targets[0];
        if (target.type === 'ContentPresenter') {
          const content =
            target.props.content ??
            (target.children.length === 1
              ? target.children[0]
              : { type: 'Frame', props: {}, children: target.children });
          const patchContent = content => {
            if (content?.type && content.props) {
              patchProperties(content, value.patch, key);
              return;
            }
            if (content?.match !== undefined) {
              for (const branch of content.branches) patchContent(branch.value);
              return;
            }
            throw Error(`override ${key}: для изменения свойств содержимое должно быть элементом`);
          };
          patchContent(content);
          target.props.content = content;
          target.children = [];
        } else patchProperties(target, value.patch, key);
        continue;
      }
      if (value?.fileOverride) {
        if (!c.base) throw Error('override требует базового компонента');
        const targets = [];
        visit(linked.nodes, n => {
          if (n.props.key === key) targets.push(n);
        });
        if (targets.length !== 1)
          throw Error(`override ${key}: ожидался один элемент, найдено ${targets.length}`);
        const target = targets[0],
          parts = path.split('/').slice(0, -1);
        if (value.file.startsWith('/') || value.file.includes('\\'))
          throw Error('override from: требуется относительный путь проекта');
        for (const part of value.file.split('/')) {
          if (!part || part === '.') continue;
          if (part === '..') {
            if (!parts.length) throw Error('override from: выход за пределы проекта');
            parts.pop();
          } else parts.push(part);
        }
        const file = parts.join('/');
        if (!own(files, file)) throw Error(`Файл override не найден: ${file}`);
        inputs?.set(file, files[file]);
        const patch = read(files[file], file, true).nodes[0];
        if (patch.type !== target.type)
          throw Error(`override ${key}: ожидался ${target.type}, получен ${patch.type} в ${file}`);
        for (const p of [patch, value.patch].filter(Boolean)) {
          patchProperties(target, p, key);
        }
        continue;
      }
      const target = presenters.get(key);
      if (!target) throw Error(`Нет точки расширения ${key} в ${name}`);
      if (!c.base) throw Error('override требует базового компонента');
      const base =
        target.props.content ??
        (target.children.length === 1
          ? target.children[0]
          : { type: 'Frame', props: {}, children: target.children });
      target.props.content = substitute(value, key, base);
      target.children = [];
    }
    const depth = 1 + (c.base ? depths.get(c.base) : 0);
    loading.pop();
    cache.set(name, linked);
    if (links) dependencies.set(name, inputs);
    depths.set(name, depth);
    links?.set(name, linked, inputs, depth);
    return linked;
  }
  return { link, definitions: () => Object.fromEntries(cache) };
}
