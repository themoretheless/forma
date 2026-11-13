// The modal layers of a scene: validated, and placed over the root frame.
import { color, num } from './content-layout.js';

const modalProps = new Set(['key', 'x', 'y', 'width', 'height', 'backdrop', 'dismiss', 'open']);

/**
 * @param {object[]} modalNodes the `Modal` nodes of the root, in source order
 * @param {number} rootWidth
 * @param {number} rootHeight a modal without `x` or `y` is centred in the root
 * @returns {Array<{ node: object, box: number[], backdrop: string, controls: object[] }>}
 */
export function sceneModals(modalNodes, rootWidth, rootHeight) {
  // A modal layer is a positioned surface over the whole frame. Children are
  // laid out inside it and emitted with coordinates relative to the surface.
  return modalNodes.map(node => {
    const p = node.props;
    for (const k of Object.keys(p))
      if (!modalProps.has(k)) throw Error(`Modal.${k} не поддерживается`);
    for (const [event, action] of Object.entries(node.events ?? {})) {
      if (event !== 'dismissed')
        throw Error(`Modal: обработчик ${event} не поддерживается; используйте dismissed`);
      if (!/^actions\.[A-Za-z_]\w*$/.test(action) || node.eventArgs?.[event]?.length)
        throw Error('Modal.dismissed требует actions.<имя>() без аргументов');
    }
    if (Object.keys(node.bindings ?? {}).length) throw Error('Modal не поддерживает привязки');
    if (p.key !== undefined && (typeof p.key !== 'string' || !p.key))
      throw Error('Modal.key ожидает непустую строку');
    for (const k of ['dismiss', 'open'])
      if (p[k] !== undefined && typeof p[k] !== 'boolean')
        throw Error(`Modal.${k} ожидает boolean`);
    const backdrop = color(p.backdrop ?? '#00000080');
    if (typeof backdrop !== 'string' || !/^#(?:[\da-f]{3}|[\da-f]{6}|[\da-f]{8})$/i.test(backdrop))
      throw Error('Modal.backdrop принимает hex-цвет');
    const width = num(p.width ?? 320, 'Modal.width'),
      height = num(p.height ?? 200, 'Modal.height');
    const x =
        p.x === undefined ? Math.max(0, Math.round((rootWidth - width) / 2)) : num(p.x, 'Modal.x'),
      y =
        p.y === undefined
          ? Math.max(0, Math.round((rootHeight - height) / 2))
          : num(p.y, 'Modal.y');
    for (const child of node.children)
      if (['Scroll', 'Modal'].includes(child.type)) throw Error(`Modal не содержит ${child.type}`);
    return { node, box: [x, y, width, height], backdrop, controls: node.children };
  });
}

function sceneModalNodes(rootOrChildren) {
  const children = Array.isArray(rootOrChildren)
    ? rootOrChildren
    : (rootOrChildren?.children ?? []);
  return children.filter(n => n.type === 'Modal');
}

/**
 * Splits the children of the root into the scene's own controls and its modal layers. A root
 * holding a single Scroll scrolls its controls; the Scroll is then not a control itself.
 *
 * @param {object} root
 */
export function sceneParts(root) {
  const modalNodes = sceneModalNodes(root),
    children = root.children.filter(n => n.type !== 'Modal');
  if (modalNodes.length && root.children.slice(-modalNodes.length).some(n => n.type !== 'Modal'))
    throw Error('Modal размещается после контролов сцены');
  const scroll = children.length === 1 && children[0].type === 'Scroll' ? children[0] : null;
  return { modalNodes, children, scroll, controls: scroll ? scroll.children : children };
}
