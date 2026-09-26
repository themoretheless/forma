import {parse} from './language.js';
import {equalValues} from './expressions.js';

const bindingPath=/^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)+$/;
const handlerPath=/^(?:actions|events|state)\.[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*$/;
const staleTarget='Свойство изменилось: выберите элемент заново';

// Resolve against the current document, never a stale inspector node or a regex.
export function sourceNode(source,start){
  let found=null;
  const walk=nodes=>{for(const node of nodes??[]){if(found)continue;
    if(node.start===start){found=node;continue;}
    walk(node.children);walk(node.elseChildren);walk(node.emptyChildren);}};
  walk(parse(source).nodes);
  return found;
}
function propertyRange(source,nodeStart,property){
  const range=sourceNode(source,nodeStart)?.propertyRanges?.[property];
  if(!range)throw Error(staleTarget);
  return range;
}
export function propertyEdit(source,nodeStart,property,value){
  const range=propertyRange(source,nodeStart,property);
  if(!['string','number','boolean'].includes(typeof value)||(typeof value==='number'&&!Number.isFinite(value)))throw Error('Некорректное значение свойства');
  const insert=typeof value==='string'?"'"+value.replace(/\\/g,'\\\\').replace(/'/g,"\\'").replace(/\n/g,'\\n')+"'":String(value);
  const next=source.slice(0,range.from)+insert+source.slice(range.to);
  parse(next); // Commit only syntactically valid replacements.
  return {...range,insert};
}
// A computed value is written back as the markup text shown in the inspector, so it has
// to parse on its own in the same statement context and give the same data there as it
// does inside the file: otherwise a stray brace could reopen the node and still leave a
// parsable file with a different structure than the one that was selected.
export function valueTextEdit(source,nodeStart,property,text){
  const range=propertyRange(source,nodeStart,property);
  const insert=text.trim();
  if(!insert)throw Error('Пустое значение');
  let expected;
  try{expected=parse(`component __Probe {\n    Frame {\n        __value: ${insert};\n    }\n}`).nodes[0].props.__value;}
  catch(error){throw Error(`Значение не разобрано: ${error.message}`);}
  const next=source.slice(0,range.from)+insert+source.slice(range.to);
  parse(next);
  if(!equalValues(sourceNode(next,nodeStart)?.props[property],expected))throw Error('Значение разобралось иначе, чем в исходнике');
  return {...range,insert};
}
export function statementParts(source,range){
  const text=source.slice(range.from,range.to);
  const match=/^([^\s]+)\s*(<->|->|:)/.exec(text);
  if(!match)return null;
  // statementRanges always ends on the statement's own semicolon, so only that one
  // is dropped; a semicolon inside an object argument belongs to the value.
  return {key:match[1],op:match[2],rhs:text.slice(match[0].length).trim().replace(/;$/,'')};
}
// A binding or a handler is a whole statement rather than a value, so it is rewritten
// from statementRanges and checked back against the re-parsed document.
export function statementTextEdit(source,nodeStart,property,text){
  const range=sourceNode(source,nodeStart)?.statementRanges?.[property];
  if(!range)throw Error(staleTarget);
  const parts=statementParts(source,range);
  if(!parts||parts.op===':')throw Error('Оператор изменился: выберите элемент заново');
  const rhs=text.trim();
  const handler=rhs.replace(/\s*\(.*$/s,'').trim();
  if(parts.op==='<->'){
    if(!bindingPath.test(rhs))throw Error('Двусторонняя привязка требует путь к полю');
  }else if(!handlerPath.test(handler))throw Error('Обработчик требует путь actions.…, events.… или state.…');
  const insert=`${parts.key} ${parts.op} ${rhs};`;
  const next=source.slice(0,range.from)+insert+source.slice(range.to);
  parse(next);
  const after=sourceNode(next,nodeStart);
  if(parts.op==='<->'){
    if(after?.bindings[property]!==rhs)throw Error('Привязка разобралась иначе, чем в исходнике');
  }else if(after?.events[property]!==handler)throw Error('Обработчик разобрался иначе, чем в исходнике');
  return {from:range.from,to:range.to,insert};
}
