import {parse} from './language.js';

// Resolve against the current document, never a stale inspector node or a regex.
export function propertyEdit(source, nodeStart, property, value) {
  let target;
  const walk=nodes=>{for(const node of nodes){if(node.start===nodeStart)target=node;walk(node.children);}};
  walk(parse(source).nodes);
  const range=target?.propertyRanges?.[property];
  if(!range)throw Error('Свойство изменилось: выберите элемент заново');
  if(!['string','number','boolean'].includes(typeof value)||(typeof value==='number'&&!Number.isFinite(value)))throw Error('Некорректное значение свойства');
  const insert=typeof value==='string'?"'"+value.replace(/\\/g,'\\\\').replace(/'/g,"\\'").replace(/\n/g,'\\n')+"'":String(value);
  const next=source.slice(0,range.from)+insert+source.slice(range.to);
  parse(next); // Commit only syntactically valid replacements.
  return {...range,insert};
}
