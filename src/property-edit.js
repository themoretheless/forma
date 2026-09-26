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
// Values arrive as markup text, so they are checked in a throwaway component that puts
// them exactly where a property value belongs.
function probeValue(text){return parse(`component __Probe {\n    Frame {\n        __value: ${text};\n    }\n}`).nodes[0].props.__value;}
function keptDeclarations(before,after){
  for(const [key,value]of Object.entries(before))if(key in after&&!equalValues(after[key],value))return false;
  return true;
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
  try{expected=probeValue(insert);}
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

// A node carries no range for its own block, so where a new statement goes is derived from
// what the block already holds: after the semicolon of its last declaration, or inside the
// braces when there is nothing to hang on to. Every statementRanges entry ends on its own
// semicolon, and the anchor is still bounded by the node's closing brace so a range that no
// longer matches the source cannot write into a sibling's block. A comment that trails the
// last declaration belongs to it, so the new statement goes after the comment. The indent of
// a statement that owns its line is copied so the file keeps the shape it was written in; an
// inline block (`Button { text: 'x'; }`, common in this markup) has none to copy, so one level
// past the node's own line is assumed and the closing brace moves onto a line of its own.
const trailingComments=/^(?:[ \t]*(?:\/\/[^\n]*|\/\*[\s\S]*?\*\/))+/;
const lineStartOf=(source,at)=>source.lastIndexOf('\n',at)+1;
function blockAnchor(source,node){
  const closing=node.end-1;
  const last=Object.values(node.statementRanges).reduce((a,b)=>!a||b.to>a.to?b:a,null);
  const at=last?last.to-1:source.indexOf('{',node.start);
  if(at<0||at>=closing)throw Error(staleTarget);
  const nodeStart=lineStartOf(source,node.start);
  const nodeIndent=/^[ \t]*/.exec(source.slice(nodeStart,node.start))[0];
  const statementStart=lineStartOf(source,last?.from??at);
  const own=last&&/^[ \t]*$/.test(source.slice(statementStart,last.from));
  const from=at+1+(trailingComments.exec(source.slice(at+1,closing))?.[0].length??0);
  // An inline block keeps its closing brace on the new statement's line, so the spaces before
  // the brace are cut rather than left as trailing whitespace on the statement line.
  const inline=/^[ \t]*\}$/.test(source.slice(from,closing+1));
  return {
    from,
    to:inline?closing:from,
    text:`\n${own?source.slice(statementStart,last.from):`${nodeIndent}    `}`,
    tail:inline?`\n${nodeIndent}`:'',
  };
}
const propertyName=/^[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*$/;
export function propertyAddEdit(source,nodeStart,property,text){
  const node=sourceNode(source,nodeStart);
  if(!node)throw Error(staleTarget);
  if(!propertyName.test(property))throw Error('Некорректное имя свойства');
  if(property in node.propertyRanges||property in node.statementRanges)throw Error(`Свойство ${property} уже объявлено`);
  const insert=text.trim();
  if(!insert)throw Error('Пустое значение');
  let expected;
  try{expected=probeValue(insert);}
  catch(error){throw Error(`Значение не разобрано: ${error.message}`);}
  const anchor=blockAnchor(source,node);
  const added=`${anchor.text}${property}: ${insert};${anchor.tail}`;
  const next=source.slice(0,anchor.from)+added+source.slice(anchor.to);
  parse(next);
  const after=sourceNode(next,nodeStart);
  if(!after||!equalValues(after.props[property],expected))throw Error('Свойство разобралось иначе, чем в исходнике');
  if(!keptDeclarations(node.props,after.props))throw Error('Соседние объявления изменились');
  return {from:anchor.from,to:anchor.to,insert:added};
}
// Removal is line-based on purpose: the statement has to own its line, otherwise cutting it
// out would leave a dangling comment or a second statement glued to the previous one, and the
// only honest answer there is to point the designer at the source.
export function propertyRemoveEdit(source,nodeStart,property){
  const node=sourceNode(source,nodeStart);
  if(!node)throw Error(staleTarget);
  const range=node.propertyRanges[property]??node.statementRanges[property];
  if(!range)throw Error(`Свойство ${property} не объявлено в этом исходнике`);
  const lineStart=source.lastIndexOf('\n',range.from)+1;
  let lineEnd=source.indexOf('\n',range.to);
  if(lineEnd<0)lineEnd=source.length;
  const line=source.slice(lineStart,lineEnd);
  const prefix=line.slice(0,range.from-lineStart),suffix=line.slice(range.to-lineStart);
  // A value token stops before its semicolon, so for a property the whole prefix must be the
  // name and its colon; a binding or handler range already spans the statement.
  const declaredName=/^\s*([\w.]+)\s*:\s*$/.exec(prefix)?.[1];
  const ownsLine=property in node.propertyRanges
    ?declaredName===property&&/^\s*;?\s*(?:(?:\/\/[^\n]*|\/\*[\s\S]*?\*\/)\s*)*$/.test(suffix)
    :/^\s*$/.test(prefix)&&/^\s*(?:(?:\/\/[^\n]*|\/\*[\s\S]*?\*\/)\s*)*$/.test(suffix);
  if(!ownsLine)throw Error(`${property} не на отдельной строке: правьте исходник`);
  const cutEnd=lineEnd<source.length?lineEnd+1:lineStart>0&&source[lineStart-1]==='\n'?lineStart:lineEnd;
  const cutStart=cutEnd===lineEnd?lineStart>0&&source[lineStart-1]==='\n'?lineStart-1:lineStart:lineStart;
  const next=source.slice(0,cutStart)+source.slice(cutEnd);
  parse(next);
  const after=sourceNode(next,nodeStart);
  if(!after||property in after.props)throw Error('Свойство не удалилось: выберите элемент заново');
  if(!keptDeclarations(node.props,after.props))throw Error('Соседние объявления изменились');
  return {from:cutStart,to:cutEnd,insert:''};
}
