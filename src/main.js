import {readStorage,writeStorage,validateProject,restoreProject,writeProject} from './browser-storage.js';
import {studioControlsProject} from './studio-controls-builtin.js';
import {sampleProject} from './sample-project.js';
import {mountStudioShell} from './studio-shell.js';
import {mountMinimalShell} from './minimal-shell.js';
import {createStudioEvents,studioEventTypes} from './studio-events.js';
const studioEvents=createStudioEvents();
const isStudioControls=new URLSearchParams(location.search).get('project')==='studio-controls';
const storageKey=name=>isStudioControls?name+':studio-controls-guide':name;
import {designPreset,collectionScenario} from './design-presets.js';
let designPresetName='original',nativeSource=null,nativeBuffer='';
import {createLayoutInspector} from './layout-inspector.js';
let layoutInspector,lastVisuals=[],sourceDiagnostic=null;
import {createCanvasTools} from './canvas-tools.js';
import {createElementTools} from './element-tools.js';
import './canvas-tools.css';
let canvasTools,elementTools;
import './style.css';
import './selection.css';
import './vector-artboard.css';
import './control-tree.css';
import './property-inspector.css';
import './states-panel.css';
import {createControlTree} from './control-tree.js';
import {createStatesPanel} from './states-panel.js';
import {designStatesSummary,findEntry,stateCreateEdit,stateDeleteEdit,statePropertyEdit,stateRenameEdit} from './design-states.js';
import {copyElement,insertElement,moveAmongSiblings,moveIntoContainer,removeElement} from './element-edit.js';
import {restoreSelection,selectionIdentities} from './selection-restore.js';
import {mountEditor} from './editor.js';
import {createPropertyInspector} from './property-inspector.js';
import {createBulkEditor} from './bulk-edit.js';
import {designReferencesInFiles} from './design-data.js';
import {insertableControls} from './control-catalog.js';
import {createSpacingOverlay} from './spacing-overlay.js';
import {gridProperties,gridStyles} from './grid.js';
import {takesCoordinates,coordinateShift} from './positioning.js';
import {sizeProperties,sizeValue} from './sizing.js';
import {nativeSnapshot} from './native-snapshot.js';
import {loadVectorRuntime,createVectorPreview} from './vector-preview.js';
import {createComponentCompiler,sceneControlNodes} from './components.js';
import {expandStructure,evaluate} from './component-semantics.js';
import {writePreviewBinding} from './preview-state.js';
let previewRefreshPending=false,lastPreviewControls=[];
function schedulePreviewRefresh(){if(previewRefreshPending)return;previewRefreshPending=true;queueMicrotask(()=>{previewRefreshPending=false;try{render();error='';}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;}output();});}
import {createCacheBudget} from './cache-budget.js';
const studioCache=createCacheBudget();
const compileComponents=createComponentCompiler({cache:studioCache});
import {setDesignData,designReferences} from './design-data.js';
let codeEditor,propertyInspector;
let spacingOverlay;
let vectorPreview;
let controlTree,bulkEditor,selectedPath=null,inspectorNote='';
let statesPanel;
// The design files the entry resolves, in reference order and relative to the entry's folder: the
// states panel authors the first of them, while a state row switches by name across all of them.
let designFiles=[];
// The active tree is live UI state, independent of eviction of inactive files.
let activeTreeDocument=null;
let renderer=isStudioControls||readStorage('localStorage',storageKey('forma-renderer'))==='vector'?'vector':'html';
import {parse,resolve,validateDesign,designStatePatch} from './language.js';

const initialProject=isStudioControls?studioControlsProject:sampleProject;
let files=restoreProject(readStorage('localStorage',storageKey('forma-project')),initialProject);
files['Cargo.toml']??=`[package]\nname = "forma-preview-app"\nversion = "0.1.0"\nedition = "2021"\n`;
files['src/main.rs']??=`mod actions;\n\nfn main() {\n    let mut state = actions::SearchState {\n        query: String::from("Архитектура"),\n        loading: false,\n        status: String::from("Готов"),\n    };\n    println!("Rust-приложение запущено");\n    println!("Запрос: {}", state.query);\n    actions::search(&mut state);\n    println!("После actions::search: loading={}, status={}", state.loading, state.status);\n}\n`;
let active=Object.keys(files)[0],entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui')), scenario=0,state={},compiled=null,selected=null,mode=readStorage('sessionStorage',storageKey('forma-canvas-mode'))==='interact'?'interact':'design',pending=null,breakOn=false,logs=[],tab='problems',error='',saveTimer,compileTimer;
try{const view=JSON.parse(readStorage('localStorage',storageKey('forma-view')));if(view?.entry in files)entry=view.entry;if(view?.active in files)active=view.active;}catch{}
function persistView(){try{writeStorage('localStorage',storageKey('forma-view'),JSON.stringify({active,entry}));}catch{}}
const $=id=>document.getElementById(id);
document.querySelector('#app').innerHTML=`
<header><div class="brand"><span class="brandmark">F</span> forma <small>STUDIO</small></div><div class="project-title">${isStudioControls?'Studio Controls':'knowledge-workspace'} <span> / локальный проект</span></div><a class="controls-link" href="${isStudioControls?'/':'/?project=studio-controls'}">${isStudioControls?'Мой проект':'Контролы Studio'}</a><a class="controls-link" href="/vector-ui/examples/controls.html">Контролы · Reveal</a><button id="import">Открыть</button><button id="export">Экспорт</button><button id="run" class="accent">${mode==='design'?'▶ Взаимодействие':'⌖ Выбор элемента'}</button></header>
<div class="workspace"><aside class="explorer"><div class="section-title">ПРОЕКТ <button id="new" title="Создать файл">+</button></div><div id="tree"></div><div class="explorer-note"><span class="dot"></span> Локальное сохранение<br><small>Проект хранится в этом браузере.<br>Экспортируйте для резервной копии.</small></div></aside>
<main><div class="tabs"><span class="file-icon">◇</span><span id="filename"></span><span id="saved">Сохранено</span><button id="entry" title="Показать этот UI в предпросмотре">Показать UI</button></div><div class="editor-preview"><section class="code-pane"><div class="pane-toolbar"><span>РАЗМЕТКА / КОД</span><span id="language">Forma UI</span></div><div class="code-wrap"><div id="lines"></div><textarea id="code" spellcheck="false" aria-label="Редактор исходного кода"></textarea></div></section><section class="preview-pane"><div class="pane-toolbar"><span>LIVE PREVIEW <i class="dot"></i></span><select id="scenario" aria-label="Состояние дизайна"></select></div><div class="preview-tools"><button id="desktop" class="chosen">Desktop</button><button id="mobile">Mobile</button><button id="theme">◐ Тема</button><span id="preview-name"></span></div><div id="canvas"><div id="preview"></div></div><div class="preview-caption" id="caption">Выберите элемент, чтобы найти его в разметке</div></section></div>
<section class="bottom"><div class="bottom-tabs"><button data-tab="problems" class="chosen">Диагностика <span id="problem-count">0</span></button><button data-tab="events">События <span id="event-count">0</span></button><button data-tab="state">Состояние</button><div class="debug-controls"><label><input type="checkbox" id="break"> Break on event</label><button id="continue" disabled>▶ Продолжить</button><button id="reset">↺ Сброс</button></div></div><div id="output"></div></section></main>
<aside class="inspector"><div class="section-title">ИНСПЕКТОР</div><div id="inspector"><div class="empty-icon">⌖</div><p>Выберите компонент</p><small>Свойства и привязки появятся здесь</small></div><div class="debug-info"><span>UI DEBUGGER</span><p id="debug-status">Готов</p><small>События и состояние дизайн-runtime.<br>Rust / DAP не подключён.</small></div></aside></div>
<footer><span class="dot"></span><span id="status">Готов</span><span class="footer-right">Forma UI · UTF-8 <span id="position">Ln 1, Col 1</span></span></footer><input type="file" id="upload" accept=".json" hidden>`;
const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
// Recovery data is downloadable without parsing or executing its contents.
{
 const key=storageKey('forma-project');
 const current=readStorage('localStorage',key);
 let damaged=null;
 if(current!==null)try{validateProject(JSON.parse(current));}catch{damaged=current;}
 const recovery=damaged??readStorage('localStorage',key+':recovery');
 if(recovery!==null){
  const button=document.createElement('button');button.textContent='Скачать данные восстановления';
  button.title='Исходная запись проекта, которую не удалось прочитать';
  button.onclick=()=>{const url=URL.createObjectURL(new Blob([recovery],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='forma-project-recovery.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};
  $('export').after(button);
 }
 if(damaged!==null)saveStatus('damaged','Сохранённый проект повреждён: открыта исходная версия');
}
function resetProjectEditing(){clearTimeout(saveTimer);clearTimeout(compileTimer);saveTimer=compileTimer=undefined;setDesignData({});codeEditor?.resetProject();}
function saveStatus(status,text){$('saved').textContent=text;studioEvents.emit(studioEventTypes.save,{status,text});}
function persist(){try{if(!writeProject(storageKey('forma-project'),files))throw Error('Storage unavailable');saveStatus('saved','Сохранено');}catch{saveStatus('unsaved','Не сохранено: экспортируйте проект');}}
function tree(){const buttons=[...$('tree').querySelectorAll('[data-path]')],paths=Object.keys(files),existing=new Set(buttons.map(b=>b.dataset.path));
 if(buttons.length===paths.length&&paths.every(path=>existing.has(path))){for(const b of buttons)b.classList.toggle('active',b.dataset.path===active);return;}
 const groups={};for(const path of Object.keys(files)){const folder=path.includes('/')?path.slice(0,path.lastIndexOf('/')):'Проект';(groups[folder]??=[]).push(path);} $('tree').innerHTML=Object.entries(groups).map(([folder,paths])=>`<div class="folder">⌄ &nbsp; ${esc(folder)}</div>${paths.map(p=>`<button class="file ${p===active?'active':''}" data-path="${esc(p)}"><span class="${p.endsWith('.rs')?'rust':'ui'}">${p.endsWith('.rs')?'R':'◇'}</span>${esc(p.split('/').at(-1))}</button>`).join('')}`).join('');document.querySelectorAll('[data-path]').forEach(b=>b.onclick=()=>open(b.dataset.path));}
function open(path){active=path;persistView();codeEditor?.setLanguage(path);$('filename').textContent=path.split('/').at(-1);$('code').value=files[path];$('language').textContent=path.endsWith('.rs')?'Rust · редактирование':path.endsWith('.ui')?'Forma UI':'Текст';$('entry').disabled=!path.endsWith('.ui')||path.endsWith('.design.ui');lines();tree();refreshControlTree();refreshStatesPanel();}
function lines(){
  if(codeEditor){const {line,column}=codeEditor.position();$('position').textContent=`Ln ${line}, Col ${column}`;}
  else {const text=$('code').value;$('lines').textContent=Array.from({length:text.split('\n').length},(_,i)=>i+1).join('\n');const before=text.slice(0,$('code').selectionStart).split('\n');$('position').textContent=`Ln ${before.length}, Col ${before.at(-1).length+1}`;}
  highlightSource();
}
function highlightSource(){
  spacingOverlay?.update();canvasTools?.update();elementTools?.update();
  if(codeEditor){codeEditor.highlight(selected&&active===selectedPath?selected:null);return;}
  const code=$('code');let layer=$('source-highlight');
  if(!layer){layer=document.createElement('div');layer.id='source-highlight';layer.setAttribute('aria-hidden','true');code.parentElement.append(layer);}
  layer.hidden=!selected||active!==selectedPath;
  if(layer.hidden)return;
  const first=code.value.slice(0,selected.start).split('\n').length-1;
  const last=code.value.slice(0,selected.end).split('\n').length-1;
  const lineHeight=parseFloat(getComputedStyle(code).lineHeight);
  const start=first*lineHeight-code.scrollTop;
  const end=(last+1)*lineHeight-code.scrollTop;
  const top=Math.max(0,start),bottom=Math.min(code.clientHeight,end);
  layer.hidden=bottom<=top;
  layer.style.top=(code.offsetTop+top)+'px';
  layer.style.height=Math.max(0,bottom-top)+'px';
  layer.dataset.lines=`${first+1}-${last+1}`;
}
function log(text){logs.push({time:new Date().toLocaleTimeString(),text});if(logs.length>200)logs.shift();output();}
function output(){$('problem-count').textContent=error?1:0;studioEvents.emit(studioEventTypes.problems,{count:error?1:0});$('event-count').textContent=logs.length;$('output').innerHTML=tab==='state'?`<pre>${esc(JSON.stringify(state,null,2))}</pre>`:tab==='events'?logs.map(l=>`<div class="log"><time>${l.time}</time>${esc(l.text)}</div>`).join('')||'<p class="muted">События появятся при взаимодействии с предпросмотром.</p>':error?`<p class="error">● ${esc(error)}</p>`:'<p class="ok">✓ Разметка проверена. Ошибок нет.</p>';diagnosticLinks();}
function diagnosticLinks(){
 if(tab!=='problems'||sourceDiagnostic?.message!==error)return;
 for(const [label,source]of [['К ошибке',sourceDiagnostic.source],['К определению узла',sourceDiagnostic.related]]){
  if(!source||typeof files[source.file]!=='string')continue;
  const button=document.createElement('button'),line=files[source.file].slice(0,source.from).split('\n').length;
  button.textContent=`${label} · ${source.file}:${line}`;
  button.onclick=()=>{open(source.file);$('code').setSelectionRange(source.from,source.to);$('code').focus();};$('output').append(button);
 }
}
function compile(reset=false){persistView();try{if(!entry||!files[entry])throw Error('Откройте .ui компонент и нажмите «Показать UI»');const next=parse(files[entry]);const states=[];designFiles=[];for(const ref of next.designs){const base=entry.includes('/')?entry.slice(0,entry.lastIndexOf('/')+1):'';const path=base+ref.replace(/^\.\//,'');if(!(path in files))throw Error(`Дизайн-файл не найден: ${path}`);designFiles.push(path);const design=parse(files[path]);if(Object.keys(design.overrides).length||design.states.length){validateDesign(next,design);for(const key of Object.keys(design.overrides)){if(Object.hasOwn(next.overrides,key))throw Error(`Повторный дизайн-key ${key}`);Object.defineProperty(next.overrides,key,{value:design.overrides[key],enumerable:true});}for(const state of design.states){if(states.some(other=>other.name===state.name))throw Error(`Повторное состояние ${state.name}`);states.push({...state,file:path});}}}compiled=next;compiled.states=states;scenario=Math.min(scenario,Math.max(0,states.length-1));if(reset)state=structuredClone(states[scenario]?.state??{});$('scenario').innerHTML=(states.length?'<option value="-1">Базовые значения</option>':'')+states.map((s,i)=>`<option value="${i}">${esc(s.name)}</option>`).join('')||'<option>Без состояний</option>';$('scenario').value=scenario;$('preview-name').textContent=next.name;error='';render();$('status').textContent='Live preview обновлён';}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;$('status').textContent='Ошибка · сохранён последний предпросмотр';}refreshControlTree();refreshStatesPanel();output();}
function render(designMode=mode==='design'){
if(!compiled)return;
if(renderer==='vector'){
  if(!vectorPreview)throw Error('Загрузка векторного Rust/WASM…');
  if(!files['components/Button.ui'])throw Error('Добавьте components/Button.ui с примитивами кнопки');
  if(compiled.designs.length)throw Error('Векторный срез пока не поддерживает design-файлы и ViewModel');
  const linked=compileComponents(files,entry,state,{measureText:vectorPreview.measureText,instanceProps:designMode?n=>designPreset(designPresetName,n):undefined,transformScene:designMode?scene=>collectionScenario(designPresetName,scene,files):undefined});
  lastVisuals=linked.visualNodes??[];lastPreviewControls=linked.previewControls??[];
  const ok=vectorPreview.render({container:$('preview'),...linked,designMode,nodes:designMode&&designPresetName!=='original'?linked.previewNodes:compiled.nodes,selectedStart:selectedPath===entry?selected?.start:null});
  if(!ok)throw Error(error||'Ошибка векторного компонента');
  $('preview').style.width=vectorPreview.layoutSnapshot().width+'px';$('preview').style.minHeight='0';$('preview').style.overflow='visible';
  $('caption').textContent=designMode?'Rust/WASM · компонент из примитивов · щёлкните для выбора':'Rust/WASM · hover / pressed + transitions · события в журнале';
  if(designMode&&designPresetName!=='original')$('caption').textContent='Дизайнерский сценарий · исходник не изменён · редактирование частей отключено';
  $('canvas').classList.toggle('interacting',!designMode);canvasTools?.update();layoutInspector?.update();elementTools?.update();return;
}
const preview=$('preview');preview.classList.remove('vector-artboard');preview.style.backgroundColor='';preview.style.borderRadius='';preview.style.minHeight='';const cssMap={'padding':'padding','margin':'margin','gap':'gap','overflow':'overflow','background':'background','color':'color','width':'width','height':'height','radius':'borderRadius','font.size':'fontSize','opacity':'opacity'};
// Controls whose drawing the scene moves out of its flow slot, in document order so a moved
// container is measured after its own shift landed.
const freeNodes=[];
// One design file holds several states; the switcher index picks which patch lies over the base.
const activeDesign=designMode?designStatePatch(compiled,compiled.states[scenario]?.name):null;
for(const kind of ['padding','margin'])for(const side of ['top','right','bottom','left'])cssMap[`${kind}.${side}`]=kind+side[0].toUpperCase()+side.slice(1);
function make(source,parent=null,parentEl=null){const override=activeDesign&&Object.hasOwn(activeDesign,source.props.key)?activeDesign[source.props.key]:null;const n={...source,props:{...source.props,...override}};const types={Frame:'div',Column:'div',Row:'div',Grid:'div',Stack:'div',Scroll:'div',Text:'div',TextInput:'input',TextField:'input',Checkbox:'input',Slider:'input',Button:'button',Panel:'div'};if(!types[n.type])throw Error(`Компонент ${n.type} пока не поддерживается`);const el=document.createElement(types[n.type]);el.dataset.start=n.start;el.className='ui-node '+n.type.toLowerCase();if(n.type==='Checkbox')el.type='checkbox';if(n.type==='Slider')el.type='range';if(n.type==='Stack'){el.style.display='grid';}if(n.type==='Scroll')el.style.overflow='auto';if(n.type==='Frame'||n.type==='Column'||n.type==='Row'){el.style.display='flex';el.style.flexDirection=n.type==='Row'?'row':'column';}for(const [k,v]of Object.entries(n.props)){const x=resolve(v,state);if(k==='key'){el.dataset.key=x;continue;}if(k==='clip'){if(!['Frame','Row','Column','Grid','Stack'].includes(n.type)||typeof x!=='boolean')throw Error('Frame.clip ожидает true или false');if('overflow' in n.props)throw Error('Используйте clip или overflow, не оба');el.style.overflow=x?'clip':'visible';continue;}if(gridProperties.has(k)||k==='x'||k==='y')continue;else if(sizeProperties.has(k)){if(typeof x==='string'&&/^(?:\d+(?:\.\d+)?)?\*$/.test(x)&&((parent?.type==='Row'&&k==='width')||(['Column','Frame'].includes(parent?.type)&&k==='height'))){el.style.flex=`${x==='*'?1:Number(x.slice(0,-1))} 1 0px`;el.style[k==='width'?'minWidth':'minHeight']='0';}else el.style[k]=sizeValue(x);}else if(k==='text')el.textContent=x??'';else if(k==='value')el.value=x??'';else if(k==='placeholder')el.placeholder=x;else if(k==='disabled')el.disabled=!!x;else if(k==='checked'||k==='selected')el.checked=!!x;else if(k==='style')el.classList.add(String(x));else if(cssMap[k])el.style[cssMap[k]]=typeof x==='number'&&k!=='opacity'?x+'px':Array.isArray(v)?v.map(a=>typeof a==='number'?a+'px':resolve(a,state)).join(' '):x;else throw Error(`Свойство ${k} пока не поддерживается`);}
for(const [key,path]of Object.entries(n.bindings)){if(!['value','checked','selected'].includes(key))throw Error(`Неподдерживаемая привязка ${key} <-> ${path}`);const property=key==='value'?'value':'checked';el[property]=n.props[key]??(property==='value'?'':false);el.addEventListener('input',()=>{const value=property==='checked'?el.checked:n.type==='Slider'?Number(el.value):el.value;if(writePreviewBinding(state,n,path,value,{createMissing:true})){log(`${path} = ${JSON.stringify(value)}`);if(n.events.changed)dispatch(n.events.changed,n,'changed');schedulePreviewRefresh();}});}
el.addEventListener('click',e=>{e.stopPropagation();if(mode==='design'){select(source);return;}if(n.events.clicked)dispatch(n.events.clicked,n);});Object.assign(el.style,gridStyles(n,state,parent));
if(takesCoordinates(n,parent))freeNodes.push([n,parent,parentEl,el]);
for(const child of n.children)el.append(make(child,n,el));
return el;}
const focused=document.activeElement;const focusIdentity=preview.contains(focused)?{key:focused.dataset.key,start:focused.dataset.start,selectionStart:focused.selectionStart,selectionEnd:focused.selectionEnd}:null;const fragment=document.createDocumentFragment();const expanded=expandStructure(compiled.nodes,compiled.defaults,state,{enums:compiled.enums,allowMissingState:true});for(const n of expanded)fragment.append(make(n));preview.replaceChildren(fragment);
// A free control keeps its flow slot, so its offset is the distance between where the flow put
// it and where the scene puts it — measurable only once the nodes are in the document.
for(const [node,parent,parentEl,el]of freeNodes){
  const shift=coordinateShift(node,parent,state,parentEl.getBoundingClientRect(),el.getBoundingClientRect());
  if(shift){el.style.position='relative';for(const [css,px]of Object.entries(shift))el.style[css]=px+'px';}
}
if(focusIdentity&&mode==='interact'){const restored=Array.from(preview.querySelectorAll('[data-start]')).find(el=>focusIdentity.key?el.dataset.key===focusIdentity.key:el.dataset.start===focusIdentity.start);if(restored){restored.focus({preventScroll:true});if(typeof focusIdentity.selectionStart==='number'&&restored.setSelectionRange)restored.setSelectionRange(focusIdentity.selectionStart,focusIdentity.selectionEnd);}}$('canvas').classList.toggle('interacting',mode!=='design');spacingOverlay?.update();}
// The panel is rebuilt from one place so every change of selection or of the preview preset
// recomputes `editable`: rows that write markup must not survive a switch to a design scenario.
function renderInspector(){
  studioEvents.emit(studioEventTypes.selection,{selected:!!(selected&&selectedPath&&Object.hasOwn(files,selectedPath))});
  // With nothing selected the panels have to say so rather than keep the rows of the control that is
  // gone: those rows would write into markup the file no longer holds.
  if(!selected||!selectedPath||!Object.hasOwn(files,selectedPath)){propertyInspector?.render({node:null,editable:false});bulkEditor?.render({starts:[],editable:false});return;}
  propertyInspector?.render({node:selected,path:selectedPath,source:files[selectedPath],editable:designPresetName==='original',note:inspectorNote,state:designStateFor(selected)});
  // The panel above draws the one control the designer last clicked; this section draws what the whole
  // selection has in common. Only the canvas keeps a batch, so the row offsets come from it alone.
  bulkEditor?.render({path:selectedPath,source:files[selectedPath],starts:selectedPath===entry?elementTools?.selection()??[]:[],primary:selected.start,editable:designPresetName==='original'});
}
// The design layer the preview is showing for this control: the file that carries the state, and the
// two entries its key is patched by. Without a state on screen, or for a control no design file
// addresses, there is nothing extra to say about the rows, and the panel stays the markup editor.
function designStateFor(node){
  const state=scenario<0?null:compiled?.states?.[scenario];
  const key=selectedPath===entry&&designPresetName==='original'?node?.props?.key:null;
  if(!state||typeof key!=='string')return null;
  const source=files[state.file];
  if(typeof source!=='string')return null;
  try{
    const overridden=findEntry(source,state.name,key),base=findEntry(source,null,key);
    return overridden||base?{name:state.name,path:state.file,source,entry:overridden,base}:null;
  }catch{/* a file that stopped compiling keeps the rows it had before */return null;}
}
function select(n){selected=n;selectedPath=entry;vectorPreview?.select(n.start);canvasTools?.update();elementTools?.update();layoutInspector?.update();if(active!==entry)open(entry);$('code').setSelectionRange(n.start,n.start);const line=files[entry].slice(0,n.start).split('\n').length;$('code').scrollTop=Math.max(0,(line-4)*23);$('lines').scrollTop=$('code').scrollTop;document.querySelectorAll('.ui-node').forEach(el=>el.classList.toggle('selected',Number(el.dataset.start)===n.start));inspectorNote='';renderInspector();lines();controlTree?.select(n.start,entry);}
// The mirror of `select` for the moment there is nothing to select: every surface that was drawing the
// control lets it go, so no row survives that could write into markup the file has dropped.
function clearSelection(){selected=null;selectedPath=null;vectorPreview?.select(null);canvasTools?.update();elementTools?.setSelection([]);layoutInspector?.update();document.querySelectorAll('.ui-node.selected').forEach(el=>el.classList.remove('selected'));inspectorNote='';renderInspector();lines();controlTree?.select(null,null);}
function refreshControlTree(){
  if(!controlTree)return;
  const path=controlTree.scope==='designer'?entry:active;
  let doc=null,message='',stale=false;
  if(!path?.endsWith('.ui'))message='Откройте файл .ui';
  else try{const source=files[path],previous=studioCache.get('tree:'+path)??(activeTreeDocument?.path===path?activeTreeDocument:null);doc=previous?.source===source?previous.document:parse(source);if(previous?.document!==doc)studioCache.set('tree:'+path,{source,document:doc});activeTreeDocument={path,source,document:doc};if(controlTree.scope==='designer'&&error)message=error;}
  catch(e){doc=(activeTreeDocument?.path===path?activeTreeDocument.document:studioCache.get('tree:'+path)?.document)??null;message=e.message;stale=!!doc;}
  controlTree.update({document:doc,path,error:message,stale,selectedStart:selected?.start,selectedPath,files});
}
// The panel authors one design file at a time: the file being edited when it is a design file,
// otherwise the first file the entry references. Rows switch the preview by state name, so a
// component with two design files never points a row at the wrong switcher index.
function designFilePath(){
  if(active.endsWith('.design.ui'))return active;
  return designFiles[0]??null;
}
function refreshStatesPanel(){
  if(!statesPanel)return;
  const path=designFilePath();
  let states=[],baseCount=0,message='',editable=designPresetName==='original';
  if(path){
    const source=path===active?$('code').value:files[path];
    try{
      const doc=parse(source);
      if(!doc.designBody)throw Error('Файл не является дизайн-файлом');
      states=doc.states.map(state=>({name:state.name,count:state.nodes.length}));
      baseCount=doc.entries.length;
    }catch(e){message=e.message;editable=false;}
  }
  statesPanel.update({path,states,baseCount,editable,error:message,active:scenario<0?null:compiled?.states[scenario]?.name});
}
// The live text of the file the states panel authors, read the way the panel's own rows read it:
// from the editor when that file is open, so a design edit never computes against older text.
function designStateFile(){
  const path=designFilePath();
  if(!path)throw Error('Дизайн-файл не выбран');
  return {path,source:path===active?$('code').value:files[path]};
}
// A panel action splices the design file its rows came from, so the edit is computed against that
// file's live text and passes the same stale-snapshot guard an inspector edit passes. An MCP write
// additionally names the text it read, because the agent's view and the editor can part ways
// between its read and its call — the live text cannot, which is what commitSource checks.
function designStateEdit(build,expectedContent){
  const {path,source}=designStateFile();
  if(expectedContent!==undefined&&expectedContent!==source)throw Error('Дизайн-файл изменился: обновите его через design_states_read');
  const edit=build(source);
  commitSource({file:path,source,...edit});
  // The editor publishes the spliced text through its input event; compiling afterwards redraws
  // the panel from the file the edit actually produced.
  queueMicrotask(()=>{clearTimeout(compileTimer);compile();});
  return {file:path,content:source.slice(0,edit.from)+edit.insert+source.slice(edit.to)};
}
function selectTreeControl(item){
  if(item.path===entry){
    if(mode!=='design')$('run').click();
    select(item.node);return;
  }
  // Template source nodes are not runtime instances: reveal their source, but
  // do not pretend their source offsets are bounds in the entry's preview.
  selected=item.node;selectedPath=item.path;vectorPreview?.select(null);
  document.querySelectorAll('.ui-node.selected').forEach(el=>el.classList.remove('selected'));
  if(active!==item.path)open(item.path);
  $('code').setSelectionRange(item.node.start,item.node.start);
  inspectorNote='Узел шаблона. Геометрию конкретного экземпляра здесь выбрать нельзя.';renderInspector();
  lines();controlTree.select(item.node.start,item.path);
}
function showNativeInspection(data){
 let panel=$('native-inspection');if(!panel){panel=document.createElement('details');panel.id='native-inspection';panel.className='layout-inspector';document.querySelector('.inspector').append(panel);}
 for(const other of document.querySelectorAll('.inspector>.layout-inspector'))if(other!==panel)other.open=false;
 panel.open=true;panel.replaceChildren();const title=document.createElement('summary');title.textContent='Нативное окно · живое дерево · F9: выбор';panel.append(title);
 const current=nativeSource&&files[nativeSource.entry]===nativeSource.source;
 const roots=nativeSource?.nodes?.[0]?.children??[],controls=sceneControlNodes(roots);
 const status=document.createElement('p');status.textContent=current?'Данные из работающего Rust runtime. F9 и щелчок в окне выбирают элемент.':'Исходник изменился или окно запущено раньше этой сессии: переход к коду отключён.';panel.append(status);
 for(const node of data.nodes??[]){
  const row=document.createElement('button'),control=data.controls?.find(c=>c.index===node.control),source=controls[node.control];
  row.style.display='block';row.style.marginLeft=(node.parent===null?0:node.control===null?12:24)+'px';
  row.textContent=source?source.type+' '+(source.props.key??''):node.kind;
  if(control)row.textContent+=' · '+control.bounds.map(v=>Math.round(v)).join(', ')+(control.disabled?' · disabled':'');
  row.setAttribute('aria-pressed',String(node.control===data.selected));
  row.disabled=!current||!source;row.onclick=()=>selectTreeControl({path:nativeSource.entry,node:source});panel.append(row);
 }
 const source=controls[data.selected];if(current&&source)selectTreeControl({path:nativeSource.entry,node:source});
}
function dispatch(action,n,event='clicked'){if(pending){log('Событие пропущено: debugger приостановлен');return;}const args=(n.eventArgExpressions?.[event]??n.eventArgs?.[event]??[]).map(value=>evaluate(value,compiled.defaults,state,[],false,n.environment));log(`Событие ${n.type}.${event} → ${action}(${args.map(value=>JSON.stringify(value)).join(', ')})`);if(breakOn){pending={action,n};$('continue').disabled=false;$('debug-status').textContent='Пауза перед '+action;tab='state';syncTabs();output();return;}execute(action);}
function execute(action){if(action==='actions.search'){state.status=`Дизайн-обработчик: запрос «${state.query||'пусто'}»`;log('Выполнен дизайн-обработчик search (без Rust/backend)');}else log(`Обработчик ${action} не подключён`);render();output();}
function syncTabs(){document.querySelectorAll('[data-tab]').forEach(b=>b.classList.toggle('chosen',b.dataset.tab===tab));}
$('code').oninput=()=>{files[active]=$('code').value;lines();saveStatus('changed','Изменено');clearTimeout(saveTimer);saveTimer=setTimeout(persist,350);clearTimeout(compileTimer);compileTimer=setTimeout(()=>compile(active.endsWith('.design.ui')),250);};$('code').onscroll=()=>{$('lines').scrollTop=$('code').scrollTop;};$('code').onclick=lines;$('code').onkeyup=lines;
$('code').onkeydown=e=>{if(e.key==='Tab'){e.preventDefault();const el=e.target;el.setRangeText('    ',el.selectionStart,el.selectionEnd,'end');el.dispatchEvent(new Event('input'));}if((e.metaKey||e.ctrlKey)&&e.key==='s'){e.preventDefault();persist();}};
$('scenario').onchange=e=>{scenario=Number(e.target.value);compile(true);};$('entry').onclick=()=>{if(renderer==='vector'&&active==='components/Button.ui'){compile();return;}entry=active;scenario=0;compile(true);};
$('run').onclick=()=>{mode=mode==='design'?'interact':'design';writeStorage('sessionStorage',storageKey('forma-canvas-mode'),mode);studioEvents.emit(studioEventTypes.mode,{mode});$('run').textContent=mode==='design'?'▶ Взаимодействие':'⌖ Выбор элемента';$('caption').textContent=mode==='design'?'Выберите элемент, чтобы найти его в разметке':'События выполняются в дизайн-runtime · Rust не подключён';render();};
$('desktop').onclick=()=>{$('preview').style.width='520px';$('desktop').classList.add('chosen');$('mobile').classList.remove('chosen');};$('mobile').onclick=()=>{$('preview').style.width='320px';$('mobile').classList.add('chosen');$('desktop').classList.remove('chosen');};$('theme').onclick=()=>$('preview').classList.toggle('light');
$('break').onchange=e=>breakOn=e.target.checked;$('continue').onclick=()=>{if(pending){const p=pending;pending=null;$('continue').disabled=true;$('debug-status').textContent='Готов';execute(p.action);}};$('reset').onclick=()=>{pending=null;$('continue').disabled=true;$('debug-status').textContent='Готов';compile(true);log('Состояние предпросмотра восстановлено');};
document.querySelectorAll('[data-tab]').forEach(b=>b.onclick=()=>{tab=b.dataset.tab;syncTabs();output();});
$('new').onclick=()=>{const path=prompt('Путь нового файла','ui/NewWindow.ui');if(!path)return;try{validateProject({[path]:''});}catch(error){alert(error.message);return;}if(path in files){alert('Файл уже существует');return;}files[path]=path.endsWith('.ui')?'component NewWindow {\n    Frame {\n        Text {\n            text: \'Новое окно\';\n        }\n    }\n}\n':'';open(path);persist();};
$('export').onclick=()=>{const url=URL.createObjectURL(new Blob([JSON.stringify(files,null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='forma-project.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};$('import').onclick=()=>$('upload').click();$('upload').onchange=async e=>{try{const f=e.target.files[0];if(!f)return;const data=validateProject(JSON.parse(await f.text()));if(!confirm('Заменить текущий проект? При необходимости сначала экспортируйте его.'))return;resetProjectEditing();files=data;active=Object.keys(files)[0];entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui'));scenario=0;open(active);persist();compile(true);}catch(e){alert(e.message);}finally{e.target.value='';}};
$('code').addEventListener('scroll',highlightSource);
$('code').addEventListener('input',()=>{selected=null;selectedPath=null;controlTree?.select(null,null);refreshControlTree();highlightSource();document.querySelectorAll('.ui-node.selected').forEach(el=>el.classList.remove('selected'));});
controlTree=createControlTree({explorer:document.querySelector('.explorer'),toolbar:document.querySelector('.preview-tools'),onSelect:selectTreeControl,onScopeChange:refreshControlTree,onOpenTemplate:path=>{open(path);controlTree.setView({scope:'file',visible:true});},canReorder:()=>designPresetName==='original',onReorder:reorderTreeControl});
// Mounting after the tree puts the state list underneath it, and both read the same compiled
// switcher: a row is the preview's active state, never a second source of truth.
statesPanel=createStatesPanel({explorer:document.querySelector('.explorer'),
 onActivate:name=>{
   if(name===null){scenario=-1;compile(true);return;}
   const index=(compiled?.states??[]).findIndex(state=>state.name===name);
   if(index<0)throw Error('Состояние не участвует в текущем предпросмотре');
   scenario=index;compile(true);
 },
 onCreate:name=>designStateEdit(source=>stateCreateEdit(source,name)),
 onRename:(name,newName)=>designStateEdit(source=>stateRenameEdit(source,name,newName)),
 onDelete:name=>designStateEdit(source=>stateDeleteEdit(source,name))});
codeEditor=mountEditor($('code'),{getProject:()=>({files,path:active})});
// A panel edit of one file: the guard rejects a change computed against text the editor has
// already moved past, and the reselect walks the AST the new compile yields.
function commitSource({file,source,from,to,insert,reselect,after}){
  if((file===active?$('code').value:files[file])!==source)throw Error('Исходник изменился — повторите операцию');
  parse(source.slice(0,from)+insert+source.slice(to));
  if(active!==file)open(file);
  codeEditor.edit({from,to,insert});
  // A change that adds or removes a row rebuilds its panel once the editor has published the
  // input event and the new AST exists. `after` runs at the very end, after the host has republished
  // the reselected control: the editor's own input event drops the selection, so a caller that keeps
  // a batch of offsets — the ones this change just moved — has to restore it over that handoff.
  if(reselect!==undefined)queueMicrotask(()=>{clearTimeout(compileTimer);compile();let found;const walk=ns=>{for(const n of ns){if(n.start===reselect)found??=n;walk(n.children??[]);}};walk(compiled?.nodes??[]);if(found)select(found);after?.();});
}
// The tree is drawn from a snapshot parse, so the move is computed against that same text.
// Only the entry file compiles into the preview whose AST the reselect walks.
function reorderTreeControl({path,start,targetStart,side}){
  const source=activeTreeDocument?.path===path?activeTreeDocument.source:files[path];
  if(source===undefined){$('caption').textContent='Файл не найден';return;}
  try{
    // The tree names no point in the receiving container's space, so an inside drop only changes the
    // structure: placing a control by the pointer stays the canvas gesture's job.
    const change=side==='inside'?moveIntoContainer(source,start,targetStart):moveAmongSiblings(source,start,targetStart,side);
    if(change)commitSource({file:path,source,from:change.from,to:change.to,insert:change.insert,reselect:path===entry?change.start:undefined});
  }catch(e){$('caption').textContent=e.message;}
}
propertyInspector=createPropertyInspector({container:$('inspector'),designTokens:()=>designReferencesInFiles(files),
 // The inspector renders from a snapshot of the source, so both hooks below read the file as it
 // really stands: `files` only catches the editor up on the next microtask, and a committed row
 // stays on screen with the snapshot it was rendered from.
 liveSource:path=>path===active?$('code').value:files[path],
 commit:commitSource,
 onError:message=>{error=message;sourceDiagnostic=null;output();}});
canvasTools=createCanvasTools({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.preview-tools'),
getScene:()=>renderer==='vector'?vectorPreview?.layoutSnapshot():null,
getSelection:()=>{if(!selected||selectedPath!==entry)return null;const children=compiled?.nodes?.[0]?.children??[];const nodes=sceneControlNodes(children);return {index:nodes.findIndex(n=>n.start===selected.start),root:selected.start===compiled?.nodes?.[0]?.start};},
getMode:()=>mode,setMode:next=>{if(mode!==next)$('run').click();},onChange:()=>{spacingOverlay?.update();layoutInspector?.update();elementTools?.update();}});
const presetPicker=document.createElement('select');presetPicker.className='design-preset';presetPicker.setAttribute('aria-label','Проверка дизайна');
for(const [value,label] of [['original','Исходный вид'],['long','Длинный текст'],['empty','Пустой текст'],['disabled','Недоступные контролы'],['list-empty','Список: пусто'],['list-12','Список: 12 строк'],['list-100','Список: 100 строк'],['loading','Список: загрузка'],['error','Список: ошибка']])presetPicker.add(new Option(label,value));
document.querySelector('.canvas-tools').append(presetPicker);
presetPicker.title='Только предпросмотр: исходники и нативное приложение не меняются';
presetPicker.onchange=()=>{designPresetName=presetPicker.value;try{if(mode!=='design')$('run').click();else render();error='';}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;}renderInspector();refreshControlTree();refreshStatesPanel();output();};
layoutInspector=createLayoutInspector({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.canvas-tools'),
getVisuals:()=>designPresetName==='original'?lastVisuals:[],getRuntime:()=>vectorPreview?.layoutSnapshot(),getMode:()=>mode,
getControl:()=>lastPreviewControls.findIndex(n=>n.start===selected?.start),
readSource:path=>files[path],
readTracks:source=>{const raw=files[source.file].slice(source.from,source.to);const value=parse('component Tracks { Frame { columns: '+raw+'; } }').nodes[0].props.columns;return Array.isArray(value)?value:[value];},
openSource:source=>{open(source.file);$('code').setSelectionRange(source.from,source.to);$('code').focus();},
edit:(source,insert)=>{const previous=files[source.file];if(previous===undefined)throw Error('Файл не найден');const next=previous.slice(0,source.from)+insert+previous.slice(source.to);parse(next);if(active!==source.file)open(source.file);codeEditor.edit({from:source.from,to:source.to,insert});compile();}});
// The palette lists the components this project resolves, and only a change under
// `components/` can alter that, so typing in the page never rebuilds it.
let insertSignature='',insertCatalog=[];
function projectInserts(){
 const parts=[];for(const [path,source]of Object.entries(files))if(path.startsWith('components/')&&path.endsWith('.ui'))parts.push(path+':'+source.length);
 const signature=parts.sort().join('|');
 if(signature!==insertSignature){insertSignature=signature;insertCatalog=insertableControls(files);}
 return insertCatalog;
}
elementTools=createElementTools({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.canvas-tools'),
context:()=>{if(renderer!=='vector'||mode!=='design'||designPresetName!=='original'||error||!vectorPreview)return null;const root=compiled?.nodes[0],children=root?.children??[];return {source:files[entry],path:entry,root,start:selectedPath===entry?selected?.start:null,top:sceneControlNodes(children),pick:start=>{let found;const walk=nodes=>{for(const n of nodes){if(n.start===start)found??=n;walk(n.children??[]);}};if(root)walk([root]);return found??null;},scene:vectorPreview.layoutSnapshot(),grid:lastVisuals.find(v=>v.control===-1)?.grid,inserts:projectInserts(),visuals:lastVisuals,state};},
select,report:message=>{$('caption').textContent=message;},
copy:async text=>{try{await navigator.clipboard.writeText(text);$('caption').textContent='CSS скопирован в буфер';}catch{$('caption').textContent='Буфер обмена недоступен';}},
history:action=>{
 // Undo and redo replace the whole page, so the batch is described by what each control *is* — its
 // key, or the slot it occupies however deep inside a group — and looked up again in the tree that
 // stands afterwards. Nothing is left holding the panels when a control does not come back.
 const identities=selectionIdentities(compiled?.nodes[0],elementTools.selection());
 if(active!==entry)open(entry);
 if(codeEditor[action]())queueMicrotask(()=>{clearTimeout(compileTimer);compile();
  const restored=restoreSelection(compiled?.nodes[0],identities);
  if(restored.nodes.length)select(restored.nodes[0]);
  else{clearSelection();if(identities.length)$('caption').textContent='Ни один из выбранных контролов не вернулся';}
  // The panel is redrawn from what the canvas holds *after* the write, since the offsets the batch was
  // built from are the ones the undo just moved.
  elementTools.setSelection(restored.starts);renderInspector();
 });
},
commit:(change,context)=>{
 if(entry!==context.path||files[entry]!==context.source)throw Error('Исходник изменился — повторите операцию');
 const next=context.source.slice(0,change.from)+change.insert+context.source.slice(change.to);
 if(next===context.source)return;
 parse(next);compileComponents({...files,[entry]:next},entry,state,{measureText:vectorPreview.measureText});
 if(active!==entry)open(entry);codeEditor.edit({from:change.from,to:change.to,insert:change.insert});
 // The editor publishes its input event first; reselect against the new AST afterwards.
 queueMicrotask(()=>{clearTimeout(compileTimer);compile();let found;const walk=ns=>{for(const n of ns){if(n.start===change.start)found=n;walk(n.children);}};walk(compiled.nodes);if(found)select(found);if(change.starts)elementTools.setSelection(change.starts);elementTools.update();});
}});
// Below the rows of one control, the panel offers the names the whole canvas selection has in
// common. Their write spans the batch, so the offsets it reports are what keeps the selection alive:
// the handoff to the reselected control leaves the panel holding one control, and the batch is put
// back over it before the rows are drawn again.
bulkEditor=createBulkEditor({container:$('inspector'),
 commit:({file,source,primary,...change})=>commitSource({file,source,...change,reselect:primary,after:()=>{elementTools.setSelection(change.starts);renderInspector();}}),
 onError:message=>{error=message;sourceDiagnostic=null;output();}});
spacingOverlay=createSpacingOverlay($('canvas'),()=>mode==='design'&&selectedPath===entry?selected:null);
open(active);compile(true);
let studioShell,shellDisposed=false;
studioEvents.emit(studioEventTypes.mode,{mode});
const minimalShell=mountMinimalShell(document.querySelector('#app'),{events:studioEvents,newFile:()=>$('new').click(),clearSelection:()=>{clearSelection();controlTree?.select(null,null);}});
mountStudioShell(document.querySelector('#app')).then(shell=>{if(shellDisposed)shell.destroy();else studioShell=shell;}).catch(e=>log('Контролы Studio: '+e.message));
import.meta.hot?.dispose(()=>{shellDisposed=true;minimalShell.destroy();clearTimeout(saveTimer);clearTimeout(compileTimer);studioShell?.destroy();elementTools?.destroy();canvasTools?.destroy();});
const rendererPicker=document.createElement('select');rendererPicker.id='renderer';rendererPicker.setAttribute('aria-label','Рендерер предпросмотра');rendererPicker.innerHTML='<option value="html">HTML · прежний</option><option value="vector">Вектор · Rust/WASM</option>';$('scenario').before(rendererPicker);rendererPicker.value=renderer;
rendererPicker.onchange=()=>{renderer=rendererPicker.value;writeStorage('localStorage',storageKey('forma-renderer'),renderer);selected=null;vectorPreview?.destroy();vectorPreview=undefined;initVector();compile();};
function initVector(){
 loadVectorRuntime().then(runtime=>{
  if(vectorPreview)return;
  vectorPreview=createVectorPreview({runtime,onSelect:n=>{if(designPresetName==='original')select(n);},onError:message=>{error=String(message);output();},onAction:(action,n)=>{if(n)dispatch(action,n);},onBindingChange:changes=>{for(const {node,path,value}of changes)if(writePreviewBinding(state,node,path,value)){log(`${path} = ${JSON.stringify(value)}`);if(node.events.changed)dispatch(node.events.changed,node,'changed');}schedulePreviewRefresh();}});
  if(renderer==='vector')compile();
 }).catch(e=>{if(renderer==='vector'){error='Векторный WASM не собран: '+e.message;output();}});
}
initVector();

// The MCP transport invokes the same runtime and UI operations as the editor.
function ideCommand(command,args={}){
  const snapshot=()=>({active,entry,scenario:scenario<0?null:compiled?.states[scenario]?.name??null,mode,error,selected:selected?.start??null,debug:{breakOn,paused:!!pending,action:pending?.action??null},renderer,capabilities:{vectorRenderer:!!vectorPreview,rustExecution:!!import.meta.hot,rustDap:false}});
  const requireFile=path=>{if(!Object.hasOwn(files,path))throw Error('File not found: '+path);};
  const findNode=start=>{let found;const walk=nodes=>{for(const n of nodes??[]){if(n.start===start)found=n;walk(n.children);}};walk(compiled?.nodes);if(!found)throw Error('Component not found; refresh component_tree');return found;};
  const applyFiles=()=>{selected=null;open(active);persist();compile(true);};
  switch(command){
    case 'ide_status':return snapshot();
    case 'designer_tree':return controlTree.snapshot();
    case 'designer_tree_view':controlTree.setView(args);return controlTree.snapshot();
    case 'designer_tree_select':controlTree.selectId(args.id);return controlTree.snapshot();
    case 'designer_tree_fold':controlTree.fold(args.id,args.collapsed);return controlTree.snapshot();
    case 'app_run':if(parse(files[entry]).defaults.contextType){import.meta.hot.send('forma:form-run',{files,state});return {status:'requested',renderer:'generated-rust'};}if(renderer==='vector'){nativeSource={entry,source:files[entry],nodes:structuredClone(compiled?.nodes??[])};if(error||!vectorPreview)throw Error(error||'Векторный renderer не готов');import.meta.hot.send('forma:vector-run',compileComponents(files,entry,state,{measureText:vectorPreview.measureText}));return {status:'requested',renderer};}if(error||!compiled)throw Error('Исправьте разметку');render(false);try{import.meta.hot.send('forma:native-run',{files,snapshot:nativeSnapshot($('preview'),compiled)});}finally{render();}return {status:'requested'};
    case 'app_stop':import.meta.hot.send('forma:rust-stop',{});import.meta.hot.send('forma:native-stop',{});import.meta.hot.send('forma:vector-stop',{});return {status:'requested'};
    case 'rust_run':if(!import.meta.hot)throw Error('Requires local dev server');import.meta.hot.send('forma:rust-run',{files});return {status:'requested',note:'Read events_read for compilation output and exit code'};
    case 'rust_stop':import.meta.hot?.send('forma:rust-stop',{});return {status:'stop requested'};
    case 'project_read':return {...files};
    case 'file_read':requireFile(args.path);return {path:args.path,content:files[args.path]};
    case 'file_open':requireFile(args.path);open(args.path);break;
    case 'editor_fold':codeEditor.fold(args);break;
    case 'file_write':{
      if(typeof args.path!=='string'||!args.path)throw Error('Invalid project path');validateProject({[args.path]:args.content});
      if(Object.hasOwn(files,args.path)&&args.expectedContent!==files[args.path])throw Error('File changed or expectedContent missing. Read it first.');
      files[args.path]=args.content;applyFiles();break;
    }
    case 'project_replace':{
      if(JSON.stringify(Object.entries(files).sort())!==JSON.stringify(Object.entries(args.expectedFiles).sort()))throw Error('Project changed. Read it first.');
      validateProject(args.files);
      resetProjectEditing();files=Object.fromEntries(Object.entries(args.files));active=Object.keys(files)[0];entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui'));scenario=0;applyFiles();break;
    }
    case 'preview_renderer':rendererPicker.value=args.renderer;rendererPicker.onchange();break;
    case 'vector_status':return vectorPreview?.snapshot()??{ready:false};
    case 'preview_open':requireFile(args.path);entry=args.path;scenario=0;compile(true);break;
    case 'preview_scenario':{if(args.name===null){if(!compiled?.states?.length)throw Error('The component has no design states');$('scenario').value=-1;$('scenario').onchange({target:$('scenario')});break;}const index=(compiled?.states??[]).findIndex(s=>s.name===args.name);if(index<0)throw Error('Design state not found');scenario=index;$('scenario').value=index;$('scenario').onchange({target:$('scenario')});break;}
    // Same target as the states panel: the file that panel authors, never an arbitrary design file
    // the entry does not use. An agent that wants another one edits it through file_write.
    case 'design_states_read':return designStatesSummary(designStateFile().source);
    case 'design_state_create':return designStateEdit(source=>stateCreateEdit(source,args.name),args.expectedContent);
    case 'design_state_rename':return designStateEdit(source=>stateRenameEdit(source,args.name,args.newName),args.expectedContent);
    case 'design_state_delete':return designStateEdit(source=>stateDeleteEdit(source,args.name),args.expectedContent);
    // The state block is named, not indexed, so a rename between the read and this call cannot land
    // a property in the wrong block; `null` is the base block, which is what the panel's own row is.
    case 'design_state_property':return designStateEdit(source=>statePropertyEdit(source,args.state??null,args.type,args.key,args.property,args.value),args.expectedContent);
    case 'preview_mode':if(mode!==args.mode)$('run').click();break;
    case 'preview_viewport':$(args.device).click();$('preview').classList.toggle('light',args.theme==='light');break;
    case 'component_tree':return {entry,nodes:compiled?.nodes??[],error};
    case 'component_spacing':return spacingOverlay.read();
    case 'component_select':if(error)throw Error('Fix diagnostics before selecting compiled source');select(findNode(args.start));break;
    case 'component_property':{
      if(error||files[entry]!==args.expectedContent)throw Error('Stale or invalid source. Read entry file first.');
      const n=findNode(args.start);if(!Object.hasOwn(n.props,args.property)||typeof n.props[args.property]==='object')throw Error('Only existing literal properties are supported');
      if(typeof n.props[args.property]!==typeof args.value)throw Error('Property type mismatch');
      select(n);const input=Array.from(document.querySelectorAll('[data-prop]')).find(el=>el.dataset.prop===args.property);if(!input)throw Error('Свойство недоступно для правки: включите исходный вид');input.value=String(args.value);input.onchange();break;
    }
    // The palette's own list, so an agent inserts exactly what a designer can insert.
    case 'component_controls':return insertableControls(files);
    case 'component_insert':case 'component_move':case 'component_delete':case 'component_duplicate':{
      // All four splice the entry file the compiled tree came from, against its live text. The
      // splice reports the offset the new, moved or duplicated node takes — for a delete, the
      // parent that keeps the selection — and that is what the preview is asked to select:
      // without it the agent would have to re-read `component_tree` to find its own work.
      const source=active===entry?$('code').value:files[entry];
      if(error||source!==args.expectedContent)throw Error('Stale or invalid source. Read the entry file first.');
      findNode(args.start);
      const change=command==='component_insert'
        ?insertElement(source,args.start,args.markup,false,args.side??'auto')
        :command==='component_move'
        ?moveAmongSiblings(source,args.start,args.target,args.side)
        // A duplicate is the node's own text spliced back after itself, so the key uniquifying
        // the paste path does covers a copied `key:` too. The copy keeps the source's
        // coordinates: the bridge also runs on the HTML preview, where no scene measured an
        // offset to take, and `component_property` can move it from there.
        :command==='component_delete'?removeElement(source,args.start)
        :insertElement(source,args.start,copyElement(source,args.start),false,'after');
      // A move onto the slot the node already holds is a no-op the primitive reports as null.
      // The caller still gets the file text back, because that is the content its next write
      // has to quote as `expectedContent`.
      if(!change)return {start:args.start,content:source,changed:false};
      commitSource({file:entry,source,...change,reselect:change.start});
      return {start:change.start,content:source.slice(0,change.from)+change.insert+source.slice(change.to),changed:true};
    }
    case 'state_read':return structuredClone(state);
    case 'state_set':for(const [key,v]of Object.entries(args.values)){if(!Object.hasOwn(state,key)||typeof state[key]!==typeof v)throw Error('Unknown field or incompatible type: '+key);}Object.assign(state,args.values);render();output();break;
    case 'event_dispatch':if(renderer==='vector'){if(error)throw Error(error);if(mode!=='interact')throw Error('Включите взаимодействие');const n=findNode(args.start);if(!n.events.clicked)throw Error('Выберите кнопку с clicked');vectorPreview.activate();break;}if(error)throw Error('Fix diagnostics first');{const n=findNode(args.start);if(!n.events.clicked)throw Error('No clicked handler');if(resolve(n.props.disabled,state))throw Error('Component is disabled');dispatch(n.events.clicked,n);break;}
    case 'debug_break':$('break').checked=args.enabled;$('break').onchange({target:$('break')});break;
    case 'debug_continue':if(!pending)throw Error('Debugger is not paused');$('continue').click();break;
    case 'debug_reset':$('reset').click();break;
    case 'events_read':return [...logs];
    case 'events_clear':logs=[];output();break;
    case 'diagnostics_read':return {error,entry};
    default:throw Error('Unknown IDE command: '+command);
  }
  return snapshot();
}
if(import.meta.hot){
  let designRequest,designTimer;
  const finishDesignRequest=()=>{const request=designRequest;designRequest=undefined;clearTimeout(designTimer);designTimer=undefined;refreshDesign.disabled=false;return request;};
  const cancelDesignRequest=()=>{if(finishDesignRequest())log('Соединение потеряно: результат дизайн-данных неизвестен. Обновите их после подключения.');};
  const refreshDesign=document.createElement('button');refreshDesign.textContent='↻ Данные дизайна';$('run').before(refreshDesign);
  refreshDesign.title='Выполнить локальный src/design.rs (без песочницы), получить текстовые константы и результаты методов';
  refreshDesign.onclick=()=>{
    if(!compiled||designRequest)return;
    const refs=designReferences(compiled);
    if(!refs.length){log('Нет ссылок design в дизайне');return;}
    if(!confirm('Выполнить src/design.rs для получения дизайн-данных? Rust-код работает с правами вашего пользователя.'))return;
    designRequest={id:crypto.randomUUID(),source:JSON.stringify(files)};
    refreshDesign.disabled=true;
    designTimer=setTimeout(()=>{if(finishDesignRequest())log('Ответ дизайн-данных не получен. Исполнение могло завершиться; автоматического повтора нет.');},45000);
    try{import.meta.hot.send('forma:design-data',{id:designRequest.id,files,refs});}
    catch(e){finishDesignRequest();log('Не удалось отправить запрос дизайн-данных: '+e.message);}
  };
  import.meta.hot.on('vite:ws:disconnect',cancelDesignRequest);
  import.meta.hot.dispose(()=>{finishDesignRequest();import.meta.hot.off('vite:ws:disconnect',cancelDesignRequest);import.meta.hot.off('forma:design-data-result',receiveDesignData);});
  const receiveDesignData=data=>{
    if(!designRequest||data?.id!==designRequest.id)return;
    const request=finishDesignRequest();
    if(request.source!==JSON.stringify(files)){log('Проект изменился: обновите дизайн-данные ещё раз');return;}
    if(data.error){error=data.error;output();return;}
    setDesignData(data.values);compile();log('Дизайн-данные обновлены из Rust');
  };
  import.meta.hot.on('forma:design-data-result',receiveDesignData);
  const runApp=document.createElement('button');runApp.textContent='▶ Приложение';runApp.title='Запустить окно; F9 в окне включает выбор элемента и живое дерево в Studio';runApp.className='accent';$('run').before(runApp);
  runApp.onclick=()=>{tab='events';syncTabs();ideCommand('app_run');output();};
  const runRust=document.createElement('button');runRust.textContent='▶ Rust';runRust.title='Собрать и запустить Cargo-проект';$('run').before(runRust);
  const stopRust=document.createElement('button');stopRust.textContent='■';stopRust.title='Остановить Rust';stopRust.disabled=true;runRust.after(stopRust);
  runRust.onclick=()=>{persist();tab='events';syncTabs();log('Запуск Rust через Cargo (offline)…');runRust.disabled=true;stopRust.disabled=false;import.meta.hot.send('forma:rust-run',{files});};
  stopRust.onclick=()=>import.meta.hot.send('forma:rust-stop',{});
  const rustOutput=data=>{nativeBuffer+=data.text;let lineEnd;while((lineEnd=nativeBuffer.indexOf('\n'))>=0){const line=nativeBuffer.slice(0,lineEnd);nativeBuffer=nativeBuffer.slice(lineEnd+1);if(line.startsWith('FORMA_INSPECT ')){try{showNativeInspection(JSON.parse(line.slice(14)));}catch(e){log('Ошибка данных инспекции: '+e.message);}}}if(nativeBuffer.length>200000)nativeBuffer='';if(!data.text.startsWith('FORMA_INSPECT '))log('[Rust] '+data.text);if(data.kind==='finished'||data.kind==='error'){runRust.disabled=false;stopRust.disabled=true;}};
  import.meta.hot.on('forma:rust-output',rustOutput);
  const hello=()=>import.meta.hot.send('forma:hello',{});
  const handle=({id,command,args})=>{try{import.meta.hot.send('forma:result',{id,result:ideCommand(command,args)});}catch(e){import.meta.hot.send('forma:result',{id,error:e.message});}};
  import.meta.hot.on('forma:command',handle);
  import.meta.hot.on('vite:ws:connect',hello);hello();
  import.meta.hot.dispose(()=>{import.meta.hot.off('forma:command',handle);import.meta.hot.off('vite:ws:connect',hello);import.meta.hot.off('forma:rust-output',rustOutput);nativeBuffer='';});
}
