// Component linking is shared by Studio preview and native launch. It emits the
// same concrete primitive template; there is no DOM rendering of button content.
import { parse } from './language.js';
import { attachSources, createElementTree, stampStableIds } from './element-tree.js';
import { createCacheBudget } from './cache-budget.js';
import { evaluate, expandStructure } from './component-semantics.js';
import { containerTypes, gaps, padding } from './component-layout.js';
import { frameTemplates, serialize } from './component-template.js';
import { clone, visit } from './compiler/data.js';
import { cachingMetrics } from './compiler/metrics-cache.js';
import { applyDesigns } from './compiler/design-patch.js';
import { createLinker } from './compiler/link.js';
import { createInstanceCompiler } from './compiler/instance.js';
import {
  createSceneLayout,
  declaredRootSize,
  fitRootToContent,
  needsFlowLayout,
  placeOnRootGrid,
} from './compiler/scene-layout.js';
import { createInstantiator } from './compiler/instantiate.js';
import { addComponentDeclarations, sharedDeclarations } from './compiler/declarations.js';
import { documentProperties, evaluationEnvironment } from './compiler/environment.js';
import { sceneModals, sceneParts } from './compiler/modals.js';
import { sourceSceneOf } from './compiler/source-scene.js';
import { builtinSources, fallback } from './compiler/builtins.js';
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
  addComponentDeclarations({ enums, tokens, i18n, styles }, scene, link);
  const { viewport, stateDefaults, environment } = evaluationEnvironment(
    document,
    scene[0]?.props ?? {},
    { enums, tokens, i18n, styles },
    metricsContext,
  );
  const { documentProps, documentSources } = documentProperties(
    document,
    entry,
    state,
    environment,
    metricsContext.properties,
  );
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
  stampStableIds(scene);
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
  const { modalNodes, children, scroll, controls: sceneControls } = sceneParts(root);
  instances = sceneControls;
  let [rootWidth, rootHeight] = declaredRootSize(rootProps);
  // `width: content` / `height: content` on the root fits the frame to what it holds, so a shell
  // region or popup is exactly as large as its controls instead of the 360×220 default.
  const { intrinsicScene, layoutScene, previewContainers, scrollAreas } = createSceneLayout({
    root,
    instantiate,
    metrics,
    rootSize: axis => (axis === 0 ? rootWidth : rootHeight),
    onRootGrid: grid => {
      rootVisual.grid = grid;
    },
  });
  fitRootToContent(root, children, intrinsicScene, (axis, size) => {
    if (axis === 0) rootWidth = size;
    else rootHeight = size;
  });
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
  const modern = needsFlowLayout(root, [...instances, ...modals.flatMap(m => m.controls)]);
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
  } else if (rootProps.columns !== undefined || rootProps.rows !== undefined)
    rootVisual.grid = placeOnRootGrid({
      root,
      instances,
      instantiate,
      metrics,
      rootWidth,
      rootHeight,
    });
  else {
    if (rootProps.padding !== undefined) rootProps.padding = padding(rootProps.padding);
    if (rootProps.gap !== undefined) rootProps.gap = gaps(rootProps.gap)[0];
    if (rootProps.clip !== undefined && typeof rootProps.clip !== 'boolean')
      throw Error('Frame.clip ожидает boolean');
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
  const controlList = [...instances, ...layered];
  const controlOrigins = controlList.map(n => n.origin ?? n.stableId ?? '');
  const controls = controlList.map(compileInstance);
  const templateTree = createElementTree(controls.flatMap(c => c.treeRoots));
  const sourceScene = sourceSceneOf(root, scroll, modals, controls, instances.length);
  // Byte-length framing keeps arbitrary Unicode and quoted template text intact.
  const template = frameTemplates(controls);
  const shortcuts = (document.shortcuts ?? []).map(s => ({
    ...s,
    args: (s.args ?? []).map(a => evaluate(a, documentProps, state, [], false, environment)),
    argExpressions: s.args ?? [],
  }));
  const sourceMap = {
    entry,
    root: {
      file: root.source?.file ?? entry,
      from: root.source?.from ?? root.start ?? 0,
      to: root.source?.to ?? root.end ?? root.start ?? 0,
      propertySources: root.propertySources ?? {},
    },
    controls: controlList.map((n, index) => ({
      index,
      key: typeof n.props?.key === 'string' ? n.props.key : null,
      origin: controlOrigins[index],
      file: n.source?.file ?? entry,
      from: n.source?.from ?? n.start ?? 0,
      to: n.source?.to ?? n.end ?? n.start ?? 0,
      propertySources: n.propertySources ?? {},
      definition: controls[index]?.treeRoots?.[0]?.source ?? null,
    })),
  };
  return {
    source: `component ${document.name} { ${sourceScene.map(serialize).join(' ')} }`,
    template,
    instanceTree,
    templateTree,
    visualNodes,
    previewNodes,
    previewControls: controlList,
    controlOrigins,
    sourceMap,
    controlClips: controlList.map(n => n.clip ?? null),
    // The scroll areas around each control, outermost first, as the runtime takes them.
    controlScrolls: controlList.map(n =>
      (n.scroll ?? []).map(({ area, viewport }) => ({
        area,
        viewport,
        content: scrollAreas[area].content,
      })),
    ),
    previewModals: modals.map(m => m.node),
    previewContainers,
    shortcuts,
  };
}
