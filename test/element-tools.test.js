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
function setup(initial,inserts=[],extra={}){
 globalThis.document={createElement:()=>new Element()};globalThis.window=new Element();
 const viewport=new Element(),artboard=new Element(),toolbar=new Element();viewport.append(artboard);let source='component Test { Frame { width: 400; Button { x: 10; y: 20; key: \'button\'; } } }';if(initial)source=initial;let selected=null,enabled=true;const commits=[],errors=[],batches=[];
 // The designer's scene is flattened: it draws the leaves with the box the layout gave them and
 // counts them in its own order, so a fake built from the page's children by position would hide
 // exactly the confusion the tools have to survive. A nested leaf is drawn inside its container's
 // corner, and that corner is the one its coordinate is measured against.
 const containers=new Set(['Frame','Scroll','Row','Column','Grid','Stack']);
 // The layout measures a box for a container as well, but this fake can only hand over one the
 // markup sized itself: an intrinsic container takes its extent from the real layout, so it keeps
 // falling back to the union of what it drew.
 const flow=new Set(['Frame','Row','Column','Grid','Stack']);
 const page=()=>{const root=parse(source).nodes[0],children=root?.children??[],top=children[0]?.type==='Scroll'?children[0].children:children,all=[],drawn=[],panels=[];const walk=(nodes,frame)=>{for(const n of nodes){all.push(n);const corner=[frame[0]+(n.props.x??0),frame[1]+(n.props.y??0)];if(containers.has(n.type)){if(n!==root&&flow.has(n.type)&&n.props.width!==undefined&&n.props.height!==undefined)panels.push({start:n.start,bounds:[corner[0],corner[1],n.props.width,n.props.height],coordinateBox:frame});walk(n.children,corner);continue;}drawn.push({start:n.start,bounds:[corner[0],corner[1],n.props.width??100,n.props.height??40],coordinateBox:frame});walk(n.children,corner);}};if(root)walk([root],[0,0]);return {root,top,all,drawn,panels};};
 const context=()=>{if(!enabled)return null;const {root,top,all,drawn,panels}=page();return {source,start:selected,path:'test.ui',root,inserts,top,pick:start=>all.find(n=>n.start===start)??null,state:{},scene:{width:400,height:200,controls:drawn.map((control,index)=>({index,...control})),containers:panels},...extra};};
 const copied=[];
 const tools=createElementTools({viewport,artboard,toolbar,context,select:n=>{selected=n.start;tools.update();batches.push(tools.selection());},commit:c=>{commits.push(c);source=source.slice(0,c.from)+c.insert+source.slice(c.to);selected=c.start;if(c.starts)tools.setSelection(c.starts);},history:()=>{},report:e=>errors.push(e),copy:v=>copied.push(v)});
 // What the host was told to draw its panels from, in the order it was told: a panel has to describe
 // the batch the gesture just made, not the one the previous gesture left. The `update()` inside the
 // handoff is the host's own rebuild path (main.js), which is where a half-finished selection shows.
 return {tools,copied,extra,viewport,artboard,toolbar,commits,errors,batches,source:()=>source,treeSelect:start=>{selected=start;tools.update();},disable:()=>{enabled=false;},down:()=>viewport.emit('pointerdown',{target:artboard,button:0,pointerId:1,clientX:40,clientY:60})};
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
test('the host is told about a choice with the new batch already in place',()=>{
 const t=setup(groupSource),[a,b]=parse(groupSource).nodes[0].children.map(n=>n.start);
 t.down();t.viewport.emit('pointerup',{pointerId:1});
 assert.deepEqual(t.batches.at(-1),[a],'a single click offers one control');
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:2,clientX:320,clientY:180,shiftKey:true});
 assert.deepEqual(t.batches.at(-1),[a,b],'a panel drawn for the choice sees both controls, not the one the last gesture left');
 assert.deepEqual(t.errors,[]);
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
const hintsOf=t=>handleLayer(t).children.filter(c=>c.className==='element-drop-hint').map(c=>[c.style.left,c.style.top,c.style.width,c.style.height]);
const transfer=()=>({setData(){},dropEffect:''});
// The palette button carries the drag, so the entry the designer grabbed is the entry that lands.
const dragFromMenu=t=>{const dt=transfer();t.toolbar.next.children.find(b=>b.dataset.type==='Text').ondragstart({dataTransfer:dt});return dt;};
test('a palette entry dragged onto the page lands at the point it was dropped',()=>{
 const t=setup(undefined,[textItem]);
 assert.equal(t.toolbar.next.children.find(b=>b.dataset.type==='Text').draggable,true);
 const dt=dragFromMenu(t);
 // The default Button paints 10,20 to 110,60, so 300,150 is the page's own empty corner.
 t.viewport.emit('dragover',{target:t.artboard,clientX:600,clientY:300,dataTransfer:dt});
 assert.deepEqual(hintsOf(t),[['0px','0px','800px','400px']],'the list that takes the control is outlined');
 t.viewport.emit('drop',{target:t.artboard,clientX:600,clientY:300,dataTransfer:dt});
 const children=parse(t.source()).nodes[0].children;
 assert.deepEqual(children.map(n=>n.type),['Button','Text']);
 assert.deepEqual([children[1].props.x,children[1].props.y],[300,150]);
 assert.equal(children[1].props.text,'Новый','the entry keeps the markup the palette wrote');
 assert.deepEqual(t.tools.selection(),[children[1].start],'the control the designer just dropped is the one selected');
 assert.deepEqual(hintsOf(t),[]);assert.deepEqual(t.errors,[]);
});
test('a palette entry dropped on a control lands beside it in the same container',()=>{
 const t=setup(groupSource,[textItem]),dt=dragFromMenu(t);
 // b paints 150,80 to 210,120, and its container is the page, so the point is read in page units.
 t.viewport.emit('drop',{target:t.artboard,clientX:360,clientY:200,dataTransfer:dt});
 const children=parse(t.source()).nodes[0].children;
 assert.deepEqual(children.map(n=>n.props.key),['a','b',undefined]);
 assert.deepEqual([children[2].props.x,children[2].props.y],[180,100]);
 assert.equal(t.commits.length,1);assert.deepEqual(t.errors,[]);
});
test('a drop inside a panel that moved off the page writes the coordinate that panel reads',()=>{
 const t=setup(shellSource,[textItem]),dt=dragFromMenu(t);
 // The shell covers 100,50 to 300,170 and its button paints 120,60 to 160,80, so this is its padding:
 // measured through the page the control would land fifty units right of the panel's own corner.
 t.viewport.emit('drop',{target:t.artboard,clientX:300,clientY:300,dataTransfer:dt});
 const shell=parse(t.source()).nodes[0].children[0];
 assert.equal(shell.props.key,'shell');
 assert.deepEqual(shell.children.map(n=>n.type),['Button','Text']);
 assert.deepEqual([shell.children[1].props.x,shell.children[1].props.y],[50,100]);
 assert.deepEqual(t.errors,[]);
});
test('a drop inside a Stack leaves the coordinate out, because the Stack places its children',()=>{
 const t=setup("component Test { Frame { width: 400; Stack { key: 'pile'; x: 50; y: 40; width: 200; height: 120; Button { key: 'a'; x: 20; y: 10; width: 40; height: 20; } } } }",[textItem]),dt=dragFromMenu(t);
 // 120,110 is on the pile's padding, so the hint names the pile rather than the page behind it.
 t.viewport.emit('dragover',{target:t.artboard,clientX:240,clientY:220,dataTransfer:dt});
 assert.deepEqual(hintsOf(t),[['100px','80px','400px','240px']]);
 t.viewport.emit('drop',{target:t.artboard,clientX:240,clientY:220,dataTransfer:dt});
 const pile=parse(t.source()).nodes[0].children[0];
 assert.equal(pile.type,'Stack');
 assert.deepEqual(pile.children.map(n=>n.props.key),['a',undefined]);
 assert.equal(pile.children[1].props.x,undefined);assert.equal(pile.children[1].props.y,undefined);
 assert.deepEqual(t.errors,[]);
});
test('a drag abandoned outside the canvas leaves no hint and no edit',()=>{
 const t=setup(undefined,[textItem]),dt=dragFromMenu(t);
 t.viewport.emit('dragover',{target:t.artboard,clientX:600,clientY:300,dataTransfer:dt});
 assert.equal(hintsOf(t).length,1);
 t.viewport.emit('dragleave',{target:t.artboard,dataTransfer:dt});
 assert.deepEqual(hintsOf(t),[]);
 window.emit('dragend',{target:t.artboard,dataTransfer:dt});
 // With the drag over, the canvas must stop accepting the payload instead of dropping it later.
 const late=t.viewport.emit('dragover',{target:t.artboard,clientX:600,clientY:300,dataTransfer:dt});
 assert.equal(late.defaultPrevented,false);assert.equal(t.commits.length,0);
});
test('a payload the palette did not write is left to the browser',()=>{
 const t=setup(undefined,[textItem]);
 const over=t.viewport.emit('dragover',{target:t.artboard,clientX:600,clientY:300,dataTransfer:{types:['Files'],dropEffect:''}});
 const down=t.viewport.emit('drop',{target:t.artboard,clientX:600,clientY:300,dataTransfer:{types:['Files'],dropEffect:''}});
 assert.equal(over.defaultPrevented,false);assert.equal(down.defaultPrevented,false);
 assert.deepEqual(t.commits,[]);assert.deepEqual(hintsOf(t),[]);
});
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
 // A Scroll flattens to the control inside it, and that control keeps its own size, so
 // unlike a Grid child it does take handles.
 const t=setup('component Test { Frame { Scroll { Button { key: \'a\'; } } } }');
 t.down();t.viewport.emit('pointerup',{pointerId:1});assert.deepEqual(t.tools.selection().length,1);
 assert.deepEqual(sides(t),['nw','n','ne','e','se','s','sw','w']);
 const grid='component Test { Frame { columns: [100, *]; rows: [40, *]; Button { key: \'a\'; cell: [1, 1]; width: 60; height: 24; } } }';
 const g=setup(grid);g.viewport.emit('pointerdown',{target:g.artboard,button:0,pointerId:1,clientX:20,clientY:20});
 g.viewport.emit('pointerup',{pointerId:1});assert.deepEqual(g.tools.selection().length,1);
 assert.deepEqual(sides(g),[]);
});
test('the element menu hands over the CSS of the selected control',()=>{
 const t=setup();t.down();t.viewport.emit('pointerup',{pointerId:1});
 const button=t.toolbar.next.children.find(b=>b.dataset.action==='css');
 assert.equal(button.disabled,false,'the menu offers handoff only for a selection');
 button.onclick();
 // Without the scene's own record of the control there is no box to report, and an invented one
 // would be somebody else's, so the menu says so instead of exporting a size it cannot see.
 assert.deepEqual(t.copied,[],'nothing is copied when the control has no handoff properties');
 assert.match(t.errors.at(-1),/нет свойств, которые переходят в CSS/);
 // The component resolves a radius, a hover colour and the size a `*` grew to; only its visual
 // node carries them, so a handoff that skipped the node would export a card with no corner.
 const start=parse(t.source()).nodes[0].children[0].start;
 t.extra.visuals=[{type:'Surface',source:{file:'test.ui',from:start},bounds:[10,20,380,44],props:{key:'button',radius:6,hoverBackground:{expr:'#a8baff'},transitionDuration:{expr:'80ms'}}}];
 button.onclick();
 // The visual node is what carries the drawn box: the markup says nothing about 380x44.
 assert.equal(t.copied.at(-1),'.button {\n  width: 380px;\n  height: 44px;\n  border-radius: 6px;\n  transition-duration: 80ms;\n}\n.button:hover {\n  background: #a8baff;\n}');
 t.tools.setSelection([]);assert.equal(button.disabled,true);
 assert.equal(t.errors.length,1,'a control whose node the scene drew exports without a complaint');
});
test('a control the scene never drew lends no box to the handoff',()=>{
 const t=setup("component Test { Frame { width: 400; Column { x: 10; y: 20; gap: 10; Button { width: 100; height: 40; key: 'card'; } } } }");
 // The scene draws the Button and counts the Column nowhere, so the designer takes the container
 // from the tree, and the handoff reports only what the Column itself declares.
 t.treeSelect(parse(t.source()).nodes[0].children[0].start);
 [...t.toolbar.next.children].find(b=>b.dataset.action==='css').onclick();
 assert.deepEqual(t.copied,['.column {\n  gap: 10px;\n}'],'the spacing belongs to the container, the 100x40 box to the control inside it');
});
test('a group hands over one block per control',()=>{
 const t=setup(groupSource);t.down();t.viewport.emit('pointerup',{pointerId:1});
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:2,clientX:320,clientY:180,shiftKey:true});
 assert.equal(t.tools.selection().length,2);
 [...t.toolbar.next.children].find(b=>b.dataset.action==='css').onclick();
 // A designer copies the pair they selected, not the first one the menu happened to look at.
 assert.deepEqual(t.copied,['.a {\n  width: 60px;\n}\n\n.b {\n  width: 60px;\n}']);
 assert.deepEqual(t.errors,[]);
});

const nestedSource="component Test { Frame { width: 400; Column { key: 'box'; Button { key: 'a'; x: 40; y: 20; width: 40; height: 20; } Button { key: 'b'; x: 60; y: 80; width: 60; height: 20; } } } }";
test('a click on a nested control selects the control under the pointer, not whoever holds its index',()=>{
 const t=setup(nestedSource);
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:1,clientX:140,clientY:180});
 t.viewport.emit('pointerup',{pointerId:1});
 const box=parse(t.source()).nodes[0].children[0],b=box.children[1];
 assert.equal(b.props.key,'b','the fixture nests the control the pointer is on');
 // The old pairing looked the drawn control up among the page's top-level children by position, so a
 // nested control either selected the wrong node or, as here, selected nothing at all.
 assert.deepEqual(t.tools.selection(),[b.start]);assert.deepEqual(t.errors,[]);
 window.emit('keydown',{target:t.viewport,key:'a',metaKey:true});
 assert.deepEqual(t.tools.selection(),[box.start],'select all takes what the page holds at its top');
 assert.deepEqual(t.errors,[]);
});
const boxesOf=t=>handleLayer(t).children.filter(c=>c.className==='element-selected-box').map(c=>[c.style.left,c.style.top,c.style.width,c.style.height]);
test('a container is measured by the union of what the scene drew inside it',()=>{
 const t=setup(nestedSource);
 const box=parse(t.source()).nodes[0].children[0];
 t.tools.setSelection([box.start]);
 // The Column paints no box of its own, so the outline is what it laid out: 40,20 to 120,100, and the
 // fixture canvas runs at 200%.
 assert.deepEqual(boxesOf(t),[['80px','40px','160px','160px']],'the outline is the children of the Column, not one of them');
 assert.deepEqual(t.errors,[]);
});

// A container that sits away from the page corner measures its children's coordinates from itself,
// so the box the scene drew and the coordinate the markup writes differ by exactly that corner.
const shellSource="component Test { Frame { width: 400; Frame { key: 'shell'; x: 100; y: 50; width: 200; height: 120; Button { key: 'a'; x: 20; y: 10; width: 40; height: 20; } } } }";
const inShell=t=>parse(t.source()).nodes[0].children[0].children[0];
const selectAt=(t,x,y,id=1)=>{t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:id,clientX:x,clientY:y});t.viewport.emit('pointerup',{pointerId:id});};
test('a drag inside a container that moved away from the page writes the coordinate that container reads',()=>{
 const t=setup(shellSource);
 // The button paints at 120,60 of the page and the canvas runs at 200%, so 280,140 is its centre.
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:1,clientX:280,clientY:140});
 t.viewport.emit('pointermove',{pointerId:1,clientX:320,clientY:160});
 t.viewport.emit('pointerup',{pointerId:1});
 assert.equal(t.commits.length,1);
 // Measuring from the page corner would have written the button's place on the page and left it
 // 100 further right than the pointer.
 assert.deepEqual([inShell(t).props.x,inShell(t).props.y],[40,20],'the box lands where the pointer left it');
 assert.deepEqual(t.errors,[]);
});
test('a nudged control stops at the corner its container measures from',()=>{
 const t=setup(shellSource);selectAt(t,280,140);
 for(let i=0;i<3;i++)window.emit('keydown',{target:t.viewport,key:'ArrowLeft',shiftKey:true});
 // Twenty units inside its container, and three steps of ten cross that corner; the page's 120
 // units of room belong to the container, not to the control.
 assert.equal(inShell(t).props.x,0);assert.deepEqual(t.errors,[]);
});
test('a leading edge of a nested control stops at the corner of its container',()=>{
 const t=setup(shellSource);selectAt(t,280,140);
 const w=handleLayer(t).children.find(c=>c.dataset.side==='w');
 t.viewport.emit('pointerdown',{target:w,button:0,pointerId:6,clientX:0,clientY:0});
 t.viewport.emit('pointermove',{pointerId:6,clientX:-400,clientY:0});
 t.viewport.emit('pointerup',{pointerId:6});
 assert.deepEqual([inShell(t).props.x,inShell(t).props.width],[0,60],'the edge travels the 20 units the container allows');
 assert.deepEqual(t.errors,[]);
});
test('a control a Stack lays over its neighbours refuses to move and says why',()=>{
 const t=setup("component Test { Frame { width: 400; Stack { key: 'pile'; x: 50; y: 40; width: 200; height: 120; Button { key: 'a'; x: 20; y: 10; width: 40; height: 20; } } } }");
 const before=t.source();
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:1,clientX:140,clientY:100});
 t.viewport.emit('pointermove',{pointerId:1,clientX:180,clientY:120});
 t.viewport.emit('pointerup',{pointerId:1});
 assert.equal(t.commits.length,0);assert.equal(t.source(),before);
 assert.deepEqual(t.errors,['В Stack положение задаёт раскладка: перенос недоступен']);
});

// A container that sizes itself has an extent of its own even though it paints nothing: the box the
// layout measured for it, which is what a designer means when they pick the panel up in the tree.
const shellOf=t=>parse(t.source()).nodes[0].children[0];
test('a container is measured by the box the layout gave it, not by what it drew inside',()=>{
 const t=setup(shellSource);
 t.tools.setSelection([shellOf(t).start]);
 // The shell covers 100,50 to 300,170 and its button paints 120,60 to 160,80, so the outline of the
 // union is a 40x20 card floating in the panel instead of the panel.
 assert.deepEqual(boxesOf(t),[['200px','100px','400px','240px']]);
 assert.deepEqual(t.errors,[]);
});
// A container the flow placed never wrote a coordinate, so the first nudge measures the new one
// against the corner the canvas has for it — the panel's own, not the one its content covers.
const flowSource="component Test { Frame { width: 400; Frame { key: 'shell'; width: 200; height: 120; Button { key: 'a'; x: 60; y: 40; width: 40; height: 20; } } } }";
test('a container that never wrote a coordinate is placed by its own corner, not by its content',()=>{
 const t=setup(flowSource);
 t.tools.setSelection([shellOf(t).start]);
 window.emit('keydown',{target:t.viewport,key:'ArrowRight'});
 // The button sits sixty units inside the panel: measured through it, one step wrote 61 and left the
 // panel with its content sixty units right of where the layout had put it.
 assert.deepEqual([shellOf(t).props.x,shellOf(t).props.y],[1,0]);
 assert.deepEqual([inShell(t).props.x,inShell(t).props.y],[60,40],'the child keeps its place inside the panel');
 assert.deepEqual(t.errors,[]);
});

// A panel paints nothing, so a plain click on its padding is a click on empty canvas. The modifier is
// how a designer reaches the layer behind the control under the pointer.
const ctrlAt=(t,x,y,id=7)=>t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:id,clientX:x,clientY:y,metaKey:true});
const nestedShells="component Test { Frame { width: 400; Frame { key: 'outer'; width: 360; height: 300; Frame { key: 'shell'; x: 100; y: 50; width: 200; height: 120; Button { key: 'a'; x: 20; y: 10; width: 40; height: 20; } } } } }";
const nodeOf=(t,key)=>{let found;const walk=nodes=>{for(const n of nodes){if(n.props.key===key)found=n;walk(n.children);}};walk(parse(t.source()).nodes);return found;};
test('a click on the modifier picks the container under the point instead of the control it holds',()=>{
 const t=setup(nestedShells);
 // 110,65 of the page is inside the shell's padding and outside its only button; the canvas runs at 200%.
 ctrlAt(t,220,130);t.viewport.emit('pointerup',{pointerId:7});
 assert.deepEqual(t.tools.selection(),[nodeOf(t,'shell').start]);
 assert.deepEqual(t.errors,[]);
});
test('a second click on the modifier steps out to the container that holds the panel',()=>{
 const t=setup(nestedShells);
 ctrlAt(t,220,130);t.viewport.emit('pointerup',{pointerId:7});
 ctrlAt(t,220,130);t.viewport.emit('pointerup',{pointerId:7});
 assert.deepEqual(t.tools.selection(),[nodeOf(t,'outer').start],'the shell sits inside the outer panel');
 // Above the outermost panel there is nothing to step to, so the selection stays where it was rather
 // than falling through to a marquee that would drop it.
 ctrlAt(t,220,130);t.viewport.emit('pointerup',{pointerId:7});
 assert.deepEqual(t.tools.selection(),[nodeOf(t,'outer').start]);
 assert.deepEqual(t.errors,[]);
});
test('a plain click on the padding of a panel still selects nothing the page draws',()=>{
 const t=setup(nestedShells);
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:8,clientX:220,clientY:130});
 t.viewport.emit('pointerup',{pointerId:8});
 assert.deepEqual(t.tools.selection(),[],'without the modifier an empty corner is still empty canvas');
 assert.deepEqual(t.errors,[]);
});
test('a panel grabbed with the modifier travels by the pointer in its own space',()=>{
 const t=setup(nestedShells);
 ctrlAt(t,220,130);
 // Alt keeps the snap away from the gesture, so the delta the designer dragged is the delta written.
 t.viewport.emit('pointermove',{pointerId:7,clientX:260,clientY:150,altKey:true});
 t.viewport.emit('pointerup',{pointerId:7});
 assert.equal(t.commits.length,1);
 assert.deepEqual([nodeOf(t,'shell').props.x,nodeOf(t,'shell').props.y],[120,60]);
 assert.deepEqual(t.errors,[]);
});

// A group and a resize both measure every member, so a container that joins them is measured by the
// extent the layout gave it or the panel loses the size it was written with.
const groupPanels="component Test { Frame { width: 400; Frame { key: 'a'; x: 100; y: 20; width: 200; height: 120; Button { key: 'in'; x: 40; y: 10; width: 40; height: 20; } } Button { key: 'b'; x: 30; y: 200; width: 60; height: 20; } Button { key: 'c'; x: 320; y: 200; width: 40; height: 20; } } }";
const menuRun=(t,action)=>[...t.toolbar.next.children].find(b=>b.dataset.action===action)?.onclick();
const starts=t=>['a','b','c'].map(key=>nodeOf(t,key).start);
const pick=(t,...keys)=>{const list=keys.map(key=>nodeOf(t,key).start);t.treeSelect(list[0]);t.tools.setSelection(list);};
test('aligning a group that holds a panel moves the panel by its own edge',()=>{
 const t=setup(groupPanels);
 // The panel covers 100,20 to 300,140 and its only button paints 140,30 to 180,50, so measuring the
 // panel through its content would have taken the left edge of the group 110 units past the leaf.
 pick(t,'a','b');
 menuRun(t,'left');
 assert.equal(t.commits.length,1);
 assert.deepEqual([nodeOf(t,'a').props.x,nodeOf(t,'b').props.x],[30,30],'both edges land on the same line');
 assert.deepEqual([nodeOf(t,'a').props.y,nodeOf(t,'a').props.height],[20,120],'only the axis the command moves');
 assert.deepEqual(t.errors,[]);
});
test('a row that mixes a panel with leaves spaces them by their own extents',()=>{
 const t=setup(groupPanels);
 // Sorted by their left edges the row is b at 30, the panel at 100 (its button paints from 140) and
 // c at 320: 330 units of span, 300 of them boxes, so 15 units of gap belong between each pair.
 pick(t,'a','b','c');
 menuRun(t,'distribute-x');
 assert.equal(t.commits.length,1);
 const x=key=>nodeOf(t,key).props.x,w=key=>nodeOf(t,key).props.width;
 assert.deepEqual([x('b'),x('a'),x('c')],[30,105,320],'the extremes hold their places');
 assert.equal(x('a')-(x('b')+w('b')),15,'the gap is measured to and from the panel, not its content');
 assert.equal(x('c')-(x('a')+w('a')),15);
 assert.deepEqual(t.errors,[]);
});
test('resizing a panel by its trailing edge writes the size the layout gave it',()=>{
 const t=setup(groupPanels);
 t.treeSelect(starts(t)[0]);
 const e=handleLayer(t).children.find(c=>c.dataset.side==='e');
 t.viewport.emit('pointerdown',{target:e,button:0,pointerId:9,clientX:0,clientY:0});
 t.viewport.emit('pointermove',{pointerId:9,clientX:50,clientY:0});
 t.viewport.emit('pointerup',{pointerId:9});
 // The edge is dragged 25 units out; a panel measured through its 40-wide button would have been
 // written 65 wide and collapsed the 200-unit panel the designer was holding.
 assert.equal(nodeOf(t,'a').props.width,225);
 assert.equal(nodeOf(t,'a').props.x,100,'the trailing edge leaves the corner alone');
 assert.deepEqual(t.errors,[]);
});
test('resizing a panel by its leading edge moves the corner the panel itself holds',()=>{
 const t=setup(groupPanels);
 t.treeSelect(starts(t)[0]);
 const w=handleLayer(t).children.find(c=>c.dataset.side==='w');
 t.viewport.emit('pointerdown',{target:w,button:0,pointerId:10,clientX:0,clientY:0});
 t.viewport.emit('pointermove',{pointerId:10,clientX:-50,clientY:0});
 t.viewport.emit('pointerup',{pointerId:10});
 assert.deepEqual([nodeOf(t,'a').props.x,nodeOf(t,'a').props.width],[75,225],'the panel grows into the page, not into its content');
 assert.deepEqual([nodeOf(t,'in').props.x,nodeOf(t,'in').props.width],[40,40],'the child keeps its own size and place');
 assert.deepEqual(t.errors,[]);
});

// Reparenting by drag: the panel the pointer lets go over takes the control as its own child, and
// the dashed frame is the only way to see that intent before the designer commits to it.
const panelAndLeaf="component Test { Frame { width: 400; Button { key: 'a'; x: 20; y: 20; width: 40; height: 20; } Frame { key: 'box'; x: 200; y: 100; width: 160; height: 100; Text { text: 'x'; } } } }";
const panelStack="component Test { Frame { width: 400; Button { key: 'a'; x: 20; y: 20; width: 40; height: 20; } Stack { key: 'pile'; x: 200; y: 100; width: 160; height: 100; Text { text: 'x'; } } } }";
const moveDrag=(t,from,to,id=1)=>{t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:id,clientX:from[0],clientY:from[1]});t.viewport.emit('pointermove',{pointerId:id,clientX:to[0],clientY:to[1]});return()=>t.viewport.emit('pointerup',{pointerId:id});};
test('a control dragged over another panel is handed to it, at the point that panel reads',()=>{
 const t=setup(panelAndLeaf);
 const release=moveDrag(t,[40,60],[480,280]);
 // The panel covers 200,100 to 360,200 and the pointer sits at 240,140, so this is its padding.
 assert.deepEqual(hintsOf(t),[['400px','200px','320px','200px']],'the panel that will take the control is outlined');
 release();
 const moved=nodeOf(t,'a');
 assert.deepEqual(nodeOf(t,'box').children.map(n=>n.props.key),[undefined,'a'],'the control joins the panel as its last child');
 assert.deepEqual([moved.props.x,moved.props.y],[40,30],'the coordinate counts from the panel, not the page');
 assert.equal(parse(t.source()).nodes[0].children.length,1);
 assert.deepEqual(t.tools.selection(),[moved.start],'the designer still holds the control that moved');
 assert.equal(t.commits.length,1);assert.deepEqual(hintsOf(t),[]);assert.deepEqual(t.errors,[]);
});
test('a drag that stays inside its own panel promises nothing and only moves the control',()=>{
 const t=setup(shellSource);
 const release=moveDrag(t,[240,120],[260,140]);
 assert.deepEqual(hintsOf(t),[],'the panel that already holds the control is not a destination');
 release();
 assert.deepEqual([inShell(t).props.x,inShell(t).props.y],[30,20]);
 assert.equal(t.commits.length,1);assert.deepEqual(t.errors,[]);
});
test('a panel that places its children itself takes the control without writing a point',()=>{
 const t=setup(panelStack);
 const release=moveDrag(t,[40,60],[480,280]);
 assert.deepEqual(hintsOf(t),[['400px','200px','320px','200px']]);
 release();
 assert.deepEqual(nodeOf(t,'pile').children.map(n=>n.type),['Text','Button']);
 assert.deepEqual([nodeOf(t,'a').props.x,nodeOf(t,'a').props.y],[20,20],'the Stack reads no coordinate, so none is invented');
 assert.equal(t.commits.length,1);assert.deepEqual(t.errors,[]);
});
test('a group is never reparented: one drop cannot place several controls at once',()=>{
 const t=setup(groupPanels);
 pick(t,'b','c');
 const release=moveDrag(t,[60,400],[300,160],2);
 assert.deepEqual(hintsOf(t),[],'the panel under the pointer stays unoutlined for a group');
 release();
 assert.deepEqual(parse(t.source()).nodes[0].children.map(n=>n.props.key),['a','b','c']);
 assert.deepEqual(nodeOf(t,'a').children.map(n=>n.props.key),['in']);
 assert.equal(t.commits.length,1);assert.deepEqual(t.errors,[]);
});

// Grouping tells the primitive the box the canvas sees, so the two measurements it needs — the union of
// the selection and the point each member was drawn at — come from the scene, not from the markup.
const pairPage="component Test { Frame { width: 400; Button { key: 'a'; x: 20; y: 20; width: 40; height: 20; } Button { key: 'b'; x: 100; y: 60; width: 40; height: 20; } } }";
test('the menu groups the selection into a panel at the box the canvas measured',()=>{
 const t=setup(pairPage);
 pick(t,'a','b');
 assert.equal([...t.toolbar.next.children].find(b=>b.dataset.action==='group').disabled,false);
 menuRun(t,'group');
 assert.equal(t.commits.length,1);
 const group=parse(t.source()).nodes[0].children[0];
 assert.equal(group.type,'Frame');
 assert.deepEqual([group.props.x,group.props.y,group.props.width,group.props.height],[20,20,120,60]);
 assert.deepEqual(group.children.map(n=>[n.props.key,n.props.x,n.props.y]),[['a',0,0],['b',80,40]]);
 assert.deepEqual(t.tools.selection(),[group.start],'the panel the gesture made is what the designer holds');
 assert.deepEqual(t.errors,[]);
});
test('Ctrl+G groups the batch the marquee picked',()=>{
 const t=setup(pairPage);
 t.viewport.emit('pointerdown',{target:t.artboard,button:0,pointerId:1,clientX:0,clientY:0});
 t.viewport.emit('pointermove',{pointerId:1,clientX:400,clientY:300});
 t.viewport.emit('pointerup',{pointerId:1});
 assert.equal(t.tools.selection().length,2);
 window.emit('keydown',{target:t.viewport,key:'g',metaKey:true});
 assert.equal(t.commits.length,1);
 assert.deepEqual(parse(t.source()).nodes[0].children.map(n=>n.type),['Frame']);
 assert.deepEqual(t.errors,[]);
});
test('a single selection has nothing to group and a Stack places its own children',()=>{
 const t=setup(pairPage);
 t.down();t.viewport.emit('pointerup',{pointerId:1});
 assert.equal([...t.toolbar.next.children].find(b=>b.dataset.action==='group').disabled,true);
 const before=t.source();
 window.emit('keydown',{target:t.viewport,key:'g',metaKey:true});
 assert.equal(t.source(),before);assert.deepEqual(t.commits,[]);
 const pile="component Test { Frame { width: 400; Stack { x: 10; y: 10; width: 200; height: 100; Button { key: 'a'; x: 20; y: 20; width: 40; height: 20; } Button { key: 'b'; x: 60; y: 40; width: 40; height: 20; } } } }";
 const s=setup(pile);
 pick(s,'a','b');
 menuRun(s,'group');
 assert.deepEqual(s.commits,[],'nothing is rewritten where a coordinate reads nothing');
 assert.match(s.errors.at(-1),/В Stack положение задаёт раскладка/);
});
test('a control the canvas never drew cannot be folded into a group',()=>{
 const t=setup("component Test { Frame { width: 400; Button { key: 'a'; x: 20; y: 20; width: 40; height: 20; } Column { gap: 4; } } }");
 const [a,lonely]=parse(t.source()).nodes[0].children;
 t.treeSelect(a.start);t.tools.setSelection([a.start,lonely.start]);
 menuRun(t,'group');
 assert.deepEqual(t.commits,[]);
 assert.match(t.errors.at(-1),/которые рисует холст/);
});
