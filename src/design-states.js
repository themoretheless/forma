import {parse} from './language.js';
import {equalValues} from './expressions.js';
import {blockAnchor,lineSpan,lineStartOf,propertyName,probeValue,propertyAddEdit,stringLiteral,valueTextEdit} from './property-edit.js';

// The states panel writes a `.design.ui` file the way the inspector writes markup: every function
// here takes the current source and returns a {from,to,insert} splice for the caller to commit.
// A `null` state name means the file's base overrides, which the panel shows as the default column.
const stale='Дизайн изменился: выберите состояние заново';
const apply=(source,edit)=>source.slice(0,edit.from)+edit.insert+source.slice(edit.to);
function designDoc(source){
  const doc=parse(source);
  if(!doc.designBody)throw Error('Файл не является дизайн-файлом');
  return doc;
}
function stateOf(doc,name){
  const state=doc.states.find(other=>other.name===name);
  if(!state)throw Error(`Состояние ${name} не объявлено`);
  return state;
}
// A block is the pair of braces one list of entries lives in: the design body holds the base, a
// `state` block its own. Its item ranges are what blockAnchor hangs a new entry on.
function blockOf(doc,stateName){
  if(stateName===null)return {label:null,start:doc.designBody.from,end:doc.designBody.to+1,nodes:doc.entries,overrideTypes:doc.overrideTypes,items:doc.entries.map(itemOf)};
  const state=stateOf(doc,stateName);
  return {label:state.name,start:state.start,end:state.end,nodes:state.nodes,overrideTypes:state.overrideTypes,items:state.nodes.map(itemOf)};
}
const itemOf=node=>({from:node.start,to:node.end});
const entriesOf=(source,list)=>list.map(node=>[node.type,node.props.key,Object.entries(node.propertyRanges).map(([name,range])=>[name,source.slice(range.from,range.to)])]);
// A splice on a stale offset, a colliding name or a stray brace that swallows a sibling all leave a
// parsable file behind, so each edit is also checked against this description: every block with the
// entries it holds — their type, key, and the text of every value. Positions stay out of it, since
// text moves as blocks come and go while its content does not.
const reading=(source,doc,mutate=list=>list)=>JSON.stringify(mutate([[null,entriesOf(source,doc.entries)],...doc.states.map(state=>[state.name,entriesOf(source,state.nodes)])]));
// The result has to read as the old file plus exactly what the edit promised.
const readsAs=(source,doc,next,expect)=>{if(reading(next,parse(next))!==reading(source,doc,expect))throw Error(stale);};
// And, for an edit that adds something, dropping that something from the result has to give the old
// file back, so no neighbouring entry can change along with it.
const keepsOthers=(source,doc,next,drop)=>{if(reading(next,parse(next),drop)!==reading(source,doc))throw Error(stale);};
const withoutEntry=(list,label,key)=>list.map(([name,entries])=>[name,name===label?entries.filter(entry=>entry[1]!==key):entries]);
const withoutState=(list,label)=>list.filter(([name])=>name!==label);
const renamedState=(list,label,name)=>list.map(([other,entries])=>[other===label?name:other,entries]);
const withState=(list,name)=>[...list,[name,[]]];

export function findState(source,name){return parse(source).states.find(state=>state.name===name)??null;}
// A reader for surfaces that have no panel: the base block and every state, each entry with its
// type, its key and the text of its values. Values are quoted straight from the file because a
// design value keeps its own spelling — `'Текст'`, `#e8edf7`, `design.titleSize` — and re-quoting
// it here would either break the round trip or push a caller into guessing how to write it back.
export function designStatesSummary(source){
  const doc=designDoc(source);
  const entry=node=>({type:node.type,key:node.props.key,properties:Object.fromEntries(Object.entries(node.propertyRanges)
    .filter(([name])=>name!=='key')
    .map(([name,range])=>[name,source.slice(range.from,range.to)]))});
  return {base:doc.entries.map(entry),states:doc.states.map(state=>({name:state.name,entries:state.nodes.map(entry)}))};
}

// A state the file never named has no entries, which is what the panel marks as not overridden.
export function findEntry(source,stateName,key){
  const doc=parse(source);
  const nodes=stateName===null?doc.entries:doc.states.filter(state=>state.name===stateName).flatMap(state=>state.nodes);
  return nodes.find(node=>node.props.key===key)??null;
}

// A state name goes into a quoted literal, so anything the markup escapes is allowed in it.
function stateLabel(name,doc,current){
  const label=String(name??'').trim();
  if(!label)throw Error('Имя состояния должно быть непустым');
  if(doc.states.some(state=>state!==current&&state.name===label))throw Error(`Состояние ${label} уже объявлено`);
  return label;
}
// A new block goes after everything the file already holds, base entries and states alike, at the
// indent those use. The blank line between blocks is the panel's separator, so an empty file gains
// none and the closing brace of an inline body moves onto a line of its own.
export function stateCreateEdit(source,name){
  const doc=designDoc(source);
  const label=stateLabel(name,doc);
  const anchor=blockAnchor(source,{start:doc.designBody.from,end:doc.designBody.to+1,items:[...doc.entries,...doc.states].map(itemOf)});
  const filled=doc.entries.length||doc.states.length;
  const edit={from:anchor.from,to:anchor.to,insert:`${filled?'\n':''}${anchor.text}state ${stringLiteral(label)} {\n${anchor.indent}}${anchor.tail}`};
  readsAs(source,doc,apply(source,edit),list=>withState(list,label));
  return edit;
}
export function stateRenameEdit(source,name,newName){
  const doc=designDoc(source);
  const state=stateOf(doc,name);
  const label=stateLabel(newName,doc,state);
  const edit={from:state.nameStart,to:state.nameEnd,insert:stringLiteral(label)};
  readsAs(source,doc,apply(source,edit),list=>renamedState(list,state.name,label));
  return edit;
}
// States the panel wrote are separated by a blank line, so the one above the block goes with it:
// cutting only the block would leave two separators between the neighbours.
function separatorAbove(source,at){
  const start=lineStartOf(source,at-2);
  return /^[ \t]*$/.test(source.slice(start,at-1))?start:at;
}
export function stateDeleteEdit(source,name){
  const doc=designDoc(source);
  const state=stateOf(doc,name);
  const span=lineSpan(source,state.start,state.end);
  const edit={from:Math.min(span.cutFrom,separatorAbove(source,span.lineStart)),to:span.cutTo,insert:''};
  readsAs(source,doc,apply(source,edit),list=>withoutState(list,state.name));
  return edit;
}
// Values arrive as markup text, so the routes are the inspector's: rewrite a value the entry already
// declares, declare one more property on it, or write the entry the state never mentioned.
export function statePropertyEdit(source,stateName,type,key,property,text){
  const doc=designDoc(source);
  const block=blockOf(doc,stateName);
  const expected=block.overrideTypes[key]??doc.overrideTypes[key];
  if(expected&&expected!==type)throw Error(`Тип дизайн-key ${key}: ожидался ${expected}, получен ${type}`);
  const entry=block.nodes.find(node=>node.props.key===key);
  if(entry)return property in entry.propertyRanges
    ?valueTextEdit(source,entry.start,property,text)
    :propertyAddEdit(source,entry.start,property,text);
  if(!/^[A-Z]\w*$/.test(type))throw Error('Некорректное имя типа');
  if(!propertyName.test(property))throw Error('Некорректное имя свойства');
  const probe=probeValue(text);
  const anchor=blockAnchor(source,block);
  const inner=anchor.indent+'    ';
  const added=`${anchor.text}${type} {\n${inner}key: ${stringLiteral(key)};\n${inner}${property}: ${text.trim()};\n${anchor.indent}}${anchor.tail}`;
  const edit={from:anchor.from,to:anchor.to,insert:added};
  const next=apply(source,edit);
  const after=parse(next);
  const created=blockOf(after,stateName).nodes.find(node=>node.props.key===key);
  if(!created||created.type!==type||!equalValues(created.props[property],probe))throw Error(stale);
  keepsOthers(source,doc,next,list=>withoutEntry(list,block.label,key));
  return edit;
}
export function stateEntryRemoveEdit(source,stateName,key){
  const doc=designDoc(source);
  const block=blockOf(doc,stateName);
  const entry=block.nodes.find(node=>node.props.key===key);
  if(!entry)throw Error(stateName===null?`База не переопределяет ${key}`:`Состояние ${stateName} не переопределяет ${key}`);
  const span=lineSpan(source,entry.start,entry.end);
  // Like a declaration, an entry goes away with the lines it owns; one written inline
  // (`state 'x' { Text { … } }`) is cut out of the line it shares with its block's braces.
  const owns=/^[ \t]*$/.test(source.slice(span.lineStart,entry.start));
  const edit=owns?{from:span.cutFrom,to:span.cutTo,insert:''}:{from:entry.start,to:entry.end,insert:''};
  // A surviving entry would leave the result with one block more than the edit promised, so the
  // comparison below is the check.
  readsAs(source,doc,apply(source,edit),list=>withoutEntry(list,block.label,key));
  return edit;
}
