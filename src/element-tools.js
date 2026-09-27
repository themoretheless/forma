import {createEventScope} from './event-scope.js';
import {copyElements,insertElements,removeElement,moveElement,locateElement,gridCell,reorderElement,editElements} from './element-edit.js';
import {selectionBounds,alignSelection,distributeSelection,snapSelection,intersects} from './selection-layout.js';
export function createElementTools({viewport,artboard,toolbar,context,select,commit,history,report}){
 const events=createEventScope(),listen=events.listen;
 let clipboard='',drag=null,selection=[],primary=null,snapping=true;
 const bar=document.createElement('div');bar.className='element-tools';bar.hidden=true;bar.setAttribute('role','menu');bar.setAttribute('aria-label','Редактирование элементов');
 const labels=[['copy','Копировать'],['cut','Вырезать'],['paste','Вставить'],['duplicate','Дублировать'],['delete','Удалить'],['up','↑ Выше'],['down','↓ Ниже'],['undo','Отменить'],['redo','Повторить'],['snap','Привязки'],['left','По левому краю'],['center','По центру X'],['right','По правому краю'],['top','По верхнему краю'],['middle','По центру Y'],['bottom','По нижнему краю'],['distribute-x','Равные интервалы X'],['distribute-y','Равные интервалы Y']];
 for(const [action,label] of labels){const b=document.createElement('button');b.textContent=label;b.dataset.action=action;b.setAttribute('role',action==='snap'?'menuitemcheckbox':'menuitem');b.tabIndex=-1;b.onclick=()=>{run(action);closeMenu(true);};bar.append(b);}
 const count=document.createElement('span');count.className='selection-count';bar.append(count);toolbar.after(bar);viewport.tabIndex=0;
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
  const hit=[...c.scene.controls].reverse().find(n=>x>=n.bounds[0]&&y>=n.bounds[1]&&x<=n.bounds[0]+n.bounds[2]&&y<=n.bounds[1]+n.bounds[3]);
  const node=hit&&c.nodes[hit.index];if(node&&!selection.includes(node.start))choose([node.start],c);
  openMenu(e.clientX,e.clientY);
 },true);
 listen(window,'pointerdown',e=>{if(!bar.contains(e.target))closeMenu();},true);
 listen(window,'blur',()=>closeMenu());
 listen(window,'resize',()=>closeMenu());
 listen(bar,'keydown',e=>{
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
 function current(){const c=context();if(!c)return null;if(c.start!==primary){primary=c.start;selection=c.nodes.some(n=>n.start===c.start)?[c.start]:[];}selection=selection.filter(start=>c.nodes.some(n=>n.start===start));return c;}
 function items(c,starts=selection){return starts.map(start=>{const index=c.nodes.findIndex(n=>n.start===start),control=c.scene.controls.find(n=>n.index===index);return control?{start,bounds:control.bounds}:null;}).filter(Boolean);}
 function setSelection(starts){const c=context();primary=c?.start;selection=[...new Set(starts??[])].filter(start=>c?.nodes.some(n=>n.start===start));update();}
 function choose(starts,c){const n=c.nodes.find(n=>n.start===starts.at(-1));if(n)select(n);else if(c.root)select(c.root);primary=context()?.start;selection=[...starts];update();}
 function apply(change,c){if(change)commit(change,c);}
 function origin(c,item){return [item.bounds[0]+(c.scene.scrollOffset?.[0]??0),item.bounds[1]+(c.scene.scrollOffset?.[1]??0)];}
 function moves(c,entries){return editElements(c.source,entries.map(e=>e.start),start=>{const e=entries.find(e=>e.start===start),item=items(c,[start])[0];const cell=e.cell??(c.grid?gridCell(c.grid,item.bounds[0]+e.dx+item.bounds[2]/2,item.bounds[1]+e.dy+item.bounds[3]/2):null);return moveElement(c.source,start,e.dx,e.dy,cell,origin(c,item));});}
 function run(action,text){try{
  const c=current();if(!c)return;
  if(action==='snap'){snapping=!snapping;return;}
  if(action==='undo'||action==='redo'){history(action);return;}
  if(c.start==null)throw Error('Сначала выберите контрол или контейнер');
  if(action==='up'||action==='down'){if(selection.length!==1)return;apply(reorderElement(c.source,selection[0],action==='up'?-1:1),c);}
  if(action==='copy'||action==='cut'){if(!selection.length)return;clipboard=copyElements(c.source,selection);if(action==='copy')return clipboard;}
  if(action==='delete'||action==='cut')apply(editElements(c.source,selection,start=>removeElement(c.source,start)),c);
  if(action==='duplicate'){
   if(!selection.length)return;
   let copied=copyElements(c.source,selection);
   if(!c.grid){const changes=items(c).map(item=>{const n=locateElement(c.source,item.start).node;const moved=moveElement(c.source,item.start,16,16,null,origin(c,item));return {start:n.start,text:moved.insert};});copied=changes.sort((a,b)=>a.start-b.start).map(c=>c.text).join('\n');}
   apply(insertElements(c.source,Math.max(...selection),copied),c);
  }
  if(action==='paste')apply(insertElements(c.source,c.start,text??clipboard),c);
  if(alignments.has(action)){
   if(c.grid)throw Error('В Grid положение задаётся ячейками; выравнивание доступно в свободной раскладке');
   const list=items(c),entries=action.startsWith('distribute-')?distributeSelection(list,action.endsWith('x')?0:1):alignSelection(list,action);apply(moves(c,entries),c);
  }
 }catch(e){report(e.message);}finally{update();}}
 function geometry(){const r=artboard.getBoundingClientRect(),p=viewport.getBoundingClientRect();const c=context();return {x:r.left-p.left+viewport.scrollLeft,y:r.top-p.top+viewport.scrollTop,scale:r.width/(artboard.offsetWidth||c?.scene.width||r.width)};}
 function box(bounds,className){const g=geometry(),el=document.createElement('div');el.className=className;Object.assign(el.style,{left:g.x+bounds[0]*g.scale+'px',top:g.y+bounds[1]*g.scale+'px',width:bounds[2]*g.scale+'px',height:bounds[3]*g.scale+'px'});overlay.append(el);}
 function draw(){overlay.replaceChildren();const c=context();if(!c)return;
  for(const item of items(c))box(item.bounds,'element-selected-box');
  if(!drag)return;
  if(drag.kind==='marquee'){box(drag.marquee,'element-marquee');return;}
  if(!drag.moved)return;
  for(const item of drag.items)box([item.bounds[0]+drag.dx,item.bounds[1]+drag.dy,item.bounds[2],item.bounds[3]],'element-drag-preview');
  const g=geometry();for(const guide of drag.guides??[]){const line=document.createElement('div');line.className='element-snap-guide';Object.assign(line.style,guide.axis===0?{left:g.x+guide.position*g.scale+'px',top:g.y+'px',height:c.scene.height*g.scale+'px'}:{left:g.x+'px',top:g.y+guide.position*g.scale+'px',width:c.scene.width*g.scale+'px'});overlay.append(line);}
  const bounds=selectionBounds(drag.items.map(i=>i.bounds));const label=document.createElement('span');label.className='element-drag-label';label.textContent=`Δx ${Math.round(drag.dx)} · Δy ${Math.round(drag.dy)}`;Object.assign(label.style,{left:g.x+(bounds[0]+drag.dx)*g.scale+'px',top:g.y+(bounds[1]+drag.dy)*g.scale-22+'px'});overlay.append(label);
 }
 function cancel(){const d=drag;drag=null;if(d&&viewport.hasPointerCapture(d.id))viewport.releasePointerCapture(d.id);draw();}
 listen(viewport,'pointerdown',e=>{
  const c=current();if(!c||e.button!==0||e.detail>1||e.target.closest('.grid-handle')||viewport.classList.contains('canvas-pan-ready'))return;
  if(!artboard.contains(e.target)&&e.target!==viewport)return;
  const r=artboard.getBoundingClientRect(),scale=geometry().scale,x=(e.clientX-r.left)/scale,y=(e.clientY-r.top)/scale;
  const hit=[...c.scene.controls].reverse().find(n=>x>=n.bounds[0]&&y>=n.bounds[1]&&x<=n.bounds[0]+n.bounds[2]&&y<=n.bounds[1]+n.bounds[3]);
  const n=hit?c.nodes[hit.index]:null;e.preventDefault();e.stopImmediatePropagation();viewport.focus({preventScroll:true});
  if(n&&e.shiftKey){choose(selection.includes(n.start)?selection.filter(s=>s!==n.start):[...selection,n.start],c);return;}
  if(n){if(!selection.includes(n.start))choose([n.start],c);const list=items(c);drag={...c,start:primary,kind:'move',id:e.pointerId,x:e.clientX,y:e.clientY,scale,items:list,dx:0,dy:0,moved:false};}
  else drag={...c,kind:'marquee',id:e.pointerId,x:e.clientX,y:e.clientY,scale,origin:[x,y],marquee:[x,y,0,0],initial:e.shiftKey?[...selection]:[],moved:false};
  viewport.setPointerCapture(e.pointerId);
 },true);
 listen(viewport,'pointermove',e=>{
  if(!drag||e.pointerId!==drag.id)return;e.preventDefault();e.stopImmediatePropagation();
  let dx=(e.clientX-drag.x)/drag.scale,dy=(e.clientY-drag.y)/drag.scale;drag.moved=Math.hypot(dx,dy)*drag.scale>=3;
  if(drag.kind==='marquee'){drag.marquee=[drag.origin[0]+Math.min(0,dx),drag.origin[1]+Math.min(0,dy),Math.abs(dx),Math.abs(dy)];draw();return;}
  const bounds=selectionBounds(drag.items.map(i=>i.bounds));drag.guides=[];
  if(snapping&&!e.altKey&&!drag.grid){const others=drag.scene.controls.filter(c=>!drag.items.some(i=>drag.nodes[c.index]?.start===i.start)).map(c=>c.bounds);const snapped=snapSelection(bounds,dx,dy,others,[drag.scene.width,drag.scene.height],6/drag.scale);dx=snapped.dx;dy=snapped.dy;drag.guides=snapped.guides;}
  // Clamp the whole group together so its internal spacing is preserved.
  if(!drag.grid){dx=Math.max(dx,-bounds[0]-(drag.scene.scrollOffset?.[0]??0));dy=Math.max(dy,-bounds[1]-(drag.scene.scrollOffset?.[1]??0));}
  drag.dx=dx;drag.dy=dy;draw();
 },true);
 listen(viewport,'pointerup',e=>{
  if(!drag||e.pointerId!==drag.id)return;e.stopImmediatePropagation();const d=drag;cancel();
  if(d.kind==='marquee'){const hit=d.moved?items(d,d.nodes.map(n=>n.start)).filter(i=>intersects(i.bounds,d.marquee)).map(i=>i.start):[];choose([...new Set([...d.initial,...hit])],d);return;}
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
  if(mod&&key==='a'){e.preventDefault();const c=current();choose(c.nodes.map(n=>n.start),c);return;}
  if(e.key==='Escape'){e.preventDefault();choose([],current());return;}
  if(e.altKey&&!mod&&(e.key==='ArrowUp'||e.key==='ArrowDown')){e.preventDefault();run(e.key==='ArrowUp'?'up':'down');return;}
  if(mod&&key==='z'){e.preventDefault();run(e.shiftKey?'redo':'undo');return;}
  if(mod&&key==='y'){e.preventDefault();run('redo');return;}
  if(mod&&key==='d'){e.preventDefault();run('duplicate');return;}
  if(e.key==='Delete'||e.key==='Backspace'){e.preventDefault();run('delete');return;}
  const direction={ArrowLeft:[-1,0],ArrowRight:[1,0],ArrowUp:[0,-1],ArrowDown:[0,1]}[e.key];
  if(!direction||mod||e.altKey)return;e.preventDefault();
  try{const c=current(),list=items(c);if(!list.length)return;const step=e.shiftKey?10:1,bounds=selectionBounds(list.map(i=>i.bounds));
   const dx=Math.max(direction[0]*step,-bounds[0]-(c.scene.scrollOffset?.[0]??0)),dy=Math.max(direction[1]*step,-bounds[1]-(c.scene.scrollOffset?.[1]??0));
   let columnDelta=direction[0],rowDelta=direction[1];
   const cells=c.grid?list.map(i=>{const n=locateElement(c.source,i.start).node;return {column:n.props.cell?.[1]??n.props.column??1,row:n.props.cell?.[0]??n.props.row??1};}):[];
   if(c.grid){columnDelta=Math.max(1-Math.min(...cells.map(c=>c.column)),Math.min(columnDelta,c.grid.columns.length-Math.max(...cells.map(c=>c.column))));rowDelta=Math.max(1-Math.min(...cells.map(c=>c.row)),Math.min(rowDelta,c.grid.rows.length-Math.max(...cells.map(c=>c.row))));}
   apply(moves(c,list.map((i,index)=>({start:i.start,dx,dy,cell:c.grid?{column:cells[index].column+columnDelta,row:cells[index].row+rowDelta}:null}))),c);
  }catch(error){report(error.message);}
 });
 for(const action of ['copy','cut'])listen(window,action,e=>{if(!scope(e)||!current())return;const value=run('copy');if(!value)return;e.preventDefault();e.clipboardData.setData('text/plain',value);if(action==='cut')run('delete');});
 listen(window,'paste',e=>{if(!scope(e)||!current())return;e.preventDefault();run('paste',e.clipboardData.getData('text/plain'));});
 function update(){const c=current();if(!c)closeMenu();let location;try{if(c?.start!=null)location=locateElement(c.source,c.start);}catch{}
  count.textContent=selection.length?`Выбрано: ${selection.length}`:'';
  for(const b of bar.children){const action=b.dataset.action;if(!action)continue;b.disabled=!c||(!['undo','redo','snap'].includes(action)&&c.start==null);
   if(['copy','cut','delete','duplicate'].includes(action))b.disabled=!c||!selection.length;
   if(action==='snap'){b.setAttribute('aria-checked',String(snapping));b.title='Края и центры · Alt при переносе отключает привязку';}
   if(alignments.has(action))b.disabled=!c||!!c.grid||selection.length<(action.startsWith('distribute-')?3:2);
   if(action==='up'||action==='down'){const siblings=location?.parent?.children??[],index=siblings.indexOf(location?.node);b.disabled=!c||selection.length!==1||index<0||location?.node.type==='Scroll'||(action==='up'?index===0:index===siblings.length-1);b.title='Порядок среди соседей · Alt+'+(action==='up'?'↑':'↓');}
  }draw();
 }
 update();return {update,cancel,setSelection,selection:()=>[...selection],destroy(){events.dispose();cancel();closeMenu();bar.remove();overlay.remove();}};
}
