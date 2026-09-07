import {test} from 'node:test';
import assert from 'node:assert/strict';
import {selectionBounds,alignSelection,distributeSelection,snapSelection} from '../src/selection-layout.js';
import {parse} from '../src/language.js';
import {editElements,moveElement,removeElement,copyElements,insertElements} from '../src/element-edit.js';
const apply=(s,c)=>s.slice(0,c.from)+c.insert+s.slice(c.to);
test('alignment respects different sizes, distribution preserves outer edges',()=>{
 const items=[{start:1,bounds:[10,20,20,30]},{start:2,bounds:[60,80,40,10]},{start:3,bounds:[150,100,10,20]}];
 assert.deepEqual(selectionBounds(items.map(i=>i.bounds)),[10,20,150,100]);
 assert.deepEqual(alignSelection(items,'right').map(i=>i.dx),[130,60,0]);
 const d=distributeSelection(items,0);assert.deepEqual(d.map(i=>i.dx),[0,10,0]);
 assert.throws(()=>distributeSelection(items.slice(0,2),0),/три/);
});
test('snapping chooses the nearest edge or center within screen-scaled threshold',()=>{
 const result=snapSelection([10,20,20,20],38,0,[[50,90,40,20]],[400,300],3);
 assert.equal(result.dx,40);assert.ok(result.guides.some(g=>g.axis===0&&g.position===50));
 const far=snapSelection([10,20,20,20],35,0,[[50,90,40,20]],[400,300],3);assert.equal(far.dx,35);
});
test('group edits are atomic and selection offsets follow changes of different lengths',()=>{
 const source="component X { Frame { Button { key:'one'; x:9; y:0; } Button { key:'two'; x:99; y:10; } } }";
 const starts=parse(source).nodes[0].children.map(n=>n.start);
 const change=editElements(source,starts,start=>moveElement(source,start,2,5));const next=apply(source,change),nodes=parse(next).nodes[0].children;
 assert.deepEqual(nodes.map(n=>n.props.x),[11,101]);assert.deepEqual(change.starts,nodes.map(n=>n.start));
 const copied=copyElements(next,change.starts);const pasted=insertElements(next,change.starts[1],copied);const all=parse(apply(next,pasted)).nodes[0].children;
 assert.deepEqual(all.map(n=>n.props.key),['one','two','one_2','two_2']);assert.deepEqual(pasted.starts,all.slice(2).map(n=>n.start));
 const removed=editElements(next,change.starts,start=>removeElement(next,start));assert.equal(parse(apply(next,removed)).nodes[0].children.length,0);assert.deepEqual(removed.starts,[]);
});
