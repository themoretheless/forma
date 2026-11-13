// Component linking is shared by Studio preview and native launch. It emits the
// same concrete primitive template; there is no DOM rendering of button content.
import { parse } from './language.js';
import { attachSources, createElementTree } from './element-tree.js';
import { createCacheBudget } from './cache-budget.js';
import {
  evaluate,
  selectedProperties,
  selectedPropertySources,
  validateContract,
  expandStructure,
} from './component-semantics.js';
import {
  containerTypes,
  naturalContainer,
  intrinsicLength,
  length,
  constrained,
} from './component-layout.js';
import { frameTemplates, serialize } from './component-template.js';
import { own } from './shared/object.js';
import { clone, visit } from './compiler/data.js';
import { cachingMetrics } from './compiler/metrics-cache.js';
import { applyDesigns } from './compiler/design-patch.js';
import { createLinker } from './compiler/link.js';
import { createInstanceCompiler } from './compiler/instance.js';
import { createSceneLayout } from './compiler/scene-layout.js';
import { createInstantiator } from './compiler/instantiate.js';
import { sharedDeclarations } from './compiler/declarations.js';
import { sceneModals } from './compiler/modals.js';
import { builtinSources, fallback, primitiveTypes } from './compiler/builtins.js';
import { cell, gap, insets, sizes } from './compiler/content-layout.js';
export { evaluate } from './component-semantics.js';
export { contentProps, layoutProps, standard } from './compiler/vocabulary.js';

const readDocument = (source, path, fragment = false) =>
  attachSources(parse(source, { fragment }), path);

// A session retains parsed documents and definition-only linked templates under
// one budget. State, callbacks, expansion and compiled output stay per-compile.
export function createComponentCompiler({ cache: storage = createCacheBudget() } = {}) {
  let linkedHits = 0,
    linkedMisses = 0;
  function read(source, path, fragment = false) {
    const key = (fragment ? 'compiler:fragment:' : 'compiler:document:') + path,
      previous = storage.get(key);
    const document =
      previous?.source === source ? previous.document : readDocument(source, path, fragment);
    if (previous?.document !== document) storage.set(key, { source, document });
    return document;
  }
  const links = {
    get(name, files) {
      const saved = storage.get('compiler:linked:' + name);
      let valid = !!saved;
      if (saved)
        for (const [path, source] of saved.dependencies)
          if (files[path] !== source) {
            valid = false;
            break;
          }
      if (valid) {
        linkedHits++;
        return { ...saved, linked: clone(saved.linked) };
      }
      if (saved) storage.delete('compiler:linked:' + name);
      linkedMisses++;
      return null;
    },
    set(name, linked, dependencies, depth) {
      storage.set('compiler:linked:' + name, { linked: clone(linked), dependencies, depth });
    },
  };
  const run = (files, entry, state = {}, metrics = {}) =>
    compile(files, entry, state, metrics, read, links);
  run.cacheStats = () => ({ ...storage.snapshot(), linkedHits, linkedMisses });
  return run;
}

export function compileComponents(files, entry, state = {}, metrics = {}) {
  return compile(files, entry, state, metrics, readDocument);
}
// Controls in runtime order: base scene (optionally inside one Scroll), then
// the children of every Modal layer. Hosts index Rust controls by this order.
export function sceneControlNodes(rootOrChildren) {
  const children = Array.isArray(rootOrChildren)
    ? rootOrChildren
    : (rootOrChildren?.children ?? []);
  const base = children.filter(n => n.type !== 'Modal'),
    modals = children.filter(n => n.type === 'Modal');
  const controls = base[0]?.type === 'Scroll' ? base[0].children : base;
  return [...controls, ...modals.flatMap(m => m.children ?? [])];
}
function sceneModalNodes(rootOrChildren) {
  const children = Array.isArray(rootOrChildren)
    ? rootOrChildren
    : (rootOrChildren?.children ?? []);
  return children.filter(n => n.type === 'Modal');
}
// Definition linking is also consumed by Rust code generation. It deliberately
// runs before evaluating props/state so generated forms retain dependencies.
export function linkComponentDefinitions(files) {
  const entry = '__forma_definitions.ui';
  return compile(
    { ...files, [entry]: 'component Definitions { Frame {} }' },
    entry,
    {},
    { definitionsOnly: true },
    readDocument,
  );
}
function compile(files, entry, state, metrics, read, links) {
  const metricsContext = metrics;
  metrics = cachingMetrics(metrics);
  const document = read(files[entry], entry);
  let scene = clone(document.nodes);
  if (document.designs?.length && metricsContext.designMode !== false)
    applyDesigns(scene, document, files, entry, read, metricsContext.designState);
  metricsContext.transformScene?.(scene);
  visit(scene, n => {
    const patch = metricsContext.instanceProps?.(n);
    if (patch) n.props = { ...n.props, ...patch };
  });
  const { link, definitions } = createLinker({ files, read, links });
  if (metricsContext.definitionsOnly) {
    for (const name of Object.keys(builtinSources)) link(name);
    for (const path of Object.keys(files)) {
      const match = /^components\/([A-Za-z_][\w]*)\.ui$/.exec(path);
      if (match) link(match[1]);
    }
    return { definitions: definitions(), fallback };
  }
  const { enums, tokens, i18n, styles } = sharedDeclarations(document, entry, files, read);
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
  const rawRootProps = scene[0]?.props ?? {};
  const vpW =
    typeof rawRootProps.width === 'number'
      ? rawRootProps.width
      : Number(metricsContext.viewport?.width ?? 360);
  const vpH =
    typeof rawRootProps.height === 'number'
      ? rawRootProps.height
      : Number(metricsContext.viewport?.height ?? 220);
  const viewport = { width: vpW, height: vpH, compact: vpW < 640 };
  const stateDefaults = Object.fromEntries(
    Object.entries(document.stateDefinitions ?? {})
      .filter(([, d]) => d.default !== undefined)
      .map(([k, d]) => [k, d.default]),
  );
  const environment = {
    enums,
    tokens,
    i18n,
    styles,
    stateDefaults,
    locale: metricsContext.locale ?? null,
    viewport,
  };
  const documentProps = { ...document.defaults, ...metricsContext.properties };
  for (const [key, definition] of Object.entries(document.propDefinitions ?? {}))
    if (definition.required && !own(documentProps, key))
      throw Error(`${document.name}: обязательное свойство ${key} не задано`);
  Object.assign(
    documentProps,
    selectedProperties(document.matches, documentProps, state, environment),
    metricsContext.properties,
  );
  validateContract(
    document.propDefinitions,
    Object.fromEntries(
      Object.entries(documentProps).map(([key, value]) => [
        key,
        evaluate(value, documentProps, state, [key], false, environment),
      ]),
    ),
    enums,
    document.name,
  );
  if (document.forward?.length)
    throw Error('forward применяется к дочернему элементу, не к объявлению component');
  const documentSources = {
    ...Object.fromEntries(
      Object.entries(document.defaultRanges ?? {}).map(([key, range]) => [
        key,
        { file: entry, ...range, label: document.name },
      ]),
    ),
    ...selectedPropertySources(document.matches, documentProps, state, environment),
  };
  scene = expandStructure(scene, documentProps, state, environment, {
    trackOrigins: true,
    propertySources: documentSources,
    recursive: node =>
      containerTypes.has(node.type) || node.type === 'Scroll' || node.type === 'Modal',
  });
  const instanceTree = createElementTree(scene);
  visit(scene, n => {
    if (Object.keys(n.slots ?? {}).length)
      throw Error('override объявляется в наследнике component');
  });
  if (scene.length !== 1 || !containerTypes.has(scene[0].type))
    throw Error('Ожидается один корневой Frame, Row, Column, Grid или Stack');
  let instances = [];
  const instantiate = createInstantiator({
    link,
    state,
    stateDefaults,
    enums,
    tokens,
    styles,
    viewport,
    i18n,
  });
  const visualNodes = [];
  const root = scene[0],
    rootProps = root.props;
  const modalNodes = sceneModalNodes(root),
    children = root.children.filter(n => n.type !== 'Modal');
  if (modalNodes.length && root.children.slice(-modalNodes.length).some(n => n.type !== 'Modal'))
    throw Error('Modal размещается после контролов сцены');
  const scroll = children.length === 1 && children[0].type === 'Scroll' ? children[0] : null;
  instances = scroll ? scroll.children : children;
  let rootWidth = constrained(length(rootProps.width ?? 360, 360, 360, 'width'), rootProps, 0, 360),
    rootHeight = constrained(
      length(rootProps.height ?? 220, 220, 220, 'height'),
      rootProps,
      1,
      220,
    );
  // `width: content` / `height: content` on the root fits the frame to what it holds, so a shell
  // region or popup is exactly as large as its controls instead of the 360×220 default.
  const { intrinsicScene, layoutScene, previewContainers } = createSceneLayout({
    root,
    instantiate,
    metrics,
    rootSize: axis => (axis === 0 ? rootWidth : rootHeight),
    onRootGrid: grid => {
      rootVisual.grid = grid;
    },
  });
  if (containerTypes.has(root.type)) {
    const fit = {
      ...root,
      children,
      layoutKind:
        root.type === 'Frame' && rootProps.columns === undefined && rootProps.rows === undefined
          ? 'Column'
          : root.type,
    };
    if (own(rootProps, 'width') && intrinsicLength(rootProps.width))
      rootWidth = naturalContainer(fit, 0, intrinsicScene);
    if (own(rootProps, 'height') && intrinsicLength(rootProps.height))
      rootHeight = naturalContainer(fit, 1, intrinsicScene);
  }
  const modals = sceneModals(modalNodes, rootWidth, rootHeight);
  const rootVisual = {
    id: -1,
    parent: null,
    control: -1,
    type: root.type,
    source: root.source,
    propertySources: root.propertySources,
    props: clone(rootProps),
    bounds: [0, 0, rootWidth, rootHeight],
  };
  let modern = root.type !== 'Frame';
  for (const n of [...instances, ...modals.flatMap(m => m.controls)])
    if (
      containerTypes.has(n.type) ||
      ['minWidth', 'maxWidth', 'minHeight', 'maxHeight'].some(key => own(n.props, key)) ||
      ['width', 'height'].some(key => own(n.props, key) && intrinsicLength(n.props[key]))
    )
      modern = true;
  const previewNodes = scene;
  if (modern) {
    const output = [];
    layoutScene(
      scroll ? { ...scroll, type: 'Column' } : { ...root, children },
      [0, 0, rootWidth, rootHeight],
      output,
    );
    instances = output;
    for (const modal of modals) {
      const laid = [];
      layoutScene({ ...modal.node, type: 'Column', props: {} }, modal.box, laid);
      modal.controls = laid.map(n => ({
        ...n,
        props: { ...n.props, x: n.props.x - modal.box[0], y: n.props.y - modal.box[1] },
      }));
    }
    rootProps.width = rootWidth;
    rootProps.height = rootHeight;
    rootProps.padding = 0;
    rootProps.gap = 0;
    for (const key of ['columns', 'rows', 'minWidth', 'maxWidth', 'minHeight', 'maxHeight'])
      delete rootProps[key];
  } else if (rootProps.columns !== undefined || rootProps.rows !== undefined) {
    const pad = insets(rootProps.padding),
      g = gap(rootProps);
    const resolvedChildren = instances.map(n => {
      const control = clone(n);
      for (const key of ['cell', 'row', 'column']) delete control.props[key];
      const { resolved } = instantiate(control, true);
      return { ...n, props: { ...n.props, width: resolved.width, height: resolved.height } };
    });
    const layout = { ...root, children: resolvedChildren };
    const cols = sizes(layout, 0, Math.max(0, rootWidth - pad[1] - pad[3]), metrics),
      rows = sizes(layout, 1, Math.max(0, rootHeight - pad[0] - pad[2]), metrics);
    rootVisual.grid = {
      bounds: [pad[3], pad[0], rootWidth - pad[1] - pad[3], rootHeight - pad[0] - pad[2]],
      columns: cols,
      rows,
      gap: g,
    };
    instances.forEach(n => {
      const ci = cell(n, 'columns', cols.length),
        ri = cell(n, 'rows', rows.length);
      n.props.x =
        pad[3] + (ci === null ? 0 : cols.slice(0, ci).reduce((a, b) => a + b, 0) + ci * g[1]);
      n.props.y =
        pad[0] + (ri === null ? 0 : rows.slice(0, ri).reduce((a, b) => a + b, 0) + ri * g[0]);
      if (n.props.width === undefined)
        n.props.width = ci === null ? rootVisual.grid.bounds[2] : cols[ci];
      if (n.props.height === undefined)
        n.props.height = ri === null ? rootVisual.grid.bounds[3] : rows[ri];
      for (const key of ['cell', 'row', 'column']) delete n.props[key];
    });
    delete rootProps.columns;
    delete rootProps.rows;
    rootProps.gap = 0;
  }
  const layered = modals.flatMap(m => m.controls);
  if (
    instances.length + layered.length > 1024 ||
    [...instances, ...layered].some(n => ['Frame', 'Scroll', 'Modal'].includes(n.type))
  )
    throw Error('Frame принимает 0…1024 контролов либо один Scroll с контролами');
  visualNodes.push(rootVisual);
  const compileInstance = createInstanceCompiler({
    document,
    enums,
    files,
    instantiate,
    metrics,
    visualNodes,
  });
  const controls = [...instances, ...layered].map(compileInstance);
  const templateTree = createElementTree(controls.flatMap(c => c.treeRoots));
  const sourceControls = controls.slice(0, instances.length).map(c => c.sourceNode);
  let offset = instances.length;
  const sourceModals = modals.map(modal => {
    const count = modal.controls.length,
      children = controls.slice(offset, offset + count).map(c => c.sourceNode);
    offset += count;
    const p = modal.node.props,
      props = {
        ...(p.key !== undefined ? { key: p.key } : {}),
        x: modal.box[0],
        y: modal.box[1],
        width: modal.box[2],
        height: modal.box[3],
        backdrop: { expr: modal.backdrop },
        dismiss: p.dismiss ?? true,
        open: p.open ?? true,
      };
    return { type: 'Modal', props, events: { ...modal.node.events }, children, bindings: {} };
  });
  const sourceScene = [
    {
      ...root,
      type: 'Frame',
      children: [
        ...(scroll ? [{ ...scroll, children: sourceControls }] : sourceControls),
        ...sourceModals,
      ],
    },
  ];
  // Byte-length framing keeps arbitrary Unicode and quoted template text intact.
  const template = frameTemplates(controls);
  const shortcuts = (document.shortcuts ?? []).map(s => ({
    ...s,
    args: (s.args ?? []).map(a => evaluate(a, documentProps, state, [], false, environment)),
    argExpressions: s.args ?? [],
  }));
  return {
    source: `component ${document.name} { ${sourceScene.map(serialize).join(' ')} }`,
    template,
    instanceTree,
    templateTree,
    visualNodes,
    previewNodes,
    previewControls: [...instances, ...layered],
    previewModals: modals.map(m => m.node),
    previewContainers,
    shortcuts,
  };
}
