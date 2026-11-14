// Instantiating a component: checking the instance against the component's contract, resolving
// its properties and expanding its template, including the control flow inside it.

import {
  evaluate,
  evaluateProperties,
  expandStructure,
  selectedProperties,
  selectedPropertySources,
  validateContract,
} from '../component-semantics.js';
import { propertyOrigins } from '../property-origins.js';
import { own } from '../shared/object.js';
import { rangeBounds, rangeFraction } from '../shared/range.js';
import { fallback, primitiveTypes } from './builtins.js';
import { clone, visit } from './data.js';
import { layoutProps, standard, visualTypes } from './vocabulary.js';

/**
 * @param {{ link: (name: string) => object, state: object, stateDefaults: object, enums: object,
 *   tokens: object, styles: object, viewport: object, i18n: object }} environment
 *   the linker and what expressions of one compile are evaluated against
 * @returns {(instance: object, top: boolean, parents?: string[]) => object}
 */
export function createInstantiator({
  link,
  state,
  stateDefaults,
  enums,
  tokens,
  styles,
  viewport,
  i18n,
}) {
  const contracts = new WeakMap();
  let expandedCount = 0;
  function rejectInteraction(n) {
    if (
      n.type === 'PointerArea' ||
      Object.keys(n.events ?? {}).length ||
      Object.keys(n.bindings ?? {}).length
    )
      throw Error(`Вложенный визуальный компонент не содержит интерактивных элементов: ${n.type}`);
  }
  function instantiate(instance, top, parents = []) {
    if (parents.includes(instance.type))
      throw Error(`Цикл композиции: ${[...parents, instance.type].join(' → ')}`);
    if (parents.length >= 32) throw Error('Глубина композиции компонентов превышает 32');
    if (!top) rejectInteraction(instance);
    if (instance.groupBound && own(instance.props, 'group')) {
      // The shared expansion copies every binding into the properties; the group is not a
      // property of the control, its `checked` was derived from it when the source was parsed.
      const props = { ...instance.props };
      delete props.group;
      instance = { ...instance, props };
    }
    const linked = link(instance.type);
    if (instance.normalizedRange && typeof instance.props.value === 'number') {
      // A bound number lies on the control's scale; the template takes a fraction of it.
      const bounds = rangeBounds(name => instance.props[name] ?? linked.defaults[name]);
      const fraction = rangeFraction(bounds, instance.props.value);
      // The previews write the widget's fraction back onto the same scale.
      instance.rangeBounds = bounds;
      instance = {
        ...instance,
        props: { ...instance.props, value: { expr: `${fraction * 100}%` } },
      };
    }
    let contract = contracts.get(linked);
    if (!contract) {
      let hasContent = false;
      const presenterKeys = new Set(Object.keys(linked.slotDefinitions ?? {}));
      visit(linked.nodes, n => {
        if (n.type === 'ContentPresenter') {
          if (n.props.key === 'content') hasContent = true;
          if (typeof n.props.key === 'string') presenterKeys.add(n.props.key);
        }
      });
      const groupedNames = new Set(
        (linked.matches ?? []).flatMap(group =>
          group.branches.flatMap(branch => Object.keys(branch.props)),
        ),
      );
      contract = { hasContent, presenterKeys, groupedNames };
      contracts.set(linked, contract);
    }
    const { hasContent, presenterKeys, groupedNames } = contract;
    if (instance.children.length && !hasContent)
      throw Error('Для дочернего контента нужен ContentPresenter с key: content');
    for (const k of Object.keys(instance.props))
      if (
        !(top ? standard : layoutProps).has(k) &&
        !layoutProps.has(k) &&
        !own(linked.defaults, k) &&
        !own(linked.propDefinitions ?? {}, k) &&
        !presenterKeys.has(k) &&
        !groupedNames.has(k) &&
        !(k === 'font.size' && own(linked.defaults, 'fontSize'))
      )
        throw Error(`Неизвестное свойство ${instance.type}.${k}`);
    if (instance.props['font.size'] !== undefined && instance.props.fontSize !== undefined)
      throw Error('fontSize и font.size — одно свойство');
    // A visual instance never inherits its caller's props or button fallbacks.
    // Arguments and supplied children have already been resolved in the caller.
    const rawSlotFills = {};
    for (const key of presenterKeys)
      if (own(instance.props, key) && instance.props[key]?.slotParams?.length)
        rawSlotFills[key] = instance.props[key];
    const props = { ...(top ? fallback : {}), ...linked.defaults, ...instance.props };
    const compStateDefaults = Object.fromEntries(
      Object.entries(linked.stateDefinitions ?? {})
        .filter(([, d]) => d.default !== undefined)
        .map(([k, d]) => [k, d.default]),
    );
    const env = {
      ...instance.environment,
      viewport,
      enums: { ...enums, ...linked.enums },
      tokens: { ...tokens, ...linked.tokens },
      i18n: { ...i18n, ...linked.i18n },
      styles: { ...styles, ...linked.styles },
      stateDefaults: { ...stateDefaults, ...compStateDefaults },
      locals: instance.environment?.locals,
    };
    Object.assign(props, selectedProperties(linked.matches, props, state, env), instance.props);
    for (const [key, definition] of Object.entries(linked.propDefinitions ?? {}))
      if (definition.required && !own(linked.defaults, key) && !own(instance.props, key))
        throw Error(`${instance.type}: обязательное свойство ${key} не задано`);
    if (instance.props['font.size'] !== undefined) props.fontSize = instance.props['font.size'];
    const sources = {
      ...linked.defaultSources,
      ...selectedPropertySources(linked.matches, props, state, env),
      ...instance.propertySources,
    };
    const scope = {
      props,
      sources,
      traces: instance.propertyOrigins ?? {},
      instance,
      rawSlotFills,
      top,
      environment: env,
      parents: [...parents, instance.type],
    };
    const resolved = {};
    for (const key of Object.keys(props)) {
      if (own(rawSlotFills, key)) {
        resolved[key] = rawSlotFills[key];
        continue;
      }
      resolved[key] = expandValue(props[key], scope, 0, [key]);
    }
    validateContract(linked.propDefinitions, resolved, env.enums, instance.type);
    const roots = expandChildren(linked.nodes, scope, 0);
    if (
      roots.length !== 1 ||
      (top ? roots[0].type !== 'Rectangle' : !visualTypes.has(roots[0].type))
    )
      throw Error(
        top
          ? 'Базовая кнопка требует один Rectangle'
          : 'Визуальный компонент требует один Frame, Text, Image или Rectangle',
      );
    if (!top) {
      visit(roots, n => {
        rejectInteraction(n);
        if (!visualTypes.has(n.type)) throw Error(`Неподдерживаемый визуальный примитив ${n.type}`);
      });
      // Keep defining-file origins, and use the caller's origin for layout edits.
      const root = roots[0];
      if (
        own(instance.props, 'cell') &&
        (own(instance.props, 'row') || own(instance.props, 'column'))
      )
        throw Error('cell: ожидаются row column без отдельных row/column');
      if (['cell', 'row', 'column'].some(key => own(instance.props, key))) {
        root.propertySources = { ...root.propertySources };
        for (const key of ['cell', 'row', 'column']) {
          delete root.props[key];
          delete root.propertySources[key];
        }
      }
      for (const key of layoutProps)
        if (own(instance.props, key)) {
          root.props[key] = resolved[key];
          if (instance.propertySources?.[key])
            root.propertySources = {
              ...root.propertySources,
              [key]: clone(instance.propertySources[key]),
            };
        }
    }
    const origins = {};
    for (const key of Object.keys(props)) {
      const value = props[key];
      origins[key] =
        instance.propertyOrigins?.[key] ??
        propertyOrigins(value, sources[key], props, sources, {}, new Set([key]));
    }
    return { roots, resolved, origins };
  }
  function expandValue(value, scope, depth, stack = []) {
    const result = evaluate(value, scope.props, state, stack, false, scope.environment);
    return result?.type ? expand(result, scope, depth + 1) : result;
  }
  function expandChildren(nodes, scope, depth) {
    return expandStructure(nodes, scope.props, state, scope.environment, {
      recursive: false,
      evaluateProps: false,
    }).map(node => expand(node, { ...scope, environment: node.environment }, depth + 1));
  }
  function expand(n, scope, depth) {
    if (Object.keys(n.bindings ?? {}).length)
      throw Error(
        'Вложенные двусторонние привязки пока не поддерживаются; задайте привязку на экземпляре контрола',
      );
    if (depth > 64 || ++expandedCount > 16384)
      throw Error('Превышен предел глубины или размера визуальной композиции');
    if (n.expandedVisual) return clone(n);
    if (Object.keys(n.slots ?? {}).length)
      throw Error('override объявляется в наследнике component');
    if (!scope.top) rejectInteraction(n);
    if (n.type === 'ContentPresenter') {
      if (Object.keys(n.props).some(k => !['key', 'content'].includes(k)))
        throw Error('ContentPresenter принимает key и content');
      let content =
        n.props.content ??
        (n.children.length === 1
          ? n.children[0]
          : { type: 'Frame', props: {}, children: n.children });
      const slotKey = n.props.key;
      let slotScope = scope;
      if (scope.instance.children.length && slotKey === 'content') {
        if (own(scope.instance.props, 'content'))
          throw Error('Задайте content или дочерние элементы');
        content = { type: 'Frame', props: {}, children: scope.instance.children };
      } else if (typeof slotKey === 'string' && own(scope.instance.props, slotKey)) {
        content = scope.rawSlotFills?.[slotKey] ?? scope.instance.props[slotKey];
      }
      if (n.slotArgs?.length) {
        const slotLocals = { ...(scope.environment?.locals ?? {}) };
        const fillParams = content?.slotParams ?? [];
        n.slotArgs.forEach((arg, idx) => {
          const val = evaluate(arg.value, scope.props, state, [], false, scope.environment);
          slotLocals[arg.name] = val;
          if (fillParams[idx]) slotLocals[fillParams[idx]] = val;
        });
        slotScope = { ...scope, environment: { ...scope.environment, locals: slotLocals } };
        if (content?.expandedVisual) {
          content = { ...content, expandedVisual: false };
        }
      }
      const result = expandValue(content, slotScope, depth);
      if (!result?.type) throw Error('override должен вернуть элемент или base.content');
      return result;
    }
    const sources = {
      ...selectedPropertySources(n.matches, scope.props, state, scope.environment),
      ...n.propertySources,
    };
    const raw = {
      ...Object.fromEntries((n.forward ?? []).map(key => [key, { expr: `props.${key}` }])),
      ...selectedProperties(n.matches, scope.props, state, scope.environment),
      ...n.props,
    };
    const origins = Object.fromEntries(
      Object.entries(raw).map(([key, value]) => [
        key,
        propertyOrigins(value, sources[key], scope.props, scope.sources, scope.traces),
      ]),
    );
    const slotFillKeys = new Set();
    for (const [k, v] of Object.entries(n.props ?? {}))
      if (v?.slotParams?.length) slotFillKeys.add(k);
    const evalNode = slotFillKeys.size
      ? {
          ...n,
          props: Object.fromEntries(Object.entries(n.props).filter(([k]) => !slotFillKeys.has(k))),
        }
      : n;
    const evaluated = evaluateProperties(evalNode, scope.props, state, scope.environment);
    const expandedProps = {};
    for (const key of Object.keys(evaluated)) {
      const value = evaluated[key];
      expandedProps[key] = value?.type ? expand(value, scope, depth + 1) : value;
    }
    for (const k of slotFillKeys) expandedProps[k] = n.props[k];
    const node = {
      ...n,
      props: expandedProps,
      propertySources: sources,
      propertyOrigins: origins,
      children: expandChildren(n.children ?? [], scope, depth),
      forward: [],
      matches: [],
    };
    if (!primitiveTypes.has(n.type)) return instantiate(node, false, scope.parents).roots[0];
    return { ...node, expandedVisual: true };
  }
  return instantiate;
}
