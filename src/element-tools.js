import {createEventScope} from './event-scope.js';
import {parse} from './language.js';
import {copyElements,copyElement,insertElements,insertElement,removeElement,moveElement,locateElement,gridCell,reorderElement,editElements,holdsChildren,resizeElement,resizeBlock} from './element-edit.js';
import {selectionBounds,alignSelection,distributeSelection,snapSelection,intersects} from './selection-layout.js';
import {controlCss} from './handoff.js';
export function createElementTools({viewport,artboard,toolbar,context,select,commit,history,report,copy=text=>navigator.clipboard?.writeText(text)}){
 const events=createEventScope(),listen=events.listen;
 let clipboard='',drag=null,selection=[],primary=null,snapping=true,resize=null;
 const bar=document.createElement('div');bar.className='element-tools';bar.hidden=true;bar.setAttribute('role','menu');bar.setAttribute('aria-label','Редактирование элементов');
 const labels=[['copy','Копировать'],['cut','Вырезать'],['paste','Вставить'],['duplicate','Дублировать'],['delete','Удалить'],['up','↑ Выше'],['down','↓ Ниже'],['undo','Отменить'],['redo','Повторить'],['snap','Привязки'],['left','По левому краю'],['center','По центру X'],['right','По правому краю'],['top','По верхнему краю'],['middle','По центру Y'],['bottom','По нижнему краю'],['distribute-x','Равные интервалы X'],['distribute-y','Равные интервалы Y'],['css','Скопировать CSS']];
 for(const [action,label] of labels){const b=document.createElement('button');b.textContent=label;b.dataset.action=action;b.setAttribute('role',action==='snap'?'menuitemcheckbox':'menuitem');b.tabIndex=-1;b.onclick=()=>{run(action);closeMenu(true);};bar.append(b);}
 const count=document.createElement('span');count.className='selection-count';bar.append(count);toolbar.after(bar);viewport.tabIndex=0;
 // The palette is the only way to create a control the page does not have yet, so its entries
 // are the types the page compiler takes as a top-level control: the project's components plus
 // the built-in fallbacks. A design system has dozens of them, so the group carries a filter
 // that typing anywhere in the menu jumps into.
 const filter=document.createElement('input');filter.type='search';filter.className='element-add-filter';
 filter.placeholder='Добавить контрол…';filter.setAttribute('aria-label','Поиск по палитре контролов');
 filter.oninput=()=>{query=filter.value;renderAdds();};
 filter.onkeydown=e=>{if(e.key==='Enter'){e.preventDefault();addButtons.find(b=>!b.disabled)?.onclick?.();}};
 let catalog=[],query='',addSignature='',addButtons=[];
 const matches=item=>{const needle=query.trim().toLocaleLowerCase();return !needle||item.type.toLocaleLowerCase().includes(needle);};
 function renderAdds(){
  const items=catalog.filter(matches),key=query.toLocaleLowerCase()+'\u0000'+items.map(item=>item.type).join(',');
  if(key===addSignature)return;addSignature=key;
  for(const button of addButtons)button.remove();addButtons=[];
  for(const item of items){
   const b=document.createElement('button');b.textContent='＋ '+item.type;b.className='element-add';b.dataset.action='add';b.dataset.type=item.type;
   b.setAttribute('role','menuitem');b.tabIndex=-1;b.title='В выбранный контейнер или после выбранного контрола';
   b.onclick=()=>{run('add',item.markup);closeMenu(true);};bar.append(b);addButtons.push(b);
  }
  if(catalog.length){count.remove();filter.hidden=false;bar.append(filter);bar.append(count);}
  else filter.remove();
 }
 // A changed project palette is a new list, so an old query must not hide every entry of it.
 let catalogKey='';
 function drawAdds(items){
  const list=items??[],key=list.map(item=>item.type+item.markup.length).join(',');
  if(key!==catalogKey){catalogKey=key;catalog=list;query='';filter.value='';}
  renderAdds();
 }
 function closeMenu(restore=false){if(bar.hidden)return;bar.hidden=true;if(restore)viewport.focus({preventScroll:true});}
 function openMenu(x,y){
  update();bar.hidden=false;
  const bounds=bar.getBoundingClientRect();
  bar.style.left=Math.max(8,Math.min(x,(window.innerWidth??1024)-bounds.width-8))+'px';
  bar.style.top=Math.max(8,Math.min(y,(window.innerHeight??768)-bounds.height-8))+'px';
  [...bar.children].find(b=>b.dataset.action&&!b.disabled)?.focus({preventScroll:true});
 }
 listen(viewport,'contextmenu',e=>{
  const c=current();if(!c||editable(e))return;e.preventDefault();e.stopPropagation();cancel();
  const r=artboard.getBoundingClientRect(),scale=geometry().scale,x=(e.clientX-r.left)/scale,y=(e.clientY-r.top)/scale;
  // The menu follows the same modifier as the press, so a control-click that reached a panel keeps
  // its commands for that panel instead of the leaf the pointer happens to sit on.
  const start=(e.metaKey||e.ctrlKey)?shellAt(c,x,y,null):hitAt(c,x,y);if(start!=null&&!selection.includes(start))choose([start],c);
  openMenu(e.clientX,e.clientY);
 },true);
 listen(window,'pointerdown',e=>{if(!bar.contains(e.target))closeMenu();},true);
 listen(window,'blur',()=>closeMenu());
 listen(window,'resize',()=>closeMenu());
 listen(bar,'keydown',e=>{
  // The menu is mostly the palette, so a printable key searches it instead of doing nothing.
  if(e.key.length===1&&!e.metaKey&&!e.ctrlKey&&!e.altKey&&!filter.hidden&&document.activeElement!==filter)filter.focus();
  if(e.key==='Escape'||e.key==='Tab'){e.preventDefault();e.stopImmediatePropagation();closeMenu(true);return;}
  if(!['ArrowDown','ArrowUp','Home','End'].includes(e.key))return;
  e.preventDefault();e.stopImmediatePropagation();
  const buttons=[...bar.children].filter(b=>b.dataset.action&&!b.disabled),index=buttons.indexOf(document.activeElement);
  const next=e.key==='Home'?0:e.key==='End'?buttons.length-1:(index+(e.key==='ArrowDown'?1:-1)+buttons.length)%buttons.length;
  buttons[next]?.focus({preventScroll:true});
 });
 const overlay=document.createElement('div');overlay.className='element-selection-overlay';viewport.append(overlay);
 const editable=e=>e.target.closest?.('input,textarea,select,[contenteditable="true"]');
 const alignments=new Set(['left','center','right','top','middle','bottom','distribute-x','distribute-y']);
 // The scene counts the controls it drew and the markup nests them, so pairing the two by position
 // handed a selected container the box of whoever happened to sit at that index. Every measure goes
 // through the source offset the drawing carries instead: a control takes its own box, a container the
 // box the layout measured for it, and only a container the scene never sized falls back to the union
 // of what it drew. A node the page no longer has gets none of them.
 let measure=null;
 function measured(c){
  if(measure?.scene===c.scene&&measure.source===c.source)return measure;
  const nodes=new Map();try{const stack=[...parse(c.source).nodes];while(stack.length){const n=stack.pop();nodes.set(n.start,n);stack.push(...(n.children??[]));}}catch{}
  // A drawing can carry the offset of a node from a component file, and that is not something this page's
  // markup can point at, so only a box the page itself owns is measured, hit or snapped against.
  const drawn=new Map(),panels=new Map(),frames=new Map();
  for(const control of c.scene.controls)if(nodes.has(control.start)&&!drawn.has(control.start)){drawn.set(control.start,control.bounds);frames.set(control.start,control.coordinateBox??[0,0,0,0]);}
  for(const panel of c.scene.containers??[])if(nodes.has(panel.start)&&!drawn.has(panel.start)&&!panels.has(panel.start)){panels.set(panel.start,panel.bounds);frames.set(panel.start,panel.coordinateBox??[0,0,0,0]);}
  return measure={scene:c.scene,source:c.source,drawn,panels,frames,nodes,unions:new Map()};
 }
 function boxOf(c,start){
  const m=measured(c),own=m.drawn.get(start)??m.panels.get(start);if(own)return own;
  if(m.unions.has(start))return m.unions.get(start);
  const boxes=[],stack=[...(m.nodes.get(start)?.children??[])];
  while(stack.length){const n=stack.pop(),b=m.drawn.get(n.start);if(b)boxes.push(b);else stack.push(...(n.children??[]));}
  const union=boxes.length?selectionBounds(boxes):null;m.unions.set(start,union);return union;
 }
 function hitAt(c,x,y){const drawn=measured(c).drawn,control=[...c.scene.controls].reverse().find(n=>drawn.has(n.start)&&x>=n.bounds[0]&&y>=n.bounds[1]&&x<=n.bounds[0]+n.bounds[2]&&y<=n.bounds[1]+n.bounds[3]);return control?.start??null;}
 // A panel paints nothing, so it is the layer a designer reaches for when the pointer is already on
 // something they do not want: with the modifier the press takes the deepest container whose own box
 // covers the point, and from a selected node it steps out to the container that holds it.
 function shellAt(c,x,y,from){
  const m=measured(c),covers=b=>!!b&&x>=b[0]&&y>=b[1]&&x<=b[0]+b[2]&&y<=b[1]+b[3];
  if(from==null){const under=[...m.panels].filter(([,box])=>covers(box)).sort((a,b)=>a[1][2]*a[1][3]-b[1][2]*b[1][3]);return under.length?under[0][0]:null;}
  const parentOf=start=>{try{return locateElement(c.source,start).parent;}catch{return null;}};
  for(let node=parentOf(from);node;node=parentOf(node.start))if(covers(m.panels.get(node.start)))return node.start;
  return null;
 }
 // A dragged container carries its children with it, so none of them is a snap target of its own.
 function covered(c,starts){const m=measured(c),set=new Set(starts);for(const start of starts)for(const stack=[...(m.nodes.get(start)?.children??[])];stack.length;){const n=stack.pop();set.add(n.start);stack.push(...(n.children??[]));}return set;}
 // What the page holds is its own markup, so a control the designer can still reach from the tree is
 // never dropped from the selection for want of a box the scene drew for it.
 function known(c,start){return measured(c).nodes.has(start);}
 function current(){const c=context();if(!c)return null;if(c.start!==primary){primary=c.start;selection=c.start==null?[]:[c.start];}selection=selection.filter(start=>known(c,start));return c;}
 function items(c,starts=selection){return starts.map(start=>{const bounds=boxOf(c,start);return bounds?{start,bounds}:null;}).filter(Boolean);}
 function setSelection(starts){const c=context();primary=c?.start;selection=[...new Set(starts??[])].filter(start=>c&&known(c,start));update();}
 function choose(starts,c){const n=c.pick?.(starts.at(-1))??measured(c).nodes.get(starts.at(-1));if(n)select(n);else if(c.root)select(c.root);primary=context()?.start;selection=[...starts];update();}
 function apply(change,c){if(change)commit(change,c);}
 // The layout measures a control's own coordinate from the corner of the container that flows it,
 // which is the page corner only for a control sitting directly on the page.
 function origin(c,item){const frame=measured(c).frames.get(item.start)??[0,0,0,0],scroll=c.scene.scrollOffset??[0,0];
  return [item.bounds[0]+scroll[0]-frame[0],item.bounds[1]+scroll[1]-frame[1]];}
 // A coordinate cannot fall behind the corner it is measured from, so a group travels only as far
 // as its tightest member allows.
 function room(c,list){const from=list.map(i=>origin(c,i));return [Math.min(...from.map(o=>o[0])),Math.min(...from.map(o=>o[1]))];}
 // A container takes the new node as its last child and a leaf gets it as the next sibling,
 // which is the row the designer is looking at. With nothing selected the page's own control
 // list is the target so that the first control of an empty page has somewhere to go, and a
 // scrolling page must gain it inside the Scroll rather than beside it.
 function addTarget(c){
  if(c.start==null){const children=c.root?.children??[],only=children.length===1?children[0]:null;return {start:(only?.type==='Scroll'?only:c.root).start,side:'inside'};}
  const {node,parent}=locateElement(c.source,c.start);
  return holdsChildren(node)?{start:node.start,side:'inside'}:{start:node.start,side:parent?'after':'inside'};
 }
 // A control's own visual node carries what the component resolved — a radius, a hover colour, the
 // size the layout gave a `*` — so that, and not the markup the page happened to write, is what a
 // handoff has to spell out. A container that paints nothing has no box of its own, and a size
 // invented for it would be somebody else's, so the block says only what the control declares.
 function handoffBlock(c,start){
  const visual=(c.visuals??[]).find(v=>v.source?.file===c.path&&v.source.from===start);
  return visual?controlCss(visual,visual.bounds,c.state):controlCss(locateElement(c.source,start).node,null,c.state);
 }
 function moves(c,entries){return editElements(c.source,entries.map(e=>e.start),start=>{const e=entries.find(e=>e.start===start),item=items(c,[start])[0];const cell=e.cell??(c.grid?gridCell(c.grid,item.bounds[0]+e.dx+item.bounds[2]/2,item.bounds[1]+e.dy+item.bounds[3]/2):null);return moveElement(c.source,start,e.dx,e.dy,cell,origin(c,item));});}
 function run(action,text){try{
  const c=current();if(!c)return;
  if(action==='snap'){snapping=!snapping;return;}
  if(action==='undo'||action==='redo'){history(action);return;}
  if(c.start==null&&action!=='add')throw Error('Сначала выберите контрол или контейнер');
  if(action==='up'||action==='down'){if(selection.length!==1)return;apply(reorderElement(c.source,selection[0],action==='up'?-1:1),c);}
  if(action==='add'){
   if(!text)return;
   const target=addTarget(c);
   apply(insertElement(c.source,target.start,text,false,target.side),c);
  }
  if(action==='copy'||action==='cut'){if(!selection.length)return;clipboard=copyElements(c.source,selection);if(action==='copy')return clipboard;}
  if(action==='delete'||action==='cut')apply(editElements(c.source,selection,start=>removeElement(c.source,start)),c);
  if(action==='duplicate'){
   if(!selection.length)return;
   let copied=copyElements(c.source,selection);
   if(!c.grid){const changes=selection.map(start=>{const n=locateElement(c.source,start).node,item=items(c,[start])[0];
    // An offset is measured from the box the scene drew, so a node the page keeps but never paints
    // gets its duplicate where it stands rather than at a position nobody chose.
    const moved=item?moveElement(c.source,start,16,16,null,origin(c,item)):{insert:copyElement(c.source,start)};return {start:n.start,text:moved.insert};});copied=changes.sort((a,b)=>a.start-b.start).map(c=>c.text).join('\n');}
   apply(insertElements(c.source,Math.max(...selection),copied),c);
  }
  if(action==='paste')apply(insertElements(c.source,c.start,text??clipboard),c);
  if(action==='css'){
   if(!selection.length)return;
   const blocks=selection.map(start=>handoffBlock(c,start)).filter(Boolean).join('\n\n');
   if(!blocks)throw Error('В выбранном контроле нет свойств, которые переходят в CSS');
   copy(blocks);return blocks;
  }
  if(alignments.has(action)){
   if(c.grid)throw Error('В Grid положение задаётся ячейками; выравнивание доступно в свободной раскладке');
   const list=items(c),entries=action.startsWith('distribute-')?distributeSelection(list,action.endsWith('x')?0:1):alignSelection(list,action);apply(moves(c,entries),c);
  }
 }catch(e){report(e.message);}finally{update();}}
 function geometry(){const r=artboard.getBoundingClientRect(),p=viewport.getBoundingClientRect();const c=context();return {x:r.left-p.left+viewport.scrollLeft,y:r.top-p.top+viewport.scrollTop,scale:r.width/(artboard.offsetWidth||c?.scene.width||r.width)};}
 function box(bounds,className){const g=geometry(),el=document.createElement('div');el.className=className;Object.assign(el.style,{left:g.x+bounds[0]*g.scale+'px',top:g.y+bounds[1]*g.scale+'px',width:bounds[2]*g.scale+'px',height:bounds[3]*g.scale+'px'});overlay.append(el);}
 // A corner drags one side per axis, so the unit vector doubles as the set of moving edges.
 const sides=[['nw',0,0,'nwse-resize'],['n',.5,0,'ns-resize'],['ne',1,0,'nesw-resize'],['e',1,.5,'ew-resize'],['se',1,1,'nwse-resize'],['s',.5,1,'ns-resize'],['sw',0,1,'nesw-resize'],['w',0,.5,'ew-resize']];
 const unit=name=>({left:name.includes('w')?1:0,right:name.includes('e')?1:0,top:name.includes('n')?1:0,bottom:name.includes('s')?1:0});
 function handles(bounds){const g=geometry();
  for(const [name,fx,fy,cursor] of sides){const el=document.createElement('button');el.className='element-resize-handle';el.dataset.side=name;el.title='Потянуть, чтобы изменить размер';el.setAttribute('aria-label',`Изменить размер: ${name}`);el.tabIndex=-1;
   Object.assign(el.style,{left:g.x+(bounds[0]+bounds[2]*fx)*g.scale+'px',top:g.y+(bounds[1]+bounds[3]*fy)*g.scale+'px',cursor});overlay.append(el);}
 }
 function draw(){overlay.replaceChildren();const c=context();if(!c)return;
  for(const item of items(c))box(item.bounds,'element-selected-box');
  if(!drag&&resize!=null)for(const item of items(c))handles(item.bounds);
  if(!drag)return;
  if(drag.kind==='marquee'){box(drag.marquee,'element-marquee');return;}
  if(drag.kind==='resize'){const g=geometry(),b=drag.preview,label=document.createElement('span');label.className='element-drag-label';
   label.textContent=`${Math.round(b[2])} × ${Math.round(b[3])}`;box(b,'element-drag-preview');
   Object.assign(label.style,{left:g.x+b[0]*g.scale+'px',top:g.y+b[1]*g.scale-22+'px'});overlay.append(label);return;}
  if(!drag.moved)return;
  for(const item of drag.items)box([item.bounds[0]+drag.dx,item.bounds[1]+drag.dy,item.bounds[2],item.bounds[3]],'element-drag-preview');
  const g=geometry();for(const guide of drag.guides??[]){const line=document.createElement('div');line.className='element-snap-guide';Object.assign(line.style,guide.axis===0?{left:g.x+guide.position*g.scale+'px',top:g.y+'px',height:c.scene.height*g.scale+'px'}:{left:g.x+'px',top:g.y+guide.position*g.scale+'px',width:c.scene.width*g.scale+'px'});overlay.append(line);}
  const bounds=selectionBounds(drag.items.map(i=>i.bounds));const label=document.createElement('span');label.className='element-drag-label';label.textContent=`Δx ${Math.round(drag.dx)} · Δy ${Math.round(drag.dy)}`;Object.assign(label.style,{left:g.x+(bounds[0]+drag.dx)*g.scale+'px',top:g.y+(bounds[1]+drag.dy)*g.scale-22+'px'});overlay.append(label);
 }
 function cancel(){const d=drag;drag=null;if(d&&viewport.hasPointerCapture(d.id))viewport.releasePointerCapture(d.id);draw();}
 listen(viewport,'pointerdown',e=>{
  const c=current();if(!c||e.button!==0||e.detail>1||e.target.closest('.grid-handle')||viewport.classList.contains('canvas-pan-ready'))return;
  if(!artboard.contains(e.target)&&e.target!==viewport){
   const side=e.target?.dataset?.side;
   // A handle belongs to the selection rather than to the control underneath it, so the drag
   // starts from the rendered box the designer sees, not from whatever the pointer is over.
   if(side&&resize!=null){const item=items(c)[0];if(!item)return;
    e.preventDefault();e.stopImmediatePropagation();viewport.focus({preventScroll:true});
    const g=geometry();drag={...c,kind:'resize',id:e.pointerId,start:item.start,unit:unit(side),bounds:item.bounds,base:origin(c,item),x:e.clientX,y:e.clientY,scale:g.scale,edges:{left:0,right:0,top:0,bottom:0},preview:item.bounds,moved:false};
    viewport.setPointerCapture(e.pointerId);draw();}
   return;}
  const r=artboard.getBoundingClientRect(),scale=geometry().scale,x=(e.clientX-r.left)/scale,y=(e.clientY-r.top)/scale;
  const stepped=e.metaKey||e.ctrlKey,from=!stepped||e.shiftKey||selection.length!==1?null:selection[0];
  const hit=stepped?shellAt(c,x,y,from):hitAt(c,x,y);e.preventDefault();e.stopImmediatePropagation();viewport.focus({preventScroll:true});
  // Above the outermost panel there is nothing to step to, and a press that finds it must leave the
  // designer holding what they had rather than fall through to a marquee that drops the selection.
  if(stepped&&hit==null&&from!=null)return;
  if(hit!=null&&e.shiftKey){choose(selection.includes(hit)?selection.filter(s=>s!==hit):[...selection,hit],c);return;}
  if(hit!=null){if(!selection.includes(hit))choose([hit],c);const list=items(c);drag={...c,start:primary,kind:'move',id:e.pointerId,x:e.clientX,y:e.clientY,scale,items:list,slack:room(c,list),dx:0,dy:0,moved:false};}
  else drag={...c,kind:'marquee',id:e.pointerId,x:e.clientX,y:e.clientY,scale,origin:[x,y],marquee:[x,y,0,0],initial:e.shiftKey?[...selection]:[],moved:false};
  viewport.setPointerCapture(e.pointerId);
 },true);
 listen(viewport,'pointermove',e=>{
  if(!drag||e.pointerId!==drag.id)return;e.preventDefault();e.stopImmediatePropagation();
  let dx=(e.clientX-drag.x)/drag.scale,dy=(e.clientY-drag.y)/drag.scale;drag.moved=Math.hypot(dx,dy)*drag.scale>=3;
  if(drag.kind==='marquee'){drag.marquee=[drag.origin[0]+Math.min(0,dx),drag.origin[1]+Math.min(0,dy),Math.abs(dx),Math.abs(dy)];draw();return;}
  if(drag.kind==='resize'){const b=drag.bounds,u=drag.unit,base=drag.base;
   // A side cannot cross the opposite one, and a leading edge cannot push the control past the
   // corner its own coordinate is measured from.
   const left=u.left?Math.max(Math.min(dx,b[2]),-base[0]):0,right=u.right?Math.max(dx,-b[2]):0;
   const top=u.top?Math.max(Math.min(dy,b[3]),-base[1]):0,bottom=u.bottom?Math.max(dy,-b[3]):0;
   drag.edges={left,right,top,bottom};
   drag.preview=[b[0]+left,b[1]+top,Math.max(0,b[2]+right-left),Math.max(0,b[3]+bottom-top)];draw();return;}
  const bounds=selectionBounds(drag.items.map(i=>i.bounds));drag.guides=[];
  if(snapping&&!e.altKey&&!drag.grid){const skip=drag.covered??(drag.covered=covered(drag,drag.items.map(i=>i.start)));const others=drag.scene.controls.filter(c=>!skip.has(c.start)).map(c=>c.bounds);const snapped=snapSelection(bounds,dx,dy,others,[drag.scene.width,drag.scene.height],6/drag.scale);dx=snapped.dx;dy=snapped.dy;drag.guides=snapped.guides;}
  // Clamp the whole group together so its internal spacing is preserved.
  if(!drag.grid){dx=Math.max(dx,-drag.slack[0]);dy=Math.max(dy,-drag.slack[1]);}
  drag.dx=dx;drag.dy=dy;draw();
 },true);
 listen(viewport,'pointerup',e=>{
  if(!drag||e.pointerId!==drag.id)return;e.stopImmediatePropagation();const d=drag;cancel();
  if(d.kind==='marquee'){
   // A marquee picks what the page draws, so it is the drawn controls that are tested against it: a
   // container has an extent even where it paints nothing, and that would let one sweep over a leaf
   // drag the whole panel in whenever the designer meant the leaf.
   const drawn=[...measured(d).drawn.keys()];
   const hit=d.moved?items(d,drawn).filter(i=>intersects(i.bounds,d.marquee)).map(i=>i.start):[];
   choose([...new Set([...d.initial,...hit])],d);return;}
  if(d.kind==='resize'){if(!d.moved)return;
   try{apply(resizeElement(d.source,d.start,d.edges,d.bounds,d.base),d);}catch(error){report(error.message);}return;}
  if(!d.moved)return;try{
   let cells=[];
   if(d.grid){
    cells=d.items.map(i=>{const n=locateElement(d.source,i.start).node,cell=gridCell(d.grid,i.bounds[0]+i.bounds[2]/2,i.bounds[1]+i.bounds[3]/2);return {column:n.props.cell?.[1]??n.props.column??cell.column,row:n.props.cell?.[0]??n.props.row??cell.row};});
    const b=d.items[0].bounds,target=gridCell(d.grid,b[0]+d.dx+b[2]/2,b[1]+d.dy+b[3]/2);
    const dc=Math.max(1-Math.min(...cells.map(c=>c.column)),Math.min(target.column-cells[0].column,d.grid.columns.length-Math.max(...cells.map(c=>c.column))));
    const dr=Math.max(1-Math.min(...cells.map(c=>c.row)),Math.min(target.row-cells[0].row,d.grid.rows.length-Math.max(...cells.map(c=>c.row))));
    cells=cells.map(c=>({column:c.column+dc,row:c.row+dr}));
   }
   apply(moves(d,d.items.map((i,index)=>({start:i.start,dx:d.dx,dy:d.dy,cell:cells[index]}))),d);
  }catch(error){report(error.message);}
 },true);
 listen(viewport,'pointercancel',cancel);listen(window,'blur',cancel);listen(viewport,'scroll',draw);
 listen(window,'keydown',e=>{if(e.key==='Escape'&&drag){e.preventDefault();e.stopImmediatePropagation();cancel();}});
 const scope=e=>!editable(e)&&(viewport.contains(e.target)||bar.contains(e.target));
 listen(window,'keydown',e=>{
  if(!scope(e)||!current())return;
  if(e.key==='ContextMenu'||(e.shiftKey&&e.key==='F10')){e.preventDefault();const r=viewport.getBoundingClientRect();openMenu(r.left+24,r.top+24);return;}
  const mod=e.metaKey||e.ctrlKey,key=e.key.toLowerCase();
  if(mod&&key==='a'){e.preventDefault();const c=current();choose(c.top.map(n=>n.start),c);return;}
  if(e.key==='Escape'){e.preventDefault();choose([],current());return;}
  if(e.altKey&&!mod&&(e.key==='ArrowUp'||e.key==='ArrowDown')){e.preventDefault();run(e.key==='ArrowUp'?'up':'down');return;}
  if(mod&&key==='z'){e.preventDefault();run(e.shiftKey?'redo':'undo');return;}
  if(mod&&key==='y'){e.preventDefault();run('redo');return;}
  if(mod&&key==='d'){e.preventDefault();run('duplicate');return;}
  if(e.key==='Delete'||e.key==='Backspace'){e.preventDefault();run('delete');return;}
  const direction={ArrowLeft:[-1,0],ArrowRight:[1,0],ArrowUp:[0,-1],ArrowDown:[0,1]}[e.key];
  if(!direction||mod||e.altKey)return;e.preventDefault();
  try{const c=current(),list=items(c);if(!list.length)return;const step=e.shiftKey?10:1,slack=room(c,list);
   const dx=Math.max(direction[0]*step,-slack[0]),dy=Math.max(direction[1]*step,-slack[1]);
   let columnDelta=direction[0],rowDelta=direction[1];
   const cells=c.grid?list.map(i=>{const n=locateElement(c.source,i.start).node;return {column:n.props.cell?.[1]??n.props.column??1,row:n.props.cell?.[0]??n.props.row??1};}):[];
   if(c.grid){columnDelta=Math.max(1-Math.min(...cells.map(c=>c.column)),Math.min(columnDelta,c.grid.columns.length-Math.max(...cells.map(c=>c.column))));rowDelta=Math.max(1-Math.min(...cells.map(c=>c.row)),Math.min(rowDelta,c.grid.rows.length-Math.max(...cells.map(c=>c.row))));}
   apply(moves(c,list.map((i,index)=>({start:i.start,dx,dy,cell:c.grid?{column:cells[index].column+columnDelta,row:cells[index].row+rowDelta}:null}))),c);
  }catch(error){report(error.message);}
 });
 for(const action of ['copy','cut'])listen(window,action,e=>{if(!scope(e)||!current())return;const value=run('copy');if(!value)return;e.preventDefault();e.clipboardData.setData('text/plain',value);if(action==='cut')run('delete');});
 listen(window,'paste',e=>{if(!scope(e)||!current())return;e.preventDefault();run('paste',e.clipboardData.getData('text/plain'));});
 function update(){const c=current();if(!c)closeMenu();let location;try{if(c?.start!=null)location=locateElement(c.source,c.start);}catch{}
  resize=selection.length===1&&location&&!resizeBlock(location.node,location.parent)?selection[0]:null;
  drawAdds(c?.inserts);
  count.textContent=selection.length?`Выбрано: ${selection.length}`:'';
  for(const b of bar.children){const action=b.dataset.action;if(!action)continue;b.disabled=!c||(!['undo','redo','snap','add'].includes(action)&&c.start==null);
   if(action==='add')b.disabled=!c||!c.root||!(c.inserts??[]).some(item=>item.type===b.dataset.type);
   if(['copy','cut','delete','duplicate','css'].includes(action))b.disabled=!c||!selection.length;
   if(action==='snap'){b.setAttribute('aria-checked',String(snapping));b.title='Края и центры · Alt при переносе отключает привязку';}
   if(alignments.has(action))b.disabled=!c||!!c.grid||selection.length<(action.startsWith('distribute-')?3:2);
   if(action==='up'||action==='down'){const siblings=location?.parent?.children??[],index=siblings.indexOf(location?.node);b.disabled=!c||selection.length!==1||index<0||location?.node.type==='Scroll'||(action==='up'?index===0:index===siblings.length-1);b.title='Порядок среди соседей · Alt+'+(action==='up'?'↑':'↓');}
  }draw();
 }
 update();return {update,cancel,setSelection,selection:()=>[...selection],destroy(){events.dispose();cancel();closeMenu();bar.remove();overlay.remove();}};
}
