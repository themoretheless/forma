export {
  validateDesign,
  designStatePatch,
  formatSource,
  lintDocument,
} from '@themoretheless/tokenizer-forma/language';
import {
  parse as parseSource,
  resolve as resolveValue,
} from '@themoretheless/tokenizer-forma/language';
import { readDesignData } from './design-data.js';
import { stampStableIds } from './element-tree.js';

/**
 * One choice of a radio group, `value: 'a'; group <-> state.choice;`: it is checked while the
 * group holds its value, and choosing it writes that value. The runtime does the same in
 * `form_document/compiler/json.rs`. `value` is not a property of the control, so it moves to
 * `groupValue`; the binding stays for the write-back.
 */
function bindGroupChoices(nodes) {
  for (const node of nodes ?? []) {
    const path = node.bindings?.group;
    if (typeof path === 'string' && !node.groupBound) {
      const value = node.props.value ?? null;
      delete node.props.value;
      node.props.checked = {
        expression: { kind: 'binary', operator: '==', left: { expr: path }, right: value },
      };
      node.groupValue = value;
      node.groupBound = true;
    }
    bindGroupChoices(node.children);
    bindGroupChoices(node.elseChildren);
    bindGroupChoices(node.emptyChildren);
    for (const branch of node.branches ?? []) bindGroupChoices(branch.children);
  }
}

/** Parses `.ui` source with the parser's own options, then binds the choices of radio groups. */
export function parse(source, options) {
  const doc = options === undefined ? parseSource(source) : parseSource(source, options);
  bindGroupChoices(doc.nodes);
  stampStableIds(doc.nodes, options?.file ?? '');
  return doc;
}
export const resolve = (value, state) => resolveValue(value, state, readDesignData);
const recent = new Map();
export function parseShared(source) {
  let doc = recent.get(source);
  if (doc) {
    recent.delete(source);
    recent.set(source, doc);
    return doc;
  }
  doc = parse(source);
  recent.set(source, doc);
  if (recent.size > 4) recent.delete(recent.keys().next().value);
  return doc;
}
