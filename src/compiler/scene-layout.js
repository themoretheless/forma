// @ts-check
// Laying out the controls of a scene: what size each control takes and where its container puts it.

import {
  arrange,
  constrained,
  containerTypes,
  intrinsicLength,
  length,
  naturalContainer,
} from './component-layout.js';
import { cell, gap, insets, natural, sizes } from './content-layout.js';
import { own } from '../shared/object.js';
import { clone } from './data.js';
import { containerArea } from './builtins.js';

/**
 * @param {{ root: object, instantiate: Function, metrics: object,
 *   rootSize: (axis: 0 | 1) => number, onRootGrid: (grid: object) => void }} scene
 *   `rootSize` is asked on every measurement because a root sized by its content is still being
 *   measured when the first controls are; `onRootGrid` receives the grid of the root container
 */
export function createSceneLayout({ root, instantiate, metrics, rootSize, onRootGrid }) {
  const intrinsicCache = new WeakMap();
  // A container paints no box of its own, but the layout still measured one for it, and that box is
  // what a designer means when they pick up a panel. It travels beside the drawn controls so the
  // canvas can measure, grab and move a container by the extent it actually has.
  const previewContainers = [];
  // `Scroll` containers below the root, by number: viewport and content size. Each control
  // inside one lists the ones around it, outermost first, as `{ area, viewport }`.
  const scrollAreas = [];
  let scrollChain = [];
  // The `clicked` handler of the nearest container: a control without its own answers with it.
  let bubble = null;
  function intrinsicScene(node, axis) {
    const key = axis === 0 ? 'width' : 'height',
      value = node.props[key];
    if (value !== undefined && !intrinsicLength(value) && !/[*%]$/.test(value?.expr ?? ''))
      return constrained(length(value, rootSize(axis), 0), node.props, axis, rootSize(axis));
    if (containerTypes.has(node.type))
      return naturalContainer(
        {
          ...node,
          layoutKind:
            node.type === 'Frame' &&
            node.props.columns === undefined &&
            node.props.rows === undefined
              ? 'Column'
              : node.type,
        },
        axis,
        intrinsicScene,
      );
    let inst = intrinsicCache.get(node);
    if (!inst) {
      inst = instantiate(node, true);
      intrinsicCache.set(node, inst);
    }
    const { roots, resolved } = inst;
    if (intrinsicLength(resolved[key]))
      return constrained(natural(roots[0], axis, metrics), node.props, axis, rootSize(axis));
    return constrained(length(resolved[key], rootSize(axis), 0), node.props, axis, rootSize(axis));
  }
  function layoutScene(node, box, output, frame = [0, 0, 0, 0], clip = null) {
    if (node.type === 'Scroll' && node !== root) return layoutScrollArea(node, box, output, clip);
    if (!containerTypes.has(node.type)) {
      if (bubble && !node.events?.clicked)
        node = {
          ...node,
          events: { ...node.events, clicked: bubble.clicked },
          eventArgs: node.eventArgs ?? bubble.eventArgs,
        };
      // A flow container measures a child's own `x`/`y` from its outer corner and percentages
      // against its outer size, so a tool that writes a coordinate needs that box next to the drawn
      // one. The page corner is just the case where the two frames coincide.
      output.push({
        ...node,
        props: { ...node.props, x: box[0], y: box[1], width: box[2], height: box[3] },
        coordinateBox: frame,
        ...(clip ? { clip } : {}),
        ...(scrollChain.length ? { scroll: scrollChain } : {}),
      });
      return;
    }
    // A clipping container below the root narrows what its controls may show; the runtime
    // receives the rectangle per control, since the scene markup has no containers.
    const outerBubble = bubble;
    const capture = node.props.capture;
    if (capture !== undefined && typeof capture !== 'boolean')
      throw Error(`${node.type}.capture ожидает boolean`);
    if (capture && !node.events?.clicked)
      throw Error(`${node.type}.capture требует clicked на том же контейнере`);
    // With `capture: true` the area goes over the children instead, taking every click inside.
    let capturedArea = null;
    if (node.events?.clicked) {
      // Clicks on the container's empty space land on an invisible pressable area under its
      // children; children without a handler of their own answer with the container's.
      const area = {
        ...node,
        type: containerArea,
        stableId:
          typeof node.props.key === 'string' && node.props.key.length
            ? `key:${node.props.key}`
            : node.stableId
              ? `${node.stableId}/ContainerArea#0`
              : '',
        props: node.props.key === undefined ? {} : { key: node.props.key },
        bindings: {},
        children: [],
      };
      if (capture) capturedArea = area;
      else layoutScene(area, box, output, frame, clip);
      bubble = { clicked: node.events.clicked, eventArgs: node.eventArgs };
    }
    if (node !== root && node.props.clip !== undefined && typeof node.props.clip !== 'boolean')
      throw Error(`${node.type}.clip ожидает boolean`);
    if (node !== root && node.props.clip) clip = clip ? intersect(clip, box) : [...box];
    let layout;
    if (
      node.type === 'Frame' &&
      (node.props.columns !== undefined || node.props.rows !== undefined)
    ) {
      const pad = insets(node.props.padding),
        g = gap(node.props),
        inside = [
          box[0] + pad[3],
          box[1] + pad[0],
          Math.max(0, box[2] - pad[1] - pad[3]),
          Math.max(0, box[3] - pad[0] - pad[2]),
        ];
      const measured = {
        ...node,
        children: node.children.map(child => ({
          ...child,
          props: {
            ...child.props,
            width: intrinsicScene(child, 0),
            height: intrinsicScene(child, 1),
          },
        })),
      };
      const cols = sizes(measured, 0, inside[2], metrics),
        rows = sizes(measured, 1, inside[3], metrics);
      layout = {
        grid: { bounds: inside, columns: cols, rows, gap: g },
        boxes: node.children.map(child => {
          const ci = cell(child, 'columns', cols.length),
            ri = cell(child, 'rows', rows.length);
          return [
            inside[0] +
              (ci === null ? 0 : cols.slice(0, ci).reduce((a, b) => a + b, 0) + ci * g[1]),
            inside[1] +
              (ri === null ? 0 : rows.slice(0, ri).reduce((a, b) => a + b, 0) + ri * g[0]),
            ci === null ? inside[2] : cols[ci],
            ri === null ? inside[3] : rows[ri],
          ];
        }),
      };
    } else layout = arrange(node, box, intrinsicScene, { scene: true });
    if (node === root && layout.grid) onRootGrid(layout.grid);
    node.children.forEach((child, index) => {
      if (containerTypes.has(child.type))
        previewContainers.push({
          start: child.start,
          bounds: layout.boxes[index],
          coordinateBox: box,
        });
      layoutScene(child, layout.boxes[index], output, box, clip);
    });
    bubble = outerBubble;
    if (capturedArea) layoutScene(capturedArea, box, output, frame, clip);
  }
  /**
   * A `Scroll` below the root: its children are laid out like a column's, free to run past its
   * box, which becomes a viewport that clips them and scrolls on its own.
   */
  function layoutScrollArea(node, box, output, clip) {
    const area = scrollAreas.length;
    scrollAreas.push({ viewport: [...box], content: [box[2], box[3]] });
    const outer = scrollChain,
      first = output.length,
      firstContainer = previewContainers.length;
    scrollChain = [...outer, { area, viewport: [...box] }];
    layoutScene(
      { ...node, type: 'Column', props: { ...node.props, clip: undefined } },
      box,
      output,
      box,
      clip ? intersect(clip, box) : [...box],
    );
    scrollChain = outer;
    // How far the containers and controls inside reach from the viewport's corner, never less
    // than it: the same extent the Rust document measures.
    let [right, bottom] = [box[0] + box[2], box[1] + box[3]];
    const boxes = [
      ...previewContainers.slice(firstContainer).map(c => c.bounds),
      ...output.slice(first).map(({ props: p }) => [p.x, p.y, p.width, p.height]),
    ];
    for (const [x, y, w, h] of boxes) {
      right = Math.max(right, x + w);
      bottom = Math.max(bottom, y + h);
    }
    scrollAreas[area].content = [right - box[0], bottom - box[1]];
  }
  return { intrinsicScene, layoutScene, previewContainers, scrollAreas };
}

/**
 * Places the controls of a root Frame with `columns` or `rows` and no nested containers: writes
 * `x`, `y` and the missing sizes into each control and clears the grid properties of the root.
 *
 * @returns {{ bounds: number[], columns: number[], rows: number[], gap: number[] }} the root grid
 */
export function placeOnRootGrid({ root, instances, instantiate, metrics, rootWidth, rootHeight }) {
  const rootProps = root.props;
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
  const grid = {
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
    if (n.props.width === undefined) n.props.width = ci === null ? grid.bounds[2] : cols[ci];
    if (n.props.height === undefined) n.props.height = ri === null ? grid.bounds[3] : rows[ri];
    for (const key of ['cell', 'row', 'column']) delete n.props[key];
  });
  delete rootProps.columns;
  delete rootProps.rows;
  rootProps.gap = 0;
  return grid;
}

/**
 * Whether the scene is laid out by its containers. A plain root Frame of fixed-size controls keeps
 * the positions it was given; anything that needs measuring switches the whole scene to layout.
 *
 * @param {object} root
 * @param {object[]} controls the scene's controls together with those of its modal layers
 */
export function needsFlowLayout(root, controls) {
  return (
    root.type !== 'Frame' ||
    controls.some(
      n =>
        containerTypes.has(n.type) ||
        ['minWidth', 'maxWidth', 'minHeight', 'maxHeight'].some(key => own(n.props, key)) ||
        ['width', 'height'].some(key => own(n.props, key) && intrinsicLength(n.props[key])),
    )
  );
}

/** The size a root declares, or 360 by 220 when it declares none. */
export function declaredRootSize(rootProps) {
  const width = constrained(length(rootProps.width ?? 360, 360, 360, 'width'), rootProps, 0, 360);
  const height = constrained(
    length(rootProps.height ?? 220, 220, 220, 'height'),
    rootProps,
    1,
    220,
  );
  if (width <= 0 || width > 16384 || height <= 0 || height > 16384)
    throw Error('Размер корневого контейнера вне допустимого диапазона 1…16384');
  return [width, height];
}

/**
 * `width: content` / `height: content` on the root fits the frame to what it holds, so a shell
 * region or popup is exactly as large as its controls instead of the default. The width is fitted
 * first and reported through `fit` before the height is measured, because the height of wrapped
 * content depends on it.
 *
 * @param {(axis: 0 | 1, size: number) => void} fit receives each fitted size
 */
export function fitRootToContent(root, children, intrinsicScene, fit) {
  if (!containerTypes.has(root.type)) return;
  const props = root.props;
  const content = {
    ...root,
    children,
    layoutKind:
      root.type === 'Frame' && props.columns === undefined && props.rows === undefined
        ? 'Column'
        : root.type,
  };
  if (own(props, 'width') && intrinsicLength(props.width)) {
    const w = naturalContainer(content, 0, intrinsicScene);
    if (w <= 0 || w > 16384)
      throw Error('Размер корневого контейнера вне допустимого диапазона 1…16384');
    fit(0, w);
  }
  if (own(props, 'height') && intrinsicLength(props.height)) {
    const h = naturalContainer(content, 1, intrinsicScene);
    if (h <= 0 || h > 16384)
      throw Error('Размер корневого контейнера вне допустимого диапазона 1…16384');
    fit(1, h);
  }
}

/** The part of rectangle `a` that `b` also covers, with zero size when they do not meet. */
function intersect(a, b) {
  const x = Math.max(a[0], b[0]),
    y = Math.max(a[1], b[1]);
  return [
    x,
    y,
    Math.max(0, Math.min(a[0] + a[2], b[0] + b[2]) - x),
    Math.max(0, Math.min(a[1] + a[3], b[1] + b[3]) - y),
  ];
}
