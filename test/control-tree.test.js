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
 querySelector(selector){if(!this.parts.has(selector))this.parts.set(selector,new Element());return this.parts.get(selector);}
 querySelectorAll(){return [];}
 setAttribute(key,value){this.attrs.set(key,value);}
 getAttribute(key){return this.attrs.get(key);}
 before(){}
 prepend(){}
 append(...children){this.children.push(...children);}
 replaceChildren(...children){this.children=children;}
 contains(element){return this===element||this.children.some(child=>child.contains(element));}
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
 tree.fold(rows[3].id,true);assert.equal(list.children.length,4);
 tree.select(rows[4].start,path);assert.equal(list.children.length,5);assert.equal(tree.snapshot().rows[3].expanded,true);
 update({error:'Broken',stale:true,selectedStart:rows[4].start,selectedPath:path});
 assert.ok(list.children.every(el=>el.getAttribute('aria-disabled')==='true'));
 assert.throws(()=>tree.selectId(rows[4].id),/Исправьте/);
 update({selectedStart:rows[4].start,selectedPath:path});assert.ok(list.children.every(el=>el.getAttribute('aria-disabled')==='false'));
 const search=created[0].querySelector('input');search.value='title';search.oninput();
 document.activeElement=null;tree.select(rows[4].start,path);
 assert.equal(list.children.filter(el=>el.tabIndex===0).length,1,'filtered-out selection retains one keyboard focus target');
});
