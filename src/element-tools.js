import {copyElement,insertElement,removeElement,moveElement,locateElement,gridCell,reorderElement} from './element-edit.js';
export function createElementTools({viewport,artboard,toolbar,context,select,commit,history,report}){
 let clipboard='',drag=null;
 const bar=document.createElement('div');bar.className='element-tools';bar.setAttribute('role','group');bar.setAttribute('aria-label','Редактирование элементов');
 for(const [action,label] of [['copy','Копировать'],['cut','Вырезать'],['paste','Вставить'],['duplicate','Дублировать'],['delete','Удалить'],['up','↑ Выше'],['down','↓ Ниже'],['undo','Отменить'],['redo','Повторить']]){
  const b=document.createElement('button');b.textContent=label;b.dataset.action=action;b.onclick=()=>run(action);bar.append(b);
 }toolbar.after(bar);viewport.tabIndex=0;
 const ghost=document.createElement('div');ghost.className='element-drag-preview';ghost.hidden=true;viewport.append(ghost);
 const editable=e=>e.target.closest?.('input,textarea,select,[contenteditable="true"]');
 function run(action,text){try{
  const c=context();if(!c)return;
  if(action==='undo'||action==='redo'){history(action);return;}
  if(c.start==null)throw Error('Сначала выберите контрол или контейнер');
  if(action==='up'||action==='down'){const change=reorderElement(c.source,c.start,action==='up'?-1:1);if(change)commit(change,c);}
  if(action==='copy'||action==='cut'){clipboard=copyElement(c.source,c.start);if(action==='copy')return clipboard;}
  if(action==='delete'||action==='cut')commit(removeElement(c.source,c.start),c);
  if(action==='duplicate'){
   const copied=copyElement(c.source,c.start),change=insertElement(c.source,c.start,copied);
   const {parent}=locateElement(c.source,c.start);
   if(parent.props.columns===undefined&&parent.props.rows===undefined){
    const combined=c.source.slice(0,change.from)+change.insert+c.source.slice(change.to);
    const b=c.scene.controls[c.nodes.findIndex(n=>n.start===c.start)]?.bounds??[0,0];
    const moved=moveElement(combined,change.start,16,16,null,[b[0]+(c.scene.scrollOffset?.[0]??0),b[1]+(c.scene.scrollOffset?.[1]??0)]);
    change.insert='\n'+moved.insert+'\n';
   }
   commit(change,c);
  }
  if(action==='paste')commit(insertElement(c.source,c.start,text??clipboard),c);
 }catch(e){report(e.message);}finally{update();}}
 function cancel(){if(drag&&viewport.hasPointerCapture(drag.id))viewport.releasePointerCapture(drag.id);drag=null;ghost.hidden=true;}
 viewport.addEventListener('pointerdown',e=>{
  const c=context();if(!c||e.button!==0||e.detail>1||e.target.closest('.grid-handle')||viewport.classList.contains('canvas-pan-ready'))return;
  if(!artboard.contains(e.target))return;
  const r=artboard.getBoundingClientRect(),scale=r.width/(artboard.offsetWidth||c.scene.width),x=(e.clientX-r.left)/scale,y=(e.clientY-r.top)/scale;
  const hit=[...c.scene.controls].reverse().find(n=>x>=n.bounds[0]&&y>=n.bounds[1]&&x<=n.bounds[0]+n.bounds[2]&&y<=n.bounds[1]+n.bounds[3]);
  if(!hit)return;const n=c.nodes[hit.index];if(!n)return;
  e.preventDefault();e.stopImmediatePropagation();select(n);viewport.focus({preventScroll:true});
  drag={...c,start:n.start,id:e.pointerId,x:e.clientX,y:e.clientY,scale,bounds:hit.bounds,dx:0,dy:0};viewport.setPointerCapture(e.pointerId);
 },true);
 viewport.addEventListener('pointermove',e=>{
  if(!drag||e.pointerId!==drag.id)return;e.preventDefault();e.stopImmediatePropagation();
  drag.dx=(e.clientX-drag.x)/drag.scale;drag.dy=(e.clientY-drag.y)/drag.scale;
  const r=artboard.getBoundingClientRect(),p=viewport.getBoundingClientRect(),b=drag.bounds;
  ghost.hidden=Math.hypot(drag.dx,drag.dy)*drag.scale<3;
  Object.assign(ghost.style,{left:r.left-p.left+viewport.scrollLeft+(b[0]+drag.dx)*drag.scale+'px',top:r.top-p.top+viewport.scrollTop+(b[1]+drag.dy)*drag.scale+'px',width:b[2]*drag.scale+'px',height:b[3]*drag.scale+'px'});
 },true);
 viewport.addEventListener('pointerup',e=>{
  if(!drag||e.pointerId!==drag.id)return;e.stopImmediatePropagation();const d=drag,moved=!ghost.hidden;cancel();if(!moved)return;
  try{const g=d.grid;const cell=g?gridCell(g,d.bounds[0]+d.dx+d.bounds[2]/2,d.bounds[1]+d.dy+d.bounds[3]/2):null;commit(moveElement(d.source,d.start,d.dx,d.dy,cell,[d.bounds[0]+(d.scene.scrollOffset?.[0]??0),d.bounds[1]+(d.scene.scrollOffset?.[1]??0)]),d);}catch(e){report(e.message);}
 },true);
 viewport.addEventListener('pointercancel',cancel);window.addEventListener('blur',cancel);
 window.addEventListener('keydown',e=>{if(e.key==='Escape'&&drag){e.preventDefault();cancel();}});
 const scope=e=>!editable(e)&&(viewport.contains(e.target)||bar.contains(e.target));
 window.addEventListener('keydown',e=>{
  if(!scope(e)||!context())return;const mod=e.metaKey||e.ctrlKey,key=e.key.toLowerCase();
  if(e.altKey&&!mod&&(e.key==='ArrowUp'||e.key==='ArrowDown')){e.preventDefault();run(e.key==='ArrowUp'?'up':'down');return;}
  if(mod&&key==='z'){e.preventDefault();run(e.shiftKey?'redo':'undo');return;}
  if(mod&&key==='y'){e.preventDefault();run('redo');return;}
  if(mod&&key==='d'){e.preventDefault();run('duplicate');return;}
  if(e.key==='Delete'||e.key==='Backspace'){e.preventDefault();run('delete');return;}
  const direction={ArrowLeft:[-1,0],ArrowRight:[1,0],ArrowUp:[0,-1],ArrowDown:[0,1]}[e.key];
  if(!direction||mod||e.altKey)return;e.preventDefault();
  try{const c=context();if(c.start==null)return;const {node,parent}=locateElement(c.source,c.start);let cell=null;
   if(c.grid&&(parent.props.columns!==undefined||parent.props.rows!==undefined)){const col=node.props.cell?.[1]??node.props.column??1,row=node.props.cell?.[0]??node.props.row??1;cell={column:Math.max(1,Math.min(c.grid.columns.length,col+direction[0])),row:Math.max(1,Math.min(c.grid.rows.length,row+direction[1]))};}
   const step=e.shiftKey?10:1;commit(moveElement(c.source,c.start,direction[0]*step,direction[1]*step,cell,(()=>{const b=c.scene.controls[c.nodes.findIndex(n=>n.start===c.start)]?.bounds??[0,0];return [b[0]+(c.scene.scrollOffset?.[0]??0),b[1]+(c.scene.scrollOffset?.[1]??0)];})()),c);
  }catch(error){report(error.message);}
 });
 for(const action of ['copy','cut'])window.addEventListener(action,e=>{if(!scope(e)||!context())return;const value=run('copy');if(!value)return;e.preventDefault();e.clipboardData.setData('text/plain',value);if(action==='cut')run('delete');});
 window.addEventListener('paste',e=>{if(!scope(e)||!context())return;e.preventDefault();run('paste',e.clipboardData.getData('text/plain'));});
 function update(){const c=context();let location;try{if(c?.start!=null)location=locateElement(c.source,c.start);}catch{}
  for(const b of bar.children){b.disabled=!c||(!['undo','redo'].includes(b.dataset.action)&&c.start==null);
   if(b.dataset.action==='up'||b.dataset.action==='down'){const siblings=location?.parent?.children??[],index=siblings.indexOf(location?.node);b.disabled=!c||index<0||location?.node.type==='Scroll'||(b.dataset.action==='up'?index===0:index===siblings.length-1);b.title='Порядок среди соседей · Alt+'+(b.dataset.action==='up'?'↑':'↓');}
  }
 }
 update();return {update,cancel};
}
