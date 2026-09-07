import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createElementTools} from '../src/element-tools.js';
import {parse} from '../src/language.js';
class Element extends EventTarget{
 constructor(){super();this.children=[];this.dataset={};this.style={};this.classList={contains:()=>false};this.scrollLeft=this.scrollTop=0;this.capture=new Set();}
 append(n){this.children.push(n);n.parent=this;} after(n){this.next=n;} setAttribute(){} contains(n){return n===this||this.children.some(c=>c.contains(n));}closest(){return null;} focus(){}getBoundingClientRect(){return {left:0,top:0,width:800,height:400};}setPointerCapture(id){this.capture.add(id);}releasePointerCapture(id){this.capture.delete(id);}hasPointerCapture(id){return this.capture.has(id);}
 emit(type,data={}){const e=new Event(type,{cancelable:true});for(const [k,v] of Object.entries(data))Object.defineProperty(e,k,{value:v});this.dispatchEvent(e);return e;}
}
function setup(){
 globalThis.document={createElement:()=>new Element()};globalThis.window=new Element();
 const viewport=new Element(),artboard=new Element(),toolbar=new Element();viewport.append(artboard);let source='component Test { Frame { width: 400; Button { x: 10; y: 20; key: \'button\'; } } }';let selected=null,enabled=true;const commits=[],errors=[];
 const context=()=>enabled?{source,start:selected,path:'test.ui',nodes:parse(source).nodes[0].children,scene:{width:400,controls:[{index:0,bounds:[10,20,100,40]}]}}:null;
 createElementTools({viewport,artboard,toolbar,context,select:n=>{selected=n.start;},commit:c=>{commits.push(c);source=source.slice(0,c.from)+c.insert+source.slice(c.to);selected=c.start;},history:()=>{},report:e=>errors.push(e)});
 return {viewport,artboard,toolbar,commits,errors,source:()=>source,disable:()=>{enabled=false;},down:()=>viewport.emit('pointerdown',{target:artboard,button:0,pointerId:1,clientX:40,clientY:60})};
}
test('drag uses logical coordinates at 200%, commits once on release, Escape cancels',()=>{
 const t=setup();t.down();t.viewport.emit('pointermove',{pointerId:1,clientX:80,clientY:80});assert.equal(t.commits.length,0);
 t.viewport.emit('pointerup',{pointerId:1});assert.equal(t.commits.length,1);let n=parse(t.source()).nodes[0].children[0];assert.equal(n.props.x,30);assert.equal(n.props.y,30);
 t.down();t.viewport.emit('pointermove',{pointerId:1,clientX:100,clientY:80});window.emit('keydown',{key:'Escape'});t.viewport.emit('pointerup',{pointerId:1});assert.equal(t.commits.length,1);assert.equal(t.viewport.capture.size,0);
});
test('click without movement does not edit; duplicate and delete are keyboard operations',()=>{
 const t=setup();t.down();t.viewport.emit('pointerup',{pointerId:1});assert.equal(t.commits.length,0);
 window.emit('keydown',{target:t.viewport,key:'d',metaKey:true});assert.equal(parse(t.source()).nodes[0].children.length,2);
 window.emit('keydown',{target:t.viewport,key:'Delete'});assert.equal(parse(t.source()).nodes[0].children.length,1);assert.deepEqual(t.errors,[]);
});
test('shortcuts leave text editing and interaction mode untouched',()=>{
 const t=setup();t.down();t.viewport.emit('pointerup',{pointerId:1});const input=new Element();input.closest=()=>input;
 const event=window.emit('keydown',{target:input,key:'Delete'});assert.equal(event.defaultPrevented,false);assert.equal(t.commits.length,0);
 t.disable();window.emit('keydown',{target:t.viewport,key:'Delete'});assert.equal(t.commits.length,0);
});
test('Alt+Down reorders selected sibling and Alt+Up restores it',()=>{
 const t=setup();t.down();t.viewport.emit('pointerup',{pointerId:1});
 window.emit('keydown',{target:t.viewport,key:'d',metaKey:true});
 const before=t.source();
 window.emit('keydown',{target:t.viewport,key:'ArrowUp',altKey:true});
 assert.deepEqual(parse(t.source()).nodes[0].children.map(n=>n.props.key),['button_2','button']);
 window.emit('keydown',{target:t.viewport,key:'ArrowDown',altKey:true});assert.equal(t.source(),before);assert.deepEqual(t.errors,[]);
});
