import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createElementTools} from '../src/element-tools.js';
import {parse} from '../src/language.js';
class Element extends EventTarget{
 constructor(){super();this.children=[];this.dataset={};this.style={};this.classList={contains:()=>false};this.scrollLeft=this.scrollTop=0;this.capture=new Set();}
 remove(){if(this.parent)this.parent.children=this.parent.children.filter(c=>c!==this);}
 append(n){this.children.push(n);n.parent=this;} replaceChildren(){this.children=[];} after(n){this.next=n;} setAttribute(){} contains(n){return n===this||this.children.some(c=>c.contains(n));}closest(){return null;} focus(){}getBoundingClientRect(){return {left:0,top:0,width:800,height:400};}setPointerCapture(id){this.capture.add(id);}releasePointerCapture(id){this.capture.delete(id);}hasPointerCapture(id){return this.capture.has(id);}
 emit(type,data={}){const e=new Event(type,{cancelable:true});for(const [k,v] of Object.entries(data))Object.defineProperty(e,k,{value:v});this.dispatchEvent(e);return e;}
}
function setup(initial,inserts=[]){
 globalThis.document={createElement:()=>new Element()};globalThis.window=new Element();
 const viewport=new Element(),artboard=new Element(),toolbar=new Element();viewport.append(artboard);let source='component Test { Frame { width: 400; Button { x: 10; y: 20; key: \'button\'; } } }';if(initial)source=initial;let selected=null,enabled=true;const commits=[],errors=[];
 const context=()=>enabled?{source,start:selected,path:'test.ui',root:parse(source).nodes[0],inserts,nodes:parse(source).nodes[0].children,scene:{width:400,height:200,controls:parse(source).nodes[0].children.map((n,index)=>({index,bounds:[n.props.x??0,n.props.y??0,n.props.width??100,n.props.height??40]}))}}:null;
 const tools=createElementTools({viewport,artboard,toolbar,context,select:n=>{selected=n.start;},commit:c=>{commits.push(c);source=source.slice(0,c.from)+c.insert+source.slice(c.to);selected=c.start;if(c.starts)tools.setSelection(c.starts);},history:()=>{},report:e=>errors.push(e)});
 return {tools,viewport,artboard,toolbar,commits,errors,source:()=>source,disable:()=>{enabled=false;},down:()=>viewport.emit('pointerdown',{target:artboard,button:0,pointerId:1,clientX:40,clientY:60})};
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

const groupSource="component Test { Frame { width:400; Button { key:'a'; x:10; y:20; width:60; } Button { key:'b'; x:150; y:80; width:60; } } }";
test('Shift click selects a group, drag moves it once and deletion removes the whole group',()=>{
 const t=setup(groupSource);t.down();t.viewport.emit('pointerup',{pointerId:1});
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:2,clientX:320,clientY:180,shiftKey:true});assert.equal(t.tools.selection().length,2);
 t.down();t.viewport.emit('pointermove',{pointerId:1,clientX:60,clientY:80,altKey:true});t.viewport.emit('pointerup',{pointerId:1});
 assert.equal(t.commits.length,1);assert.deepEqual(parse(t.source()).nodes[0].children.map(n=>[n.props.x,n.props.y]),[[20,30],[160,90]]);assert.equal(t.tools.selection().length,2);
 window.emit('keydown',{target:t.viewport,key:'Delete'});assert.equal(parse(t.source()).nodes[0].children.length,0);assert.deepEqual(t.errors,[]);
});
test('marquee selects intersecting controls and Escape cancels without changing selection',()=>{
 const t=setup(groupSource);t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:1,clientX:0,clientY:0});t.viewport.emit('pointermove',{pointerId:1,clientX:450,clientY:250});t.viewport.emit('pointerup',{pointerId:1});assert.equal(t.tools.selection().length,2);assert.equal(t.commits.length,0);
 t.down();t.viewport.emit('pointermove',{pointerId:1,clientX:60,clientY:80});window.emit('keydown',{target:t.viewport,key:'Escape'});assert.equal(t.tools.selection().length,2);assert.equal(t.commits.length,0);
});

test('canvas context menu selects the pointed control, runs commands and closes',()=>{
 const t=setup(),menu=t.toolbar.next;assert.equal(menu.hidden,true);
 const event=t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60});
 assert.equal(event.defaultPrevented,true);assert.equal(menu.hidden,false);assert.equal(t.tools.selection().length,1);
 menu.children.find(b=>b.dataset.action==='duplicate').onclick();
 assert.equal(menu.hidden,true);assert.equal(parse(t.source()).nodes[0].children.length,2);
 window.emit('keydown',{target:t.viewport,key:'F10',shiftKey:true});assert.equal(menu.hidden,false);
 menu.emit('keydown',{key:'Escape'});assert.equal(menu.hidden,true);
 t.disable();assert.equal(t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60}).defaultPrevented,false);
});

test('destroy releases editing listeners so a detached Studio cannot execute commands',()=>{const t=setup();t.down();t.viewport.emit('pointerup',{pointerId:1});t.tools.destroy();window.emit('keydown',{target:t.viewport,key:'d',metaKey:true});assert.equal(t.commits.length,0);assert.equal(t.viewport.capture.size,0);});
const textItem={type:'Text',markup:"Text { text: 'Новый'; fontSize: 16; color: #e8edf7; }"};
test('the palette puts a new control next to the selected one and selects it',()=>{
 const t=setup(undefined,[textItem]),menu=t.toolbar.next;
 t.down();t.viewport.emit('pointerup',{pointerId:1});
 t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60});
 const add=menu.children.find(b=>b.dataset.type==='Text');
 assert.equal(add.textContent,'＋ Text');assert.equal(add.disabled,false);
 add.onclick();
 const children=parse(t.source()).nodes[0].children;
 assert.deepEqual(children.map(n=>n.type),['Button','Text']);
 assert.equal(children[1].props.text,'Новый');assert.equal(t.commits.length,1);
 assert.deepEqual(t.tools.selection(),[children[1].start]);assert.deepEqual(t.errors,[]);
});
test('with nothing selected the first control of a scrolling page goes inside the Scroll',()=>{
 const t=setup('component Test { Frame { Scroll { Button { key: \'a\'; } } } }',[textItem]),menu=t.toolbar.next;
 t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:300});
 assert.equal(t.tools.selection().length,0);
 menu.children.find(b=>b.dataset.type==='Text').onclick();
 const scroll=parse(t.source()).nodes[0].children[0];
 assert.equal(scroll.type,'Scroll');assert.deepEqual(scroll.children.map(n=>n.type),['Button','Text']);
 assert.deepEqual(t.errors,[]);
});
test('the menu lists the project palette and rebuilds when it grows',()=>{
 const list=[textItem],t=setup(undefined,list),menu=t.toolbar.next;
 t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60});
 assert.equal(menu.children.filter(b=>b.dataset.action==='add').length,1);
 list.push({type:'Button',markup:'Button { }'});
 t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60});
 assert.deepEqual(menu.children.filter(b=>b.dataset.action==='add').map(b=>b.dataset.type),['Text','Button']);
 // The selection counter is moved, not duplicated, by a rebuild.
 assert.equal(menu.children.filter(b=>b.className==='selection-count').length,1);
 assert.equal(setup().toolbar.next.children.some(b=>b.dataset.action==='add'),false);
});
test('the palette filter narrows the list and Enter adds the first match',()=>{
 const list=[textItem,{type:'Button',markup:'Button { }'}];
 const t=setup(undefined,list),menu=t.toolbar.next;
 t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60});
 const filter=menu.children.find(b=>b.className==='element-add-filter');
 filter.value='butt';filter.oninput();
 assert.deepEqual(menu.children.filter(b=>b.dataset.action==='add').map(b=>b.dataset.type),['Button']);
 filter.onkeydown({key:'Enter',preventDefault(){}});
 assert.deepEqual(parse(t.source()).nodes[0].children.map(n=>n.type),['Button','Button']);
 assert.equal(t.commits.length,1);assert.deepEqual(t.errors,[]);
 // A project whose palette changed must not stay behind the old query.
 list.splice(1,1);
 t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60});
 assert.deepEqual(menu.children.filter(b=>b.dataset.action==='add').map(b=>b.dataset.type),['Text']);
 assert.equal(filter.value,'');
});
test('a printable key with the menu open searches the palette instead of running a command',()=>{
 const t=setup(undefined,[textItem,{type:'Button',markup:'Button { }'}]),menu=t.toolbar.next;
 t.viewport.emit('contextmenu',{target:t.artboard,clientX:40,clientY:60});
 const filter=menu.children.find(b=>b.className==='element-add-filter');let focused=0;filter.focus=()=>{focused++};
 const event=menu.emit('keydown',{key:'b'});
 // The key itself is left to the browser so that it lands in the search box as the first letter.
 assert.equal(focused,1);assert.equal(event.defaultPrevented,false);assert.equal(t.commits.length,0);
});
const handleLayer=t=>t.viewport.children.find(c=>c!==t.artboard);
const sides=t=>handleLayer(t).children.filter(c=>c.dataset?.side).map(c=>c.dataset.side);
test('a single selection carries resize handles and a dragged corner changes only the size',()=>{
 const t=setup();t.down();t.viewport.emit('pointerup',{pointerId:1});
 assert.deepEqual(sides(t),['nw','n','ne','e','se','s','sw','w']);
 const se=handleLayer(t).children.find(c=>c.dataset.side==='se');
 t.viewport.emit('pointerdown',{target:se,button:0,pointerId:3,clientX:0,clientY:0});
 t.viewport.emit('pointermove',{pointerId:3,clientX:40,clientY:20});
 assert.equal(t.commits.length,0);assert.deepEqual(sides(t),[],'the handles step back while the drag runs');
 t.viewport.emit('pointerup',{pointerId:3});
 const node=parse(t.source()).nodes[0].children[0];
 // The fixture box is 100×40 and the canvas is at 200%, so 40×20 screen pixels are 20×10 scene units.
 assert.deepEqual([node.props.width,node.props.height,node.props.x,node.props.y],[120,50,10,20]);
 assert.equal(t.commits.length,1);assert.deepEqual(t.tools.selection(),[node.start]);assert.deepEqual(t.errors,[]);
});
test('a leading edge is clamped to the page and Escape drops the resize without an edit',()=>{
 const t=setup("component Test { Frame { width: 400; Button { key: 'a'; x: 20; y: 20; width: 60; height: 24; } } }");
 t.down();t.viewport.emit('pointerup',{pointerId:1});
 const w=handleLayer(t).children.find(c=>c.dataset.side==='w');
 t.viewport.emit('pointerdown',{target:w,button:0,pointerId:4,clientX:0,clientY:0});
 t.viewport.emit('pointermove',{pointerId:4,clientX:-400,clientY:0});
 t.viewport.emit('pointerup',{pointerId:4});
 const node=parse(t.source()).nodes[0].children[0];
 // The edge stops where the control touches the page border, so 20 of the 200 units are available.
 assert.deepEqual([node.props.x,node.props.width],[0,80]);assert.deepEqual(t.errors,[]);
 t.down();t.viewport.emit('pointerup',{pointerId:1});
 const e=handleLayer(t).children.find(c=>c.dataset.side==='e');const before=t.source();
 t.viewport.emit('pointerdown',{target:e,button:0,pointerId:5,clientX:0,clientY:0});
 t.viewport.emit('pointermove',{pointerId:5,clientX:60,clientY:0});
 window.emit('keydown',{target:t.viewport,key:'Escape'});t.viewport.emit('pointerup',{pointerId:5});
 assert.equal(t.source(),before);assert.deepEqual(sides(t),['nw','n','ne','e','se','s','sw','w']);
});
test('a control whose size the layout owns gets no handles',()=>{
 const t=setup('component Test { Frame { Scroll { Button { key: \'a\'; } } } }');
 t.down();t.viewport.emit('pointerup',{pointerId:1});assert.deepEqual(t.tools.selection().length,1);
 assert.deepEqual(sides(t),[]);
 const grid='component Test { Frame { columns: [100, *]; rows: [40, *]; Button { key: \'a\'; cell: [1, 1]; width: 60; height: 24; } } }';
 const g=setup(grid);g.viewport.emit('pointerdown',{target:g.artboard,button:0,pointerId:1,clientX:20,clientY:20});
 g.viewport.emit('pointerup',{pointerId:1});assert.deepEqual(g.tools.selection().length,1);
 assert.deepEqual(sides(g),[]);
});
