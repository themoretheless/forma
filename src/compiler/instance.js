// Compiling one control of the scene: instantiating its component and flattening what it draws
// into the template text the runtime loads.

import { matchesType, validateTypeName } from '../component-semantics.js';
import { literal, serializeNode } from '../component-template.js';
import { flatten, num } from './content-layout.js';
import { standard, visualTypes } from './vocabulary.js';

/**
 * @param {{ document: object, enums: object, files: Record<string, string>,
 *   instantiate: Function, metrics: object, visualNodes: object[] }} context
 *   what one compile shares between its controls; `visualNodes` collects a record per control
 */
export function createInstanceCompiler({
  document,
  enums,
  files,
  instantiate,
  metrics,
  visualNodes,
}) {
  return function compileInstance(instance, index) {
    for (const [event, action] of Object.entries(instance.events ?? {})) {
      if (!action.startsWith('events.')) continue;
      const name = action.slice(7),
        signature = document.eventDefinitions?.[name],
        args = instance.eventArgs?.[event] ?? [];
      if (!signature) {
        if (
          Object.keys(document.eventDefinitions ?? {}).length ||
          Array.isArray(document.defaults.events)
        ) {
          if (!document.defaults.events?.includes(name))
            throw Error(`Событие ${name} не объявлено`);
        }
        if (args.length) throw Error(`Для аргументов events.${name} требуется объявление event`);
      } else {
        if (signature.length !== args.length)
          throw Error(
            `events.${name}: ожидалось ${signature.length} аргументов, получено ${args.length}`,
          );
        signature.forEach((arg, i) => {
          validateTypeName(arg.type, enums);
          if (!matchesType(args[i], arg.type, enums))
            throw Error(`events.${name}.${arg.name}: ожидался ${arg.type}`);
        });
      }
    }
    const { roots, resolved, origins } = instantiate(instance, true);
    let rangeMetadata = '';
    if (instance.type === 'Slider') {
      const endpoint = value => {
        const result = instantiate(
          { ...instance, props: { ...instance.props, value: { expr: value + '%' } } },
          true,
        );
        const content = [];
        for (const child of result.roots[0].children)
          if (visualTypes.has(child.type))
            flatten(
              child,
              [0, 0, num(resolved.width, 'width'), num(resolved.height, 'height')],
              files,
              content,
              { ...metrics, preserveEmptyGeometry: true },
            );
        return content.join(' ');
      };
      rangeMetadata = `RangeInput { value: ${parseFloat(resolved.value?.expr ?? resolved.value ?? 50) / 100}; Minimum { ${endpoint(0)} } Maximum { ${endpoint(100)} } }`;
    }
    const root = roots[0],
      out = [],
      visuals = [];
    let body = '';
    for (const n of root.children) {
      if (visualTypes.has(n.type)) {
        flatten(
          n,
          [0, 0, num(resolved.width, 'width'), num(resolved.height, 'height')],
          files,
          out,
          { ...metrics, visuals },
        );
      } else body += serializeNode(n, true);
    }
    const sourceNode = {
      ...instance,
      type: 'Button',
      bindings: {},
      events: instance.events.clicked ? { clicked: instance.events.clicked } : {},
      props: Object.fromEntries(
        Object.entries(resolved).filter(
          ([k]) => standard.has(k) && k !== 'font.size' && k !== 'transition',
        ),
      ),
      children: [],
    };
    const template = `component Button { Rectangle { ${Object.entries(root.props)
      .filter(([k]) => k !== 'key' && k !== 'transition')
      .map(([k, v]) => `${k}: ${literal(v)};`)
      .join(' ')} ${body} ${out.join(' ')} ${rangeMetadata} } }`;
    visualNodes.push(
      {
        id: -2,
        parent: null,
        control: index,
        type: instance.type,
        source: instance.source,
        props: resolved,
        propertyOrigins: origins,
        bounds: [0, 0, num(resolved.width, 'width'), num(resolved.height, 'height')],
      },
      ...visuals.map(v => ({ ...v, parent: v.parent ?? -2, control: index })),
    );
    return { template, treeRoots: roots, sourceNode };
  };
}
