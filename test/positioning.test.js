import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {arrange} from '../src/component-layout.js';
import {compileComponents} from '../src/components.js';
import {takesCoordinates,coordinateShift} from '../src/positioning.js';

const rect=([left,top,width,height])=>({left,top,width,height});
const children=source=>parse(source).nodes[0].children;
const control=(props,children=[])=>({type:'Text',props,children,start:0,end:0,events:{},bindings:{}});
const container=(type,props,children)=>({type,props,children,start:0,end:0,events:{},bindings:{}});

test('a coordinate moves a control only where the flow arranges it',()=>{
 const [text,grid,framed,stack,scroll,row]=children("component Demo { Column { Text { key: 'a'; x: 12; } Grid { Text { key: 'b'; x: 5; } } Frame { columns: 2; Text { key: 'c'; x: 7; } } Stack { Text { key: 'd'; x: 9; } } Scroll { Text { key: 'e'; x: 11; } } Row { Text { key: 'f'; x: 3; } } } }");
 const column=container('Column',{});
 for(const [name,parent,node,expected] of [['Column',column,text,true],['Grid',grid,grid.children[0],false],['Frame with tracks',framed,framed.children[0],false],['Stack',stack,stack.children[0],false],['Scroll',scroll,scroll.children[0],true],['Row',row,row.children[0],true]])
  assert.equal(takesCoordinates(node,parent),expected,`${name} child`);
 assert.equal(takesCoordinates(control({x:1}),container('Frame',{width:100})),true,'a Frame without tracks flows');
 assert.equal(takesCoordinates(control({x:1}),null),false,'the page container has nothing to move against');
 assert.equal(takesCoordinates(control({}),column),false,'a control without coordinates stays in flow');
});
test('a coordinate reads the same units as the scene and offsets from the parent corner',()=>{
 const column=container('Column',{}),parent=rect([100,50,200,120]),flow=rect([100,90,80,20]);
 assert.deepEqual(coordinateShift(control({x:24,y:'10%'}),column,{},parent,flow),{left:24,top:-28},'both axes, percent of the parent width');
 assert.deepEqual(coordinateShift(control({x:'30px'}),column,{},parent,flow),{left:30},'only the axis the markup named');
 assert.deepEqual(coordinateShift(control({x:{expr:'state.offset'}}),column,{offset:36},parent,flow),{left:36},'a coordinate the state provides');
 assert.equal(coordinateShift(control({x:'content'}),column,{},parent,flow).left,0,'content sits on the parent edge, as in the scene');
 assert.equal(coordinateShift(control({x:5}),container('Grid',{}),{},parent,flow),null,'a Grid child is placed by its tracks');
});
test('a coordinate the scene would reject never reaches the preview',()=>{
 const column=container('Column',{}),parent=rect([0,0,200,120]),flow=rect([0,0,80,20]);
 assert.throws(()=>coordinateShift(control({x:-5}),column,{},parent,flow),/вне диапазона/);
 assert.throws(()=>coordinateShift(control({y:'wide'}),column,{},parent,flow),/неверный размер/);
});
test('the flow slot plus the offset is the box the scene draws',()=>{
 const placed=[control({width:200,height:40}),control({width:80,height:30,x:'50%',y:12}),control({width:120,height:20,x:8})];
 const column=container('Column',{gap:10},placed),box=[100,50,200,120];
 const flow=arrange(column,box,()=>0).boxes,scene=arrange(column,box,()=>0,{scene:true}).boxes;
 for(const [index,node]of placed.entries()){
  const shift={left:0,top:0,...coordinateShift(node,column,{},rect(box),rect(flow[index]))};
  assert.deepEqual([flow[index][0]+shift.left,flow[index][1]+shift.top,flow[index][2],flow[index][3]],scene[index],`control ${index} lands where the vector render puts it`);
 }
 assert.deepEqual(flow[0],scene[0],'an untouched sibling keeps its slot beside a moved one');
});
const button=`component Button {
 width: 100; height: 30; text: '';
 Rectangle { Text { text: props.text; } }
}`;
// The same page the vector renderer consumes: a padded Column whose outer box is known, so an
// offset measured from the container edge is distinguishable from one measured from its content.
const page=body=>compileComponents({
 'ui/Demo.ui':`component Demo { Frame { width: 320; height: 180; ${body} } }`,
 'components/Button.ui':button,
},'ui/Demo.ui',{},{measureText:(text,size)=>[text.length*size/2,size]}).previewControls;
const buttons=moved=>`Column { width: 320; height: 180; padding: 20; gap: 10;
 Button { text: 'a'; width: 100; height: 30; }
 Button { text: 'b'; width: 100; height: 30; ${moved?.b??''} }
 Button { text: 'c'; width: 100; height: 30; ${moved?.c??''} }
 Button { text: 'd'; width: 100; height: 30; }
}`;
test('the vector scene measures a coordinate from the container edge the preview offsets against',()=>{
 const flow=page(buttons()),moved=page(buttons({b:'x: 140; y: 72;',c:"x: '25%';"}));
 assert.deepEqual([moved[1].props.x,moved[1].props.y],[140,72],'a coordinate counts from the container edge, not from the flow slot');
 assert.equal(moved[2].props.x,80,'a percent measures the whole container, not its content box');
 assert.deepEqual(moved.map(n=>[n.props.x,n.props.y]).filter((_,i)=>i===0||i===3),flow.map(n=>[n.props.x,n.props.y]).filter((_,i)=>i===0||i===3),'a control beside a moved one keeps its flow slot');
 assert.deepEqual(moved.slice(1,3).map(n=>[n.props.width,n.props.height]),flow.slice(1,3).map(n=>[n.props.width,n.props.height]),'the flow still sizes a moved control');
 const column=container('Column',{width:320,height:180,padding:20,gap:10});
 for(const [index,props]of [[1,{x:140,y:72}],[2,{x:'25%'}]]){
  const shift={left:0,top:0,...coordinateShift(control(props),column,{},rect([0,0,320,180]),rect([flow[index].props.x,flow[index].props.y,flow[index].props.width,flow[index].props.height]))};
  assert.deepEqual([flow[index].props.x+shift.left,flow[index].props.y+shift.top],[moved[index].props.x,moved[index].props.y],`the preview offset lands on the box the renderer draws for control ${index}`);
 }
});
