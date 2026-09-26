import {test} from 'node:test';
import assert from 'node:assert/strict';
import {controlCss,controlDeclarations,controlStates,handoffName} from '../src/handoff.js';
import {compileComponents} from '../src/components.js';

const control=(props,type='Button')=>({type,props,children:[]});
const css=(props,bounds,state)=>controlCss(control(props),bounds,state);

test('a handoff block carries the box the layout drew and the values the compiler resolved',()=>{
 assert.equal(css({width:'auto',height:40,background:'#20272f',color:'#e8edf7',fontSize:16,radius:4,borderWidth:1,borderColor:'#465775',padding:[8,16],gap:6,clip:true,transitionDuration:{expr:'140ms'}},[0,0,240.005,40]),
  ['.button {',
   '  width: 240.01px;',
   '  height: 40px;',
   '  background: #20272f;',
   '  color: #e8edf7;',
   '  font-size: 16px;',
   '  border-radius: 4px;',
   '  border: 1px solid #465775;',
   '  padding: 8px 16px;',
   '  gap: 6px;',
   '  overflow: hidden;',
   '  transition-duration: 140ms;',
   '}',].join('\n'));
});
test('the size of the block is the size the scene computed',()=>{
 // `width: *` is a layout instruction, not a length: only the drawn box can say how wide it got.
 assert.equal(css({width:{expr:'*'},height:{expr:'auto'},borderWidth:2},[0,0,180,32]),'.button {\n  width: 180px;\n  height: 32px;\n  border-width: 2px;\n}');
 assert.equal(css({width:180,height:32},null),'.button {\n  width: 180px;\n  height: 32px;\n}');
});
test('names come from the key the designer gave the control',()=>{
 assert.equal(handoffName(control({key:'submit'})),'submit');
 assert.equal(handoffName(control({key:'2 fastest'})),'button');
 assert.equal(handoffName(control({},'TabButton')),'tabbutton');
 assert.equal(handoffName(control({},'Привет')),'control');
});
test('states come out as the pseudo-classes a browser applies',()=>{
 assert.deepEqual(controlStates(control({placeholderColor:'#98a4ba',hoverBackground:'#a8baff',focusBorderColor:'#fff',pressedBackground:'#6a83da',disabledBackground:'#596273'}),{}),
  [{selector:':hover',property:'background',value:'#a8baff'},{selector:':active',property:'background',value:'#6a83da'},{selector:':disabled',property:'background',value:'#596273'},{selector:':focus',property:'border-color',value:'#fff'},{selector:'::placeholder',property:'color',value:'#98a4ba'}]);
 assert.equal(css({key:'field',background:'#111',hoverBackground:'#222'},[0,0,10,10]),
  '.field {\n  width: 10px;\n  height: 10px;\n  background: #111;\n}\n.field:hover {\n  background: #222;\n}');
});
test('a state that paints what the control already paints asks for no rule',()=>{
 // Primitives carry a hover, a pressed and a disabled colour whether or not the design uses them,
 // so a handoff that printed all of them filled a developer's stylesheet with rules that change
 // nothing — and a designer reading the export could not see which states the control really has.
 assert.deepEqual(controlStates(control({background:'#20272f',hoverBackground:'#20272f',pressedBackground:'#111',borderColor:'#444',focusBorderColor:'#444',color:'#eee',placeholderColor:'#eee'}),{}),
  [{selector:':active',property:'background',value:'#111'}],'the state that differs from the base survives');
 assert.equal(css({key:'plain',background:'#20272f',hoverBackground:'#20272f'},[0,0,8,8]),
  '.plain {\n  width: 8px;\n  height: 8px;\n  background: #20272f;\n}','a matching hover leaves no block at all');
 assert.deepEqual(controlStates(control({hoverBackground:'#a8baff'}),{}),[{selector:':hover',property:'background',value:'#a8baff'}],'a state with no base colour is still a state');
});
test('a value CSS cannot carry is left out rather than guessed at',()=>{
 const props={width:{expr:'state.missing'},background:{expr:'props.tone'},color:'not-a-color',padding:'huge',gap:[4,'wide'],radius:'auto',clip:false,transitionDuration:200,borderColor:'argb(255, 12, 20, 40)'};
 assert.deepEqual(controlDeclarations(control(props),null,{}),['  border-color: rgb(12 20 40 / 1);']);
 assert.equal(controlCss(control({...props,borderColor:undefined}),null,{}),'','nothing survives, so no block is emitted');
 assert.deepEqual(controlDeclarations(control({clip:'true'}),null,{}),[],'a clip the markup spelled as text is not the boolean the renderer reads');
});
test('the compiled page hands over the resolved component, not the markup it started from',()=>{
 const out=compileComponents({
  'ui/Demo.ui':`component Demo { Frame { width: 320; height: 120;
   Button { text: 'Go'; width: 260; height: 40; background: state.tint; key: 'send'; }
  } }`,
  'components/Button.ui':`component Button {
   width: 100; height: 30; text: ''; radius: 16; color: #14213b;
   background: #8ca5ff; hoverBackground: #a8baff; borderWidth: 1; borderColor: #bed0ff;
   transitionDuration: 80ms;
   Rectangle { Text { text: props.text; color: props.color; } }
  }`,
 },'ui/Demo.ui',{tint:'#ff0000'},{measureText:(text,size)=>[text.length*size/2,size]});
 // The page spelled out neither a radius nor a hover colour: those come from the component, and the
 // visual node is where the compiler leaves the values a handoff has to carry.
 const drawn=out.visualNodes.find(v=>v.source.from===out.previewControls[0].start);
 assert.equal(controlCss(drawn,drawn.bounds,{}),[
  '.send {',
  '  width: 260px;',
  '  height: 40px;',
  '  background: #ff0000;',
  '  color: #14213b;',
  '  font-size: 20px;',
  '  border-radius: 16px;',
  '  border: 1px solid #bed0ff;',
  '  transition-duration: 80ms;',
  '}',
  '.send:hover {',
  '  background: #a8baff;',
  '}',
  '.send:active {',
  '  background: #6a83da;',
  '}',
  '.send:disabled {',
  '  background: #596273;',
  '}',
  '.send:focus {',
  '  border-color: #ffffff;',
  '}',].join('\n'));
});
