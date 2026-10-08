import './minimal-shell.css';
import {createEventScope} from './event-scope.js';
import {controlLabel} from './control-label.js';

// Minimal layout: only the canvas, the state tabs and the selection stay on screen.
// Every other surface keeps its existing DOM and handlers and is revealed on demand,
// so commands, tests and the MCP bridge keep driving the same elements.
export function mountMinimalShell(root,{newFile,clearSelection}={}){
 const $=selector=>root.querySelector(selector);
 const body=document.body,header=$('header'),run=$('#run');
 body.classList.add('minimal');
 const scope=createEventScope(),listen=scope.listen,observe=scope.observe;
 const panels={code:'show-code',layers:'show-layers',problems:'show-problems'};
 const toggle=(name,force)=>{body.classList.toggle(panels[name],force);if(name==='code'&&body.classList.contains('show-code'))(root.querySelector('.cm-content')??$('#code'))?.focus();};

 // Header: project menu on the title, a single run button.
 const menuButton=document.createElement('button');menuButton.className='project-menu-button';menuButton.setAttribute('aria-haspopup','menu');menuButton.textContent='▾';
 const menu=document.createElement('div');menu.className='project-menu';menu.setAttribute('role','menu');menu.hidden=true;
 $('.project-title').append(menuButton);header.append(menu);
 const collectMenu=()=>{for(const el of [...header.children])if(el!==menu&&el!==run&&!el.matches('.brand,.project-title'))menu.append(el);};
 observe(header,collectMenu,{childList:true});
 const closeMenu=()=>{menu.hidden=true;};
 listen(menuButton,'click',e=>{e.stopPropagation();menu.hidden=!menu.hidden;});
 listen(menu,'click',e=>{if(e.target.closest('button,a'))closeMenu();});
 listen(document,'click',e=>{if(!menu.contains(e.target)&&e.target!==menuButton)closeMenu();});

 const label=()=>{const interacting=$('#canvas')?.classList.contains('interacting');run.textContent=interacting?'■':'▶';run.title=interacting?'Вернуться в дизайн (Esc)':'Запустить (⌘↵)';body.classList.toggle('running',!!interacting);};
 observe($('#canvas'),label,{attributes:true,attributeFilter:['class']});
 observe(run,()=>{if(!/^[▶■]$/.test(run.textContent))label();},{childList:true,characterData:true,subtree:true});

 // State tabs mirror the scenario select.
 const scenario=$('#scenario'),tabs=document.createElement('div');tabs.className='state-tabs';tabs.setAttribute('role','tablist');tabs.setAttribute('aria-label','Состояния');
 $('#canvas').before(tabs);
 const drawTabs=()=>{
  const options=[...scenario.options];tabs.hidden=options.length<2;
  tabs.replaceChildren(...options.map(option=>{const b=document.createElement('button');b.type='button';b.setAttribute('role','tab');b.textContent=option.textContent;b.setAttribute('aria-selected',String(option.selected));b.onclick=()=>{scenario.value=option.value;scenario.dispatchEvent(new Event('change'));drawTabs();};return b;}));
 };
 observe(scenario,drawTabs,{childList:true,subtree:true,attributes:true});
 listen(scenario,'change',drawTabs);

 // Inspector only while something is selected; debugger only while running.
 const inspector=$('#inspector');
 observe(inspector,()=>body.classList.toggle('has-selection',!inspector.querySelector(':scope>:is(.empty-icon,.inspector-empty)')),{childList:true});

 // Problems badge: quiet when clean, opens the drawer when the count changes to non-zero.
 const corner=document.createElement('div');corner.className='canvas-corner';
 const problems=document.createElement('button');problems.className='problems-badge';problems.title='Проблемы';problems.onclick=()=>toggle('problems');
 const saved=document.createElement('span');saved.className='save-warning';
 const zoom=document.createElement('div');zoom.className='zoom-pill';zoom.setAttribute('role','group');zoom.setAttribute('aria-label','Масштаб');
 for(const [proxy,text,target]of [['out','−','[data-zoom="out"]'],['reset','100%','[data-zoom="reset"]'],['in','+','[data-zoom="in"]'],['fit','⤢','[data-fit]']]){
  const b=document.createElement('button');b.type='button';b.dataset.proxy=proxy;b.textContent=text;
  b.title=root.querySelector(target)?.getAttribute('aria-label')||root.querySelector(target)?.title||root.querySelector(target)?.textContent||'';
  b.onclick=()=>root.querySelector(target)?.click();zoom.append(b);
 }
 const scale=root.querySelector('[data-zoom="reset"]');
 if(scale)observe(scale,()=>{zoom.querySelector('[data-proxy="reset"]').textContent=scale.textContent;},{childList:true,characterData:true,subtree:true});
 corner.append(saved,zoom,problems);$('.preview-pane').append(corner);
 let lastCount=0;
 observe($('#problem-count'),()=>{const count=Number($('#problem-count').textContent)||0;problems.textContent=`⚠ ${count}`;problems.classList.toggle('bad',count>0);if(count>lastCount)toggle('problems',true);lastCount=count;},{childList:true,characterData:true,subtree:true});
 observe($('#saved'),()=>{const text=$('#saved').textContent;saved.textContent=/Не сохранено|повреждён/.test(text)?text:'';},{childList:true,characterData:true,subtree:true});

 // Hints on the canvas until the first shortcut is used.
 const hints=document.createElement('div');hints.className='minimal-hints';
 hints.innerHTML='<kbd>/</kbd> добавить · <kbd>⌘K</kbd> команды · <kbd>⌘P</kbd> файлы · <kbd>⌘\\</kbd> код · <kbd>L</kbd> слои';
 $('.preview-pane').append(hints);

 // Palette shared by ⌘K (commands), ⌘P (files) and / (insert).
 const palette=document.createElement('div');palette.className='palette';palette.hidden=true;
 palette.innerHTML='<div class="palette-box" role="dialog" aria-modal="true"><input aria-label="Поиск" spellcheck="false"><div class="palette-list" role="listbox"></div></div>';
 body.append(palette);
 const input=palette.querySelector('input'),list=palette.querySelector('.palette-list');
 let items=[],shown=[],index=0;
 const name=controlLabel;
 const usable=el=>!el.disabled&&!el.closest('.palette,#preview,.project-menu-button')&&name(el);
 function commands(){
  const out=[
   {label:'Показать / скрыть код',key:'⌘\\',run:()=>toggle('code')},
   {label:'Показать / скрыть слои и файлы',key:'L',run:()=>toggle('layers')},
   {label:'Показать / скрыть проблемы и журнал',run:()=>toggle('problems')},
   {label:run.title,key:'⌘↵',run:()=>run.click()},
  ];
  if(newFile)out.push({label:'Новый файл',run:newFile});
  for(const el of root.querySelectorAll('.project-menu :is(button,a),.studio-canvas-toolbar button,.preview-tools button,.canvas-tools button,.bottom-tabs button,.tabs button'))if(usable(el)&&el!==run)out.push({label:name(el),run:()=>el.click()});
  for(const select of root.querySelectorAll('.studio-canvas-toolbar select,.canvas-tools select,.pane-toolbar select'))if(select!==scenario&&!select.disabled)for(const option of select.options)out.push({label:`${select.getAttribute('aria-label')||'Выбор'}: ${option.textContent}`,run:()=>{select.value=option.value;select.dispatchEvent(new Event('change'));}});
  const seen=new Set();return out.filter(c=>!seen.has(c.label)&&seen.add(c.label));
 }
 const files=()=>[...root.querySelectorAll('#tree [data-path]')].map(el=>({label:el.dataset.path,run:()=>{el.click();toggle('code',true);}}));
 const inserts=()=>[...root.querySelectorAll('.element-tools .element-add')].filter(usable).map(el=>({label:name(el).replace(/^＋\s*/,''),run:()=>el.click()}));
 function draw(){
  const q=input.value.trim().toLowerCase();
  shown=items.filter(item=>q.split(/\s+/).every(part=>item.label.toLowerCase().includes(part))).slice(0,60);
  index=Math.min(index,Math.max(0,shown.length-1));
  list.replaceChildren(...shown.map((item,i)=>{const row=document.createElement('div');row.className='palette-item';row.setAttribute('role','option');row.setAttribute('aria-selected',String(i===index));row.innerHTML='<span></span><kbd></kbd>';row.firstChild.textContent=item.label;row.lastChild.textContent=item.key??'';row.onmousedown=e=>{e.preventDefault();pick(i);};return row;}));
  if(!shown.length){const empty=document.createElement('div');empty.className='palette-empty';empty.textContent='Ничего не найдено';list.append(empty);}
  list.children[index]?.scrollIntoView({block:'nearest'});
 }
 function openPalette(kind){
  items=kind==='files'?files():kind==='insert'?inserts():commands();
  if(kind==='insert'&&!items.length){items=commands().filter(c=>/добав|встав|insert/i.test(c.label));}
  input.placeholder=kind==='files'?'Открыть файл…':kind==='insert'?'Добавить элемент…':'Команда…';
  input.value='';index=0;palette.hidden=false;body.classList.add('used-shortcut');draw();input.focus();
 }
 const closePalette=()=>{palette.hidden=true;};
 function pick(i){const item=shown[i];closePalette();item?.run();}
 listen(input,'input',()=>{index=0;draw();});
 listen(input,'keydown',e=>{
  if(e.key==='ArrowDown'){e.preventDefault();index=Math.min(shown.length-1,index+1);draw();}
  else if(e.key==='ArrowUp'){e.preventDefault();index=Math.max(0,index-1);draw();}
  else if(e.key==='Enter'){e.preventDefault();pick(index);}
  else if(e.key==='Escape'){e.preventDefault();closePalette();}
 });
 listen(palette,'mousedown',e=>{if(e.target===palette)closePalette();});

 const typing=t=>t instanceof Element&&!!t.closest('input,textarea,select,[contenteditable="true"],.cm-editor')&&t.checkVisibility();
 listen(document,'keydown',e=>{
  const mod=e.metaKey||e.ctrlKey,key=e.key.toLowerCase();
  if(mod&&key==='k'){e.preventDefault();openPalette('commands');return;}
  if(mod&&key==='p'){e.preventDefault();openPalette('files');return;}
  if(mod&&e.key==='\\'){e.preventDefault();body.classList.add('used-shortcut');toggle('code');return;}
  if(mod&&e.key==='Enter'){e.preventDefault();run.click();return;}
  if(!palette.hidden||typing(e.target)||mod||e.altKey)return;
  if(e.key==='Escape'){
   if(!menu.hidden)closeMenu();
   else if(body.classList.contains('running'))run.click();
   else if(body.classList.contains('show-layers'))toggle('layers',false);
   else if(body.classList.contains('show-problems'))toggle('problems',false);
   else if(body.classList.contains('has-selection')&&clearSelection)clearSelection();
   else return;
   e.preventDefault();return;
  }
  if(key==='l'||key==='д'){e.preventDefault();body.classList.add('used-shortcut');toggle('layers');}
  else if(e.key==='/'){e.preventDefault();openPalette('insert');}
 },{capture:true});

 return {toggle,openPalette,destroy(){scope.dispose();palette.remove();tabs.remove();corner.remove();hints.remove();menu.remove();menuButton.remove();body.classList.remove('minimal',...Object.values(panels),'has-selection','running','used-shortcut');}};
}
