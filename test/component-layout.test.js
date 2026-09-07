import {test} from 'node:test';
import assert from 'node:assert/strict';
import {arrange,gridPlacement,naturalContainer} from '../src/component-layout.js';
const star={expr:'*'},auto={expr:'auto'};
const row=(props,other)=>({type:'Row',props:{},children:[{props:{width:star,...props}},{props:{width:star,...other}}]});
test('flex layout redistributes opposing minimum and maximum violations into a feasible fit',()=>{
  assert.deepEqual(arrange(row({minWidth:80},{maxWidth:40}),[0,0,100,20],()=>0).boxes,[[0,0,80,20],[80,0,20,20]]);
  assert.deepEqual(arrange(row({maxWidth:20},{minWidth:60}),[0,0,100,20],()=>0).boxes,[[0,0,20,20],[20,0,80,20]]);
  const column={type:'Column',props:{gap:10},children:[{props:{height:star,minHeight:80}},{props:{height:star,maxHeight:40}}]};
  assert.deepEqual(arrange(column,[0,0,20,110],()=>0).boxes,[[0,0,20,80],[0,90,20,20]]);
  assert.deepEqual(arrange(row({minWidth:80},{minWidth:80}),[0,0,100,20],()=>0).boxes.map(box=>box[2]),[80,80]);
  assert.deepEqual(arrange(row({maxWidth:20},{maxWidth:30}),[0,0,100,20],()=>0).boxes.map(box=>box[2]),[20,30]);
});
test('automatic Grid cells reserve later explicit cells and spans before placement',()=>{
  const grid={type:'Grid',props:{columns:[10,10,10]},children:[{props:{}},{props:{cell:[1,1],'column.span':2}},{props:{}}]};
  assert.deepEqual(gridPlacement(grid).placements,[{row:0,column:2,rowSpan:1,columnSpan:1},{row:0,column:0,rowSpan:1,columnSpan:2},{row:1,column:0,rowSpan:1,columnSpan:1}]);
  const partial={type:'Grid',props:{columns:[10,10]},children:[{props:{row:2}},{props:{cell:[2,1],'row.span':2}},{props:{column:1}}]};
  assert.deepEqual(gridPlacement(partial).placements.map(({row,column})=>[row,column]),[[1,1],[1,0],[0,0]]);
});
test('fixed Grid tracks and fixed child sizes do not request intrinsic text measurements',()=>{
  const grid={type:'Grid',props:{columns:[50,60],rows:[30],gap:4},children:[{type:'Text',props:{width:40,height:20}},{type:'Text',props:{width:50,height:20}}]};
  const measure=()=>{throw Error('unexpected text measurement');};
  assert.equal(naturalContainer(grid,0,measure),114);assert.equal(naturalContainer(grid,1,measure),30);
  assert.deepEqual(arrange(grid,[0,0,114,30],measure).boxes,[[5,5,40,20],[59,5,50,20]]);
});
test('Grid intrinsic spans distribute only missing content size across automatic tracks',()=>{
  const span={type:'Grid',props:{columns:[auto,auto],rows:[auto],gap:10},children:[{props:{'column.span':2}}]};
  const size=(_,axis)=>axis===0?100:20;
  assert.equal(naturalContainer(span,0,size),100);
  const layout=arrange(span,[0,0,100,20],size);assert.deepEqual(layout.grid.columns,[45,45]);assert.deepEqual(layout.boxes,[[0,0,100,20]]);
  const mixed={type:'Grid',props:{columns:[50,auto],rows:[auto]},children:[{props:{'column.span':2}}]};
  assert.equal(naturalContainer(mixed,0,size),100);assert.deepEqual(arrange(mixed,[0,0,100,20],size).grid.columns,[50,50]);
});
test('Row and Column center explicit cross sizes and stretch an omitted cross size',()=>{
  const horizontal={type:'Row',props:{},children:[{props:{width:20,height:10}},{props:{width:30}}]};
  assert.deepEqual(arrange(horizontal,[10,20,100,40],()=>0).boxes,[[10,35,20,10],[30,20,30,40]]);
  const vertical={type:'Column',props:{},children:[{props:{width:10,height:20}},{props:{height:30}}]};
  assert.deepEqual(arrange(vertical,[10,20,40,100],()=>0).boxes,[[25,20,10,20],[10,40,40,30]]);
});
