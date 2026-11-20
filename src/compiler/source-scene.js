// @ts-check
// The scene as the runtime receives it: one Frame holding the compiled controls, with the modal
// layers after them.

/**
 * @param {object} root the root node of the scene
 * @param {object | null} scroll the Scroll that wraps the controls, when the scene has one
 * @param {Array<{ node: object, box: number[], backdrop: string, controls: object[] }>} modals
 * @param {Array<{ sourceNode: object }>} controls compiled controls: the scene's own first, then
 *   each modal's in order
 * @param {number} instanceCount how many of `controls` belong to the scene itself
 */
export function sourceSceneOf(root, scroll, modals, controls, instanceCount) {
  const sourceControls = controls.slice(0, instanceCount).map(c => c.sourceNode);
  let offset = instanceCount;
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
  return [
    {
      ...root,
      type: 'Frame',
      children: [
        ...(scroll ? [{ ...scroll, children: sourceControls }] : sourceControls),
        ...sourceModals,
      ],
    },
  ];
}
