import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {buildControlTree,treeRows,createControlTree} from '../src/control-tree.js';

const path='ui/Demo.ui';
const source="component Demo { Frame { Text { key: 'title'; text: 'Header'; } Frame { Button { text: 'Search'; } } } }";
test('source hierarchy retains nesting, key labels and accurate source offsets',()=>{
 const roots=buildControlTree(parse(source),path),rows=treeRows(roots);
 assert.deepEqual(rows.map(r=>r.level),[1,2,3,3,4]);
 assert.deepEqual(rows.map(r=>r.label),['Demo','Frame','Text','Frame','Button']);
 assert.equal(rows[2].detail,'#title');assert.equal(rows[4].detail,'Search');
 for(const row of rows.filter(r=>r.node)){assert.equal(source.slice(row.node.start,row.node.start+row.node.type.length),row.node.type);assert.equal(row.path,path);}
});
test('folding hides descendants without removing siblings, and filter keeps ancestors',()=>{
 const roots=buildControlTree(parse(source),path),rows=treeRows(roots);const collapsed=new Set([rows[3].id]);
 assert.deepEqual(treeRows(roots,collapsed).map(r=>r.label),['Demo','Frame','Text','Frame']);
 assert.deepEqual(treeRows(roots,collapsed,'search').map(r=>r.label),['Demo','Frame','Frame','Button']);
 assert.equal(treeRows(roots,collapsed,'absent').length,0);
});
test('template view includes override branches but does not pretend they are active runtime nodes',()=>{
 const source="component ImageButton : Button { override content: match props.compact { true => Image { source: 'icon.svg'; }; false => base.content; }; }";
 const rows=treeRows(buildControlTree(parse(source),'components/ImageButton.ui'));
 assert.equal(rows[0].label,'ImageButton : Button');assert.match(rows[1].label,/override content: · match props.compact/);
 assert.equal(rows[2].label,'true ⇒ Image');assert.ok(rows[2].node.start>0);assert.match(rows[3].label,/false ⇒ base.content/);assert.equal(rows[3].node,null);
});
test('resource brushes are not controls and typed content nodes are shown',()=>{
 const rows=treeRows(buildControlTree(parse("component Button { Rectangle { background: Brush { color: #ffffff; }; ContentPresenter { key: 'content'; content: Frame { Text { text: 'Hi'; } }; } } }"),'components/Button.ui'));
 assert.deepEqual(rows.map(r=>r.label),['Button','Rectangle','ContentPresenter','content: Frame','Text']);
});
test('structural ids survive property edits and distinguish identical controls',()=>{
 const before=treeRows(buildControlTree(parse(source),path));const after=treeRows(buildControlTree(parse(source.replace('Header','A much longer heading')),path));
 assert.deepEqual(before.map(r=>r.id),after.map(r=>r.id));assert.equal(new Set(before.map(r=>r.id)).size,before.length);
});
test('unfiltered collapsed branches are not traversed',()=>{
 const leaf={id:'child',label:'Child',get children(){assert.fail('collapsed descendants should not be visited');}};
 const roots=[{id:'root',label:'Root',children:[leaf]}];
 assert.deepEqual(treeRows(roots,new Set(['root'])).map(n=>n.id),['root']);
});

// Minimal DOM for exercising the real selection/folding handlers and row identity.
class Element extends EventTarget {
 constructor(){super();this.children=[];this.dataset={};this.style={};this.attrs=new Map();this.parts=new Map();this.classList={toggle(){}};}
 set innerHTML(value){this.markup=value;}
 querySelector(selector){if(selector==='.control-tree-empty')return this.children.find(c=>c.className==='control-tree-empty')??null;if(!this.parts.has(selector))this.parts.set(selector,new Element());return this.parts.get(selector);}
 querySelectorAll(){return [];}
 setAttribute(key,value){this.attrs.set(key,value);}
 getAttribute(key){return this.attrs.get(key);}
 removeAttribute(key){this.attrs.delete(key);}
 remove(){if(this.parent)this.parent.children=this.parent.children.filter(c=>c!==this);this.parent=null;}
 insertBefore(child,next){child.remove();const index=next?this.children.indexOf(next):this.children.length;this.children.splice(index,0,child);child.parent=this;}
 before(){}
 prepend(){}
 append(...children){for(const child of children){child.remove();child.parent=this;this.children.push(child);}}
 replaceChildren(...children){this.children=children;}
 contains(element){return this===element||this.children.some(child=>child.contains(element));}
 closest(matcher){const key=matcher==='[data-control-id]'?'controlId':matcher==='[data-disclosure]'?'disclosure':null;return key&&this.dataset[key]!==undefined?this:null;}
 getBoundingClientRect(){return {top:0,height:20,left:0,width:120};}
 // A delegated handler reads `e.target`, so a test aims the event at a row and dispatches it on the list.
 dispatch(name,target,fields){const event=new Event(name,{bubbles:true,cancelable:true});Object.defineProperty(event,'target',{value:target});Object.assign(event,fields);this.dispatchEvent(event);return event;}
 focus(){globalThis.document.activeElement=this;}
 scrollIntoView(){}
}
test('selecting visible nodes retains rows, while folding and diagnostics update accessibility',t=>{
 const original=Object.getOwnPropertyDescriptor(globalThis,'document'),created=[];
 const document={createElement(){const element=new Element();created.push(element);return element;},activeElement:null};
 Object.defineProperty(globalThis,'document',{value:document,configurable:true});
 t.after(()=>{if(original)Object.defineProperty(globalThis,'document',original);else delete globalThis.document;});
 const storage={getItem(){return null;},setItem(){}},explorer=new Element(),toolbar=new Element();
 const tree=createControlTree({explorer,toolbar,storage,onScopeChange(){},onSelect(){},onOpenTemplate(){}});
 const doc=parse(source),update=extra=>tree.update({document:doc,path,...extra});update();
 const list=created[0].querySelector('.control-tree-items'),initial=[...list.children],rows=tree.snapshot().rows;
 tree.select(rows[2].start,path);assert.deepEqual(list.children,initial);assert.equal(initial[2].getAttribute('aria-selected'),'true');
 tree.select(rows[4].start,path);assert.deepEqual(list.children,initial);assert.equal(initial[2].getAttribute('aria-selected'),'false');assert.equal(initial[4].getAttribute('aria-selected'),'true');
 update({selectedStart:rows[4].start,selectedPath:path});assert.deepEqual(list.children,initial);
 tree.fold(rows[3].id,true);assert.equal(list.children.length,4);assert.deepEqual(list.children,initial.slice(0,4));
 tree.select(rows[4].start,path);assert.equal(list.children.length,5);assert.equal(tree.snapshot().rows[3].expanded,true);
 update({error:'Broken',stale:true,selectedStart:rows[4].start,selectedPath:path});
 assert.ok(list.children.every(el=>el.getAttribute('aria-disabled')==='true'));
 assert.deepEqual(list.children.slice(0,4),initial.slice(0,4),'unchanged rows retain native surfaces across folding and diagnostics');
 tree.update({document:parse(source),path,error:'Broken',selectedStart:rows[4].start,selectedPath:path});
 assert.deepEqual(list.children.slice(0,4),initial.slice(0,4),'reparsed source retains existing rows');
 assert.throws(()=>tree.selectId(rows[4].id),/Исправьте/);
 update({selectedStart:rows[4].start,selectedPath:path});assert.ok(list.children.every(el=>el.getAttribute('aria-disabled')==='false'));
 const search=created[0].querySelector('input');search.value='title';search.oninput();
 document.activeElement=null;tree.select(rows[4].start,path);
 assert.equal(list.children.filter(el=>el.tabIndex===0).length,1,'filtered-out selection retains one keyboard focus target');
});

// One tree with a container of three controls and a nested pair, so a drag has both a group to
// start from and a deeper group to compare it with.
const nestSource="component Demo {\n  Frame {\n    Button { key: 'a'; }\n    Column { Text { key: 'deep'; } Text { key: 'other'; } }\n    Row { gap: 4; }\n  }\n}\n";
// A lone control with a container beside it, plus a node that only fills a property slot.
const slotSource="component Demo {\n  Frame {\n    Badge { key: 'solo'; }\n    Column { Text { key: 'deep'; } }\n    ContentPresenter { content: Row { Text { text: 'slot'; } }; }\n  }\n}\n";
function dragTree(t,{editable=true,source=nestSource}={}){
 const state={editable};
 const original=Object.getOwnPropertyDescriptor(globalThis,'document'),created=[];
 const document={createElement(){const element=new Element();created.push(element);return element;},activeElement:null};
 Object.defineProperty(globalThis,'document',{value:document,configurable:true});
 t.after(()=>{if(original)Object.defineProperty(globalThis,'document',original);else delete globalThis.document;});
 const calls=[],explorer=new Element(),toolbar=new Element();
 const tree=createControlTree({explorer,toolbar,storage:{getItem(){return null;},setItem(){}},onScopeChange(){},onSelect(){},onOpenTemplate(){},onReorder:call=>calls.push(call),canReorder:()=>state.editable});
 const doc=parse(source);tree.update({document:doc,path});
 // rows: Demo, Frame, Button, Column, Text deep, Text other, Row
 return {tree,calls,doc,state,list:created[0].querySelector('.control-tree-items'),search:created[0].querySelector('input')};
}
test('a row is draggable exactly where a sibling exists to move between',t=>{
 const {list}=dragTree(t);
 assert.deepEqual(list.children.map(el=>el.draggable),[false,false,true,true,true,true,true]);
 assert.match(list.children[4].title,/перетащите/);
 assert.equal(list.children[1].title.includes('перетащите'),false,'the page container has no sibling slot');
 assert.deepEqual(dragTree(t,{editable:false}).list.children.map(el=>el.draggable),Array(7).fill(false));
});
test('a drop between siblings hands both source offsets over and leaves no markers',t=>{
 const {list,calls}=dragTree(t),deep=parse(nestSource).nodes[0].children[1].children;
 const from=list.children[5],to=list.children[4],transfer={setData(){},set effectAllowed(_){},set dropEffect(_){}};
 list.dispatch('dragstart',from,{dataTransfer:transfer});assert.equal(from.dataset.dragging,'true');
 let event=list.dispatch('dragover',to,{clientY:4,dataTransfer:transfer});
 assert.equal(event.defaultPrevented,true);assert.equal(to.dataset.drop,'before');
 event=list.dispatch('dragover',to,{clientY:16,dataTransfer:transfer});assert.equal(to.dataset.drop,'after');
 event=list.dispatch('drop',to,{});assert.equal(event.defaultPrevented,true);
 assert.deepEqual(calls,[{path,start:deep[1].start,targetStart:deep[0].start,side:'after'}]);
 assert.equal(to.dataset.drop,undefined);assert.equal(from.dataset.dragging,undefined);
 // The release after a drag that never found a slot moves nothing.
 list.dispatch('dragstart',from,{});list.dispatch('dragend',from,{});list.dispatch('drop',to,{});
 assert.equal(calls.length,1);assert.equal(from.dataset.dragging,undefined);
});
test('a row outside the dragged control’s group never opens a slot',t=>{
 const {list,calls}=dragTree(t);
 const button=list.children[2],row=list.children[6],frame=list.children[1],deep=list.children[4];
 list.dispatch('dragstart',button,{});
 for(const target of [deep,frame]){
  assert.equal(list.dispatch('dragover',target,{clientY:4}).defaultPrevented,false);
  assert.equal(target.dataset.drop,undefined);
  list.dispatch('drop',target,{});
 }
 assert.deepEqual(calls,[]);
});
test('Alt+arrows move the focused row one slot and stop at the ends of the group',t=>{
 const {list,calls}=dragTree(t),kids=parse(nestSource).nodes[0].children;
 list.dispatch('keydown',list.children[3],{key:'ArrowUp',altKey:true});
 assert.deepEqual(calls,[{path,start:kids[1].start,targetStart:kids[0].start,side:'before'}]);
 list.dispatch('keydown',list.children[2],{key:'ArrowDown',altKey:true});
 assert.equal(calls.length,2);assert.deepEqual(calls[1],{path,start:kids[0].start,targetStart:kids[1].start,side:'after'});
 list.dispatch('keydown',list.children[2],{key:'ArrowUp',altKey:true});assert.equal(calls.length,2,'above the first sibling there is nowhere to go');
 const off=dragTree(t,{editable:false});off.list.dispatch('keydown',off.list.children[3],{key:'ArrowUp',altKey:true});
 assert.deepEqual(off.calls,[]);
});
test('a broken source or an open filter takes the drag away before it starts',t=>{
 const {tree,list,calls,search}=dragTree(t);
 tree.update({document:parse(nestSource),path,error:'Broken'});
 assert.deepEqual(list.children.map(el=>el.draggable),Array(7).fill(false));
 assert.equal(list.dispatch('dragstart',list.children[2],{}).defaultPrevented,true,'a cancelled drag never leaves the row');
 tree.update({document:parse(nestSource),path});
 assert.equal(list.children[2].draggable,true);
 search.value='deep';search.oninput();
 assert.deepEqual(list.children.map(el=>el.draggable).filter(Boolean),[],'hidden siblings make the visible order a lie');
 list.dispatch('dragstart',list.children[3],{});list.dispatch('drop',list.children[2],{});
 assert.deepEqual(calls,[]);
});
test('a switch that only takes the drag away still redraws the row flags',t=>{
 const {tree,list,state,doc}=dragTree(t);
 assert.equal(list.children[2].draggable,true);
 state.editable=false;
 // The same document and the same diagnostic: only the answer to "may this source be edited" moved.
 tree.update({document:doc,path});
 assert.deepEqual(list.children.map(el=>el.draggable),Array(7).fill(false));
 state.editable=true;
 tree.update({document:doc,path});
 assert.equal(list.children[2].draggable,true);
});
test('a lone control starts a drag because a container can still take it, a property slot never does',t=>{
 const {list}=dragTree(t,{source:slotSource});
 // Demo, Frame, Badge, Column, Text, ContentPresenter, content: Row, Text in the slot.
 assert.deepEqual(list.children.map(el=>el.draggable),[false,false,true,true,true,true,false,false]);
 assert.equal(list.dispatch('dragstart',list.children[6],{}).defaultPrevented,true,'a node that fills a property belongs to no sibling run');
});
test('the middle of a container row is the slot inside it and its edges stay a reorder',t=>{
 const {list,calls}=dragTree(t,{source:slotSource}),kids=parse(slotSource).nodes[0].children;
 const badge=list.children[2],column=list.children[3];
 list.dispatch('dragstart',badge,{});
 assert.equal(list.dispatch('dragover',column,{clientY:2}).defaultPrevented,true);assert.equal(column.dataset.drop,'before');
 assert.equal(list.dispatch('dragover',column,{clientY:10}).defaultPrevented,true,'a neighbour row takes the control in its middle');
 assert.equal(column.dataset.drop,'inside');
 list.dispatch('drop',column,{});
 assert.deepEqual(calls,[{path,start:kids[0].start,targetStart:kids[1].start,side:'inside'}]);
 assert.equal(column.dataset.drop,undefined);assert.equal(badge.dataset.dragging,undefined);
});
test('a panel is not handed to what it holds, to a leaf, or to its own panel',t=>{
 const {list,calls}=dragTree(t,{source:slotSource});
 const badge=list.children[2],frame=list.children[1],column=list.children[3],deep=list.children[4];
 for(const [source,target] of [[badge,frame],[badge,deep],[column,deep]]){
  list.dispatch('dragstart',source,{});
  assert.equal(list.dispatch('dragover',target,{clientY:10}).defaultPrevented,false,'the middle names no slot here');
  assert.equal(target.dataset.drop,undefined);
  list.dispatch('drop',target,{});list.dispatch('dragend',source,{});
 }
 assert.deepEqual(calls,[]);
});
test('a row of another parent offers neither edge, so a stray drop cannot reorder what it cannot see',t=>{
 const {list,calls}=dragTree(t,{source:slotSource});
 const badge=list.children[2],deep=list.children[4];
 list.dispatch('dragstart',badge,{});
 assert.equal(list.dispatch('dragover',deep,{clientY:2}).defaultPrevented,false);
 assert.equal(deep.dataset.drop,undefined);assert.deepEqual(calls,[]);
});
