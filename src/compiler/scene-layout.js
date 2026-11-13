// Laying out the controls of a scene: what size each control takes and where its container puts it.

import {
  arrange,
  constrained,
  containerTypes,
  intrinsicLength,
  length,
  naturalContainer,
} from '../component-layout.js';
import { cell, gap, insets, natural, sizes } from './content-layout.js';
import { clone } from './data.js';

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
  function layoutScene(node, box, output, frame = [0, 0, 0, 0]) {
    if (!containerTypes.has(node.type)) {
      // A flow container measures a child's own `x`/`y` from its outer corner and percentages
      // against its outer size, so a tool that writes a coordinate needs that box next to the drawn
      // one. The page corner is just the case where the two frames coincide.
      output.push({
        ...node,
        props: { ...node.props, x: box[0], y: box[1], width: box[2], height: box[3] },
        coordinateBox: frame,
      });
      return;
    }
    if (node !== root && node.props.clip)
      throw Error(
        'Обрезка вложенного контейнера сцены пока не поддерживается; используйте clip корневого Frame',
      );
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
      layoutScene(child, layout.boxes[index], output, box);
    });
  }
  return { intrinsicScene, layoutScene, previewContainers };
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
