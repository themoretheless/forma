export function resizeTracks(values,sizes,index,delta){
  const next=values.map(v=>v?.expr??v);
  if(index<0||index>=sizes.length-1)throw Error('Нет соседней колонки');
  const total=sizes[index]+sizes[index+1],first=Math.max(0,Math.min(total,sizes[index]+delta));
  next[index]=Math.round(first*10)/10;next[index+1]=Math.round((total-first)*10)/10;
  return next;
}
export function trackSource(values){return '['+values.map(v=>{
 const value=v?.expr??v;
 if(typeof value==='number'&&Number.isFinite(value)&&value>=0)return String(value);
 if(typeof value==='string'&&/^(?:(?:props|state)\.[A-Za-z_][\w.]*|auto|content|-|(?:\d+(?:\.\d+)?)?\*|\d+(?:\.\d+)?(?:%|px))$/.test(value))return value;
 throw Error('Размер трека нельзя редактировать визуально');
}).join(', ')+']';}
export function parseTracks(text){
 const raw=text.trim();if(!raw.startsWith('[')||!raw.endsWith(']'))throw Error('Ожидается список [120, *, auto]');
 const values=raw.slice(1,-1).split(',').map(v=>v.trim());
 if(values.some(v=>!v))throw Error('Пустой трек');
 return values.map(v=>/^\d+(?:\.\d+)?$/.test(v)?Number(v):v);
}
export function createLayoutInspector({viewport,artboard,toolbar,getVisuals,getRuntime,getControl,getMode,edit,openSource,readTracks}){
 let chosen=null,drag=null;
 const panel=document.createElement('details');panel.className='layout-inspector';panel.innerHTML='<summary>Внутренние части и Grid</summary><nav class="layout-breadcrumbs" aria-label="Путь к части"></nav><div class="layout-parts"></div><div class="layout-grid"></div>';
 const toggle=document.createElement('button');toggle.className='layout-toggle';toggle.textContent='Части и Grid';toggle.setAttribute('aria-expanded','false');
 toolbar.append(toggle);(document.querySelector('.inspector')??toolbar.parentElement).append(panel);
 toggle.onclick=()=>{panel.open=!panel.open;toggle.setAttribute('aria-expanded',String(panel.open));};
 panel.addEventListener('toggle',()=>toggle.setAttribute('aria-expanded',String(panel.open)));
 const overlay=document.createElement('div');overlay.className='layout-overlay';viewport.append(overlay);
 const breadcrumbs=panel.querySelector('.layout-breadcrumbs');
 const parts=panel.querySelector('.layout-parts'),grid=panel.querySelector('.layout-grid');
 function candidates(){return getVisuals().filter(v=>v.control===getControl());}
 function geometry(v,bounds=v.bounds){
  const r=artboard.getBoundingClientRect(),p=viewport.getBoundingClientRect(),runtime=getRuntime(),control=(v.control===-1?{bounds:[0,0,runtime?.width??0,runtime?.height??0]}:runtime?.controls[v.control]);
  if(!control)return null;const scale=r.width/runtime.width;
  return {x:r.left-p.left+viewport.scrollLeft+(control.bounds[0]+bounds[0])*scale,y:r.top-p.top+viewport.scrollTop+(control.bounds[1]+bounds[1])*scale,w:bounds[2]*scale,h:bounds[3]*scale,scale};
 }
 function draw(){
  overlay.replaceChildren();if(!panel.open||getMode()!=='design'||!chosen)return;
  const rect=geometry(chosen);if(!rect)return;
  const box=document.createElement('div');box.className='layout-part-box';Object.assign(box.style,{left:rect.x+'px',top:rect.y+'px',width:rect.w+'px',height:rect.h+'px'});overlay.append(box);
  if(!chosen.grid)return;
  const area=geometry(chosen,chosen.grid.bounds);
  for(const axis of ['columns','rows']){
   if(!chosen.propertySources?.[axis])continue;
   const sizes=chosen.grid[axis],vertical=axis==='columns';let offset=0;
   sizes.slice(0,-1).forEach((size,index)=>{
    offset+=size;
    const handle=document.createElement('button');handle.className='grid-handle '+(vertical?'vertical':'horizontal');handle.title='Перетащить границу; соседние треки станут фиксированными. Escape — отменить';handle.setAttribute('aria-label',`Граница ${axis==='columns'?'колонок':'строк'} ${index+1}`);
    Object.assign(handle.style,vertical?{left:area.x+(offset+index*chosen.grid.gap[1])*area.scale-3+'px',top:area.y+'px',height:area.h+'px'}:{left:area.x+'px',top:area.y+(offset+index*chosen.grid.gap[0])*area.scale-3+'px',width:area.w+'px'});
    handle.onpointerdown=e=>{e.preventDefault();e.stopPropagation();drag={v:chosen,axis,index,start:vertical?e.clientX:e.clientY,scale:area.scale,delta:0,handle};handle.setPointerCapture(e.pointerId);};
    handle.onpointermove=e=>{if(drag?.handle!==handle)return;drag.delta=((vertical?e.clientX:e.clientY)-drag.start)/drag.scale;handle.style.transform=vertical?`translateX(${drag.delta*drag.scale}px)`:`translateY(${drag.delta*drag.scale}px)`;};
    handle.onpointerup=e=>{if(drag?.handle!==handle)return;const d=drag;drag=null;try{if(d.delta===0)return;const values=readTracks(d.v.propertySources[d.axis]);edit(d.v.propertySources[d.axis],trackSource(resizeTracks(values,d.v.grid[d.axis],d.index,d.delta)));}finally{draw();}};
    handle.onpointercancel=()=>{drag=null;draw();};
    handle.onkeydown=e=>{const step=({ArrowLeft:-1,ArrowUp:-1,ArrowRight:1,ArrowDown:1})[e.key];if(step===undefined)return;e.preventDefault();const source=chosen.propertySources[axis];edit(source,trackSource(resizeTracks(readTracks(source),sizes,index,step*(e.shiftKey?10:1))));};
    overlay.append(handle);
   });
  }
 }
 function choose(v){chosen=v;render();}
 function render(){
  if(!panel.open){overlay.replaceChildren();return;}
  parts.replaceChildren();grid.replaceChildren();breadcrumbs.replaceChildren();overlay.replaceChildren();
  const list=candidates();
  if(!list.includes(chosen))chosen=list.find(v=>v.source?.file===chosen?.source?.file&&v.source?.from===chosen?.source?.from)||list.find(v=>v.grid&&v.propertySources?.columns)||list[0];
  const chain=[];let current=chosen;
  while(current){chain.unshift(current);current=list.find(v=>v.id===current.parent);}
  for(const v of chain){const crumb=document.createElement('button');crumb.textContent=v.type;crumb.onclick=()=>choose(v);breadcrumbs.append(crumb);}
  for(const v of list){
   const b=document.createElement('button');b.textContent=v.type+(v.source?' · '+v.source.file.split('/').at(-1):'');b.setAttribute('aria-pressed',String(v===chosen));b.onclick=()=>choose(v);parts.append(b);
  }
  if(!list.length){parts.textContent='Выберите компонент на холсте. Двойной щелчок — внутренняя часть.';return;}
  if(chosen?.source){const b=document.createElement('button');b.textContent='К определению ↗';b.onclick=()=>openSource(chosen.source);grid.append(b);}
  for(const axis of ['columns','rows']){
   const source=chosen?.propertySources?.[axis];if(!source)continue;
   const label=document.createElement('label');label.textContent=axis==='columns'?'Колонки ':'Строки ';
   const input=document.createElement('input');input.setAttribute('aria-label',label.textContent.trim());input.value=trackSource(readTracks(source));
   input.onchange=()=>{try{edit(source,trackSource(parseTracks(input.value)));input.setCustomValidity('');}catch(error){input.setCustomValidity(error.message);input.reportValidity();}};label.append(input);grid.append(label);
  }
  if(chosen?.grid){const note=document.createElement('small');note.textContent='Размеры из раскладки. Перетаскивание фиксирует только два соседних трека; остальные сохраняются.';grid.append(note);}
  draw();
 }
 viewport.addEventListener('dblclick',event=>{
  if(getMode()!=='design')return;
  const p=viewport.getBoundingClientRect(),x=event.clientX-p.left+viewport.scrollLeft,y=event.clientY-p.top+viewport.scrollTop;
  const hits=candidates().filter(v=>{const b=geometry(v);return b&&x>=b.x&&y>=b.y&&x<=b.x+b.w&&y<=b.y+b.h;});
  if(hits.length){panel.open=true;choose(hits.at(-1));event.preventDefault();}
 });
 panel.addEventListener('toggle',render);viewport.addEventListener('scroll',()=>{if(!drag)draw();});
 window.addEventListener('keydown',e=>{if(e.key==='Escape'&&drag){drag=null;draw();}});
 new ResizeObserver(draw).observe(viewport);
 return {update(){if(!drag)render();}};
}
