import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {moveElement,copyElement,insertElement,removeElement,gridCell,reorderElement} from '../src/element-edit.js';
const apply=(s,c)=>s.slice(0,c.from)+c.insert+s.slice(c.to);
const sample="component Test { Frame { width: 400; Button { key: 'a'; text: 'a'; x: 10; y: 20; clicked -> actions.save(); } Button { key: 'a_2'; } } }";
const first=s=>parse(s).nodes[0].children[0];
test('move preserves events and labels, adds missing coordinates and clamps to origin',()=>{
 let n=first(sample),s=apply(sample,moveElement(sample,n.start,5,-30));
 assert.equal(first(s).props.x,15);assert.equal(first(s).props.y,0);assert.equal(first(s).events.clicked,'actions.save');
 n=parse(s).nodes[0].children[1];s=apply(s,moveElement(s,n.start,3,4));assert.equal(parse(s).nodes[0].children[1].props.y,4);
});
test('copy/paste renames collisions while preserving strings and events; deletion removes exactly one node',()=>{
 const text=copyElement(sample,first(sample).start);let s=apply(sample,insertElement(sample,first(sample).start,text));
 let ns=parse(s).nodes[0].children;assert.deepEqual(ns.map(n=>n.props.key),['a','a_3','a_2']);assert.equal(ns[1].props.text,'a');assert.equal(ns[1].events.clicked,'actions.save');
 s=apply(s,removeElement(s,ns[1].start));assert.equal(parse(s).nodes[0].children.length,2);
 assert.throws(()=>removeElement(s,parse(s).nodes[0].start),/Корневой/);
});
test('Grid movement preserves cell notation and does not introduce absolute coordinates',()=>{
 const s='component Test { Frame { columns: [100, *]; rows: [40, *]; Button { cell: [1, 1]; } } }';
 const result=apply(s,moveElement(s,first(s).start,100,0,{row:2,column:2}));assert.deepEqual(first(result).props.cell,[2,2]);assert.equal(first(result).props.x,undefined);
 assert.deepEqual(gridCell({bounds:[20,20,300,200],columns:[100,190],rows:[40,150],gap:[10,10]},160,85),{row:2,column:2});
});
test('expressions and malformed clipboard content fail without damaging source',()=>{
 const s='component Test { Frame { Button { x: state.x; } } }';assert.throws(()=>moveElement(s,first(s).start,1,0),/выражением/);
 assert.throws(()=>insertElement(sample,first(sample).start,'Button {} Button {}'),/один контрол/);
 assert.throws(()=>insertElement(sample,first(sample).start,'} } component Bad { Frame {}'),Error);
});
test('moving an implicit flow position starts at the rendered position',()=>{
 const s='component Test { Frame { Button {} } }';const result=apply(s,moveElement(s,first(s).start,5,10,null,[24,140]));assert.equal(first(result).props.x,29);assert.equal(first(result).props.y,150);
});

test('reorder swaps siblings, retains exact source and selection, and stops at boundaries',()=>{
 const nodes=parse(sample).nodes[0].children;
 const change=reorderElement(sample,nodes[1].start,-1),next=apply(sample,change),ordered=parse(next).nodes[0].children;
 assert.deepEqual(ordered.map(n=>n.props.key),['a_2','a']);assert.equal(ordered[0].start,change.start);
 assert.equal(ordered[1].events.clicked,'actions.save');
 assert.equal(reorderElement(next,ordered[0].start,-1),null);
 const back=reorderElement(next,ordered[0].start,1);assert.equal(apply(next,back),sample);assert.equal(back.start,nodes[1].start);
 assert.equal(reorderElement(sample,nodes[1].start,1),null);
});
test('reorder remains within Scroll and preserves Grid cell properties',()=>{
 const s="component Test { Frame { Scroll { Button { key: 'a'; cell: [1, 1]; } Button { key: 'b'; cell: [1, 2]; } } } }";
 const scroll=parse(s).nodes[0].children[0],change=reorderElement(s,scroll.children[0].start,1);
 const next=parse(apply(s,change)).nodes[0].children[0];assert.deepEqual(next.children.map(n=>n.props.cell),[[1,2],[1,1]]);
 assert.throws(()=>reorderElement(s,scroll.start,1),/дочерний/);
});
