import './minimal-shell.css';
import {createEventScope} from './event-scope.js';
import {controlLabel} from './control-label.js';
import {studioEventTypes} from './studio-events.js';
import {loadVectorRuntime} from './vector-preview.js';
import {createShellRegion,keyText} from './forma-shell.js';

// Minimal layout: only the canvas, the state tabs and the selection stay on screen.
// The chrome itself is Forma markup (`studio-ui/`) drawn by the Rust/WASM runtime. The old
// DOM header and toolbars stay hidden in place as command targets, so commands, tests and the
// MCP bridge keep driving the same elements.
export function mountMinimalShell(root,{events,newFile,clearSelection,inspector}={}){
 const $=selector=>root.querySelector(selector);
 const body=document.body,header=$('header'),run=$('#run');
 body.classList.add('minimal','show-layers');
 const scope=createEventScope(),listen=scope.listen,observe=scope.observe;
 const on=(type,callback)=>events.on(type,callback,{signal:scope.signal});
 const panels={code:'show-code',layers:'show-layers',problems:'show-problems'};
 const toggle=(name,force)=>{body.classList.toggle(panels[name],force);if(name==='code'&&body.classList.contains('show-code'))(root.querySelector('.cm-content')??$('#code'))?.focus();};
 const appButton=()=>$('#app-run');
 const scenario=$('#scenario');
 let regions=null,destroyed=false;
 const name=controlLabel;
 const usable=el=>!el.disabled&&!el.closest('.forma-palette,#preview')&&name(el);
 // Everything the header held besides the title, the preview toggle and the launch button.
 const menuItems=()=>[...header.querySelectorAll('button,a')].filter(el=>el!==run&&el!==appButton()&&!el.closest('.brand,.project-title')&&usable(el)).map(el=>({label:name(el),run:()=>el.click()}));

 // Preview interaction is secondary; the visible app button owns the main launch shortcut.
 let running=false;
 const label=()=>{const text=running?'Дизайн':'Взаимодействие';if(run.textContent!==text)run.textContent=text;run.title=running?'Вернуться в дизайн (Esc)':'Проверить взаимодействие во встроенном preview';body.classList.toggle('running',running);draw();};
 on(studioEventTypes.mode,({mode})=>{running=mode==='interact';label();});
 observe(run,()=>{if(!['Взаимодействие','Дизайн'].includes(run.textContent))label();},{childList:true,characterData:true,subtree:true},{immediate:false});

 // Inspector only while something is selected; debugger only while running.
 on(studioEventTypes.selection,({selected})=>body.classList.toggle('has-selection',selected));

 // Problems: quiet when clean, the drawer opens when the count grows.
 let problemCount=0,saveText='';
 on(studioEventTypes.problems,({count})=>{if(count>problemCount)toggle('problems',true);problemCount=count;draw();});
 on(studioEventTypes.save,({status,text})=>{saveText=status==='unsaved'||status==='damaged'?text:'';draw();});

 // Palette shared by ⌘K (commands), ⌘P (files), / (insert) and the project title (menu).
 const palette=document.createElement('div');palette.className='forma-palette';palette.hidden=true;
 palette.setAttribute('role','dialog');palette.setAttribute('aria-modal','true');
 body.append(palette);
 let items=[],shown=[],index=0,query='',placeholder='';
 function commands(){
  const app=appButton();
  const out=[
   {label:'Показать / скрыть код',key:'⌘\\',run:()=>toggle('code')},
   {label:'Показать / скрыть слои и файлы',key:'L',run:()=>toggle('layers')},
   {label:'Показать / скрыть проблемы и журнал',run:()=>toggle('problems')},
   ...(app?[{label:'Запустить приложение',key:'⌘↵',run:()=>app.click()}]:[]),
   {label:run.title,key:app?undefined:'⌘↵',run:()=>run.click()},
  ];
  if(newFile)out.push({label:'Новый файл',run:newFile});
  out.push(...menuItems());
  for(const el of root.querySelectorAll('.studio-canvas-toolbar button,.preview-tools button,.canvas-tools button,.bottom-tabs button,.tabs button'))if(usable(el)&&el!==run)out.push({label:name(el),run:()=>el.click()});
  for(const select of root.querySelectorAll('#app-platform,#app-backend,.studio-canvas-toolbar select,.canvas-tools select,.pane-toolbar select'))if(select!==scenario&&!select.disabled)for(const option of select.options)out.push({label:`${select.getAttribute('aria-label')||'Выбор'}: ${option.textContent}`,run:()=>{select.value=option.value;select.dispatchEvent(new Event('change'));}});
  const seen=new Set();return out.filter(c=>!seen.has(c.label)&&seen.add(c.label));
 }
 const files=()=>[...root.querySelectorAll('#tree [data-path]')].map(el=>({label:el.dataset.path,run:()=>{el.click();toggle('code',true);}}));
 const inserts=()=>[...root.querySelectorAll('.element-tools .element-add')].filter(usable).map(el=>({label:name(el).replace(/^＋\s*/,''),run:()=>el.click()}));
 // The runtime draws a bounded scene, so the list is a window of rows around the highlighted one.
 const visibleRows=9;
 function filter(){
  const q=query.trim().toLowerCase();
  shown=items.filter(item=>q.split(/\s+/).every(part=>item.label.toLowerCase().includes(part)));
  index=Math.min(index,Math.max(0,shown.length-1));
 }
 function paletteState(){
  const first=Math.max(0,Math.min(index-Math.floor(visibleRows/2),shown.length-visibleRows));
  return {search:true,placeholder,query,items:shown.slice(first,first+visibleRows).map((item,i)=>({id:first+i,label:item.label,hint:keyText(item.key),active:first+i===index}))};
 }
 function openPalette(kind){
  items=kind==='files'?files():kind==='insert'?inserts():kind==='menu'?menuItems():commands();
  if(kind==='insert'&&!items.length){items=commands().filter(c=>/добав|встав|insert/i.test(c.label));}
  placeholder=kind==='files'?'Открыть файл…':kind==='insert'?'Добавить элемент…':kind==='menu'?'Проект…':'Команда…';
  query='';index=0;palette.hidden=false;body.classList.add('used-shortcut');filter();draw();
  regions?.palette.focus('query');
 }
 const closePalette=()=>{if(palette.hidden)return;palette.hidden=true;};
 function pick(i){const item=shown[i];closePalette();item?.run();}
 // The search field edits inside Rust; the host only steers the highlight and closes.
 listen(palette,'keydown',e=>{
  if(e.key==='ArrowDown'){index=Math.min(shown.length-1,index+1);}
  else if(e.key==='ArrowUp'){index=Math.max(0,index-1);}
  else if(e.key==='Enter'){pick(index);}
  else if(e.key==='Escape'){closePalette();}
  else return;
  e.preventDefault();e.stopPropagation();draw();
 },{capture:true});
 listen(palette,'pointerdown',e=>{if(e.target===palette)closePalette();});

 // Regions: Forma markup for every visible piece of chrome.
 const topbar=document.createElement('div');topbar.className='forma-topbar';
 const corner=document.createElement('div');corner.className='canvas-corner';
 const hints=document.createElement('div');hints.className='minimal-hints';
 const resizers=[];
 function attachResizer(panel,cls,prop,axis,sign,min,max,def){
  if(!panel)return;
  const bar=document.createElement('div');bar.className='panel-resizer '+cls;
  panel.append(bar);resizers.push(bar);
  let startPos=0,startSize=0;
  listen(bar,'pointerdown',e=>{
   if(e.button!==0)return;
   e.preventDefault();bar.setPointerCapture(e.pointerId);bar.classList.add('dragging');
   startPos=axis==='x'?e.clientX:e.clientY;
   startSize=axis==='x'?panel.getBoundingClientRect().width:panel.getBoundingClientRect().height;
  });
  listen(bar,'pointermove',e=>{
   if(!bar.hasPointerCapture(e.pointerId))return;
   const delta=((axis==='x'?e.clientX:e.clientY)-startPos)*sign;
   const next=Math.round(Math.min(max,Math.max(min,startSize+delta)));
   document.documentElement.style.setProperty(prop,next+'px');
  });
  const end=e=>{if(bar.hasPointerCapture(e.pointerId))bar.releasePointerCapture(e.pointerId);bar.classList.remove('dragging');};
  listen(bar,'pointerup',end);listen(bar,'pointercancel',end);
  listen(bar,'dblclick',()=>document.documentElement.style.setProperty(prop,def+'px'));
 }
 attachResizer($('.explorer'),'explorer-resizer','--explorer-width','x',1,160,640,260);
 attachResizer($('.bottom'),'bottom-resizer','--bottom-height','y',-1,100,520,210);
 attachResizer($('.inspector'),'inspector-resizer','--inspector-width','x',-1,200,560,262);
 const zoomTargets={out:'[data-zoom="out"]',reset:'[data-zoom="reset"]',in:'[data-zoom="in"]',fit:'[data-fit]'};
 const hintList=[['/','добавить'],['⌘K','команды'],['⌘P','файлы'],['⌘\\','код'],['L','слои']];
 function draw(){
  if(!regions||destroyed)return;
  const fit=(text,size,pad)=>regions.title.measure(text,size)+2*pad;
  const title=[...$('.project-title').childNodes].find(node=>node.nodeType===3)?.textContent.trim()||'Forma';
  regions.title.update({title,titleWidth:fit(title,13,12)});
  const modeText=run.textContent,app=appButton();
  regions.run.update({showMode:true,modeText,modeWidth:fit(modeText,13,12),canLaunch:!!app,launchText:'Запустить',launchWidth:fit('Запустить',13,14)});
  const options=[...scenario.options];
  regions.states.element.hidden=options.length<2;
  regions.states.update({tabs:options.map((option,i)=>({id:i,label:option.textContent,selected:option.selected,width:fit(option.textContent,12,10)}))});
  const zoom=$('[data-zoom="reset"]')?.textContent.trim()||'100%',problemsText=String(problemCount);
  regions.corner.update({zoomable:true,unsaved:!!saveText,unsavedText:saveText,unsavedWidth:fit(saveText,11,10),zoom,problemsText,problemsWidth:fit(problemsText,12,8)+22,bad:problemCount>0});
  regions.hints.update({hints:hintList.map(([key,text])=>({key:keyText(key),label:text,keyWidth:fit(keyText(key),10,6),labelWidth:fit(text,11,0)}))});
  if(!palette.hidden)regions.palette.update(paletteState());
  if(inspectorView?.node)regions.inspector.update(inspectorState());
 }

 // Inspector: the rows propertyFields resolved for the selection. Typing keeps a draft per row; Enter,
 // Tab or leaving the panel hands it to the property inspector, which validates and commits it.
 let inspectorView=null;
 // A committed value reaches the rows only after the host recompiles, so it is shown as `pending` until
 // the row's text moves off what it was at commit time; a focused field would otherwise keep the old text.
 const drafts=new Map(),errors=new Map(),pending=new Map();
 const fieldKey=node=>{const key=String(node?.props?.key??'');return key.slice(key.lastIndexOf('/')+1);};
 function inspectorState(){
  const view=inspectorView,linkKinds=['binding','handler'];
  const row=field=>({key:field.key,text:drafts.get(field.key)??pending.get(field.key)?.text??field.text,editable:field.editable,readonly:!field.editable,
   layered:!!field.layer&&field.layer!=='markup',error:!!(errors.get(field.key)||field.error)});
  const notice=view.notice||[...errors.values()].find(Boolean)||'';
  return {title:view.node,hasNote:!!view.note,note:view.note,canAdd:view.addable.length>0,hasNotice:!!notice,notice,
   rows:view.fields.filter(field=>!linkKinds.includes(field.kind)).map(row),
   links:view.fields.filter(field=>linkKinds.includes(field.kind)).map(field=>({...row(field),label:`${field.key} ${field.op}`}))};
 }
 function commitField(key){
  if(!drafts.has(key)||!inspector)return;
  const text=drafts.get(key);drafts.delete(key);
  const before=inspectorView?.fields?.find(field=>field.key===key)?.text;
  if(before!==text)pending.set(key,{text,before});
  const message=inspector.edit(key,text);
  if(message){pending.delete(key);errors.set(key,message);drafts.set(key,text);}else errors.delete(key);
  draw();
 }
 const commitAll=()=>{for(const key of [...drafts.keys()])commitField(key);};
 function addProperty(){
  commitAll();
  items=inspectorView.addable.map(name=>({label:name,run:()=>inspector.add(name)}));
  placeholder='Добавить свойство…';query='';index=0;palette.hidden=false;filter();draw();
  regions?.palette.focus('query');
 }
 loadVectorRuntime().then(runtime=>{
  if(destroyed)return;
  const region=(entry,className,options={})=>createShellRegion({runtime,entry,className,...options});
  regions={
   title:region('ui/Title.ui','forma-title',{onAction:()=>openPalette('menu')}),
   states:region('ui/States.ui','state-tabs',{onAction:(action,[i])=>{scenario.selectedIndex=i;scenario.dispatchEvent(new Event('change'));draw();}}),
   run:region('ui/Run.ui','forma-run',{onAction:action=>(action==='launch'?appButton():run)?.click()}),
   corner:region('ui/Corner.ui','forma-corner',{onAction:(action,[target])=>action==='zoom'?$(zoomTargets[target])?.click():toggle('problems')}),
   hints:region('ui/Hints.ui','forma-hints'),
   inspector:region('ui/Inspector.ui','forma-inspector',{
    onAction:(action,[key])=>{commitAll();if(action==='remove'){drafts.delete(key);errors.delete(key);inspector.remove(key);}else if(action==='add')addProperty();},
    onBinding:(path,value,node)=>{const key=fieldKey(node);if(!key)return;drafts.set(key,value);errors.delete(key);draw();},
    onKey:(event,node)=>{
     const key=fieldKey(node);
     if(!drafts.has(key)&&!errors.has(key))return false;
     if(event.key==='Enter'){commitField(key);return true;}
     if(event.key==='Escape'){drafts.delete(key);errors.delete(key);draw();return true;}
     if(event.key==='Tab')commitField(key);
     return false;
    }}),
   palette:region('ui/Palette.ui','forma-palette-box',{onAction:(action,[i])=>pick(i),onBinding:(path,value)=>{if(path==='query'){query=value;index=0;filter();draw();}}}),
  };
  topbar.append(regions.title.element,regions.states.element,regions.run.element);
  corner.append(regions.corner.element);hints.append(regions.hints.element);palette.append(regions.palette.element);
  $('#canvas').before(topbar);$('.preview-pane').append(corner,hints);
  body.classList.add('forma-chrome');
  if(inspector){
   const aside=$('.inspector');aside.insertBefore(regions.inspector.element,$('#inspector'));
   listen(regions.inspector.element,'focusout',e=>{if(!regions.inspector.element.contains(e.relatedTarget))commitAll();});
   // The runtime's text field consumes Enter before the key hook sees it, so the panel catches it first.
   listen(regions.inspector.element,'keydown',e=>{if(e.key==='Enter'&&!e.isComposing&&drafts.size){e.preventDefault();e.stopPropagation();commitAll();}},{capture:true});
   // A new selection or a commit redraws the rows; a draft only survives for the control it was typed for.
   let shown=null;
   const off=inspector.subscribe(view=>{
    const node=view.node?`${view.path}@${view.start}`:null;
    if(node!==shown){drafts.clear();errors.clear();pending.clear();shown=node;}
    for(const [key,{before}]of [...pending])if(view.fields?.find(field=>field.key===key)?.text!==before)pending.delete(key);
    for(const key of [...errors.keys()])if(!view.fields?.some(field=>field.key===key))errors.delete(key);
    inspectorView=view;regions.inspector.element.hidden=!view.node;draw();
   });
   scope.signal.addEventListener('abort',off);
   body.classList.add('forma-inspector');
  }
  draw();
 }).catch(error=>console.error('Studio UI:',error));
 // The DOM the regions mirror keeps changing underneath them: options, zoom text, launch button.
 observe(scenario,draw,{childList:true,subtree:true,attributes:true});
 listen(scenario,'change',draw);
 observe(header,draw,{childList:true,subtree:true,characterData:true});
 const scale=$('[data-zoom="reset"]');
 if(scale)observe(scale,draw,{childList:true,characterData:true,subtree:true});

 const typing=t=>t instanceof Element&&!!t.closest('input,textarea,select,[contenteditable="true"],.cm-editor')&&t.checkVisibility();
 listen(document,'keydown',e=>{
  const mod=e.metaKey||e.ctrlKey,key=e.key.toLowerCase();
  if(mod&&key==='k'){e.preventDefault();openPalette('commands');return;}
  if(mod&&key==='p'){e.preventDefault();openPalette('files');return;}
  if(mod&&e.key==='\\'){e.preventDefault();body.classList.add('used-shortcut');toggle('code');return;}
  if(mod&&e.key==='Enter'){e.preventDefault();(appButton()??run).click();return;}
  if(!palette.hidden||typing(e.target)||mod||e.altKey)return;
  if(e.key==='Escape'){
   if(body.classList.contains('running'))run.click();
   else if(body.classList.contains('show-layers'))toggle('layers',false);
   else if(body.classList.contains('show-problems'))toggle('problems',false);
   else if(body.classList.contains('has-selection')&&clearSelection)clearSelection();
   else return;
   e.preventDefault();return;
  }
  if(key==='l'||key==='д'){e.preventDefault();body.classList.add('used-shortcut');toggle('layers');}
  else if(e.key==='/'){e.preventDefault();openPalette('insert');}
 },{capture:true});

 return {toggle,openPalette,destroy(){destroyed=true;scope.dispose();for(const r of Object.values(regions??{}))r.destroy();for(const b of resizers)b.remove();palette.remove();topbar.remove();corner.remove();hints.remove();body.classList.remove('minimal','forma-chrome','forma-inspector',...Object.values(panels),'has-selection','running','used-shortcut');}};
}
