import {readStorage,writeStorage,validateProject,restoreProject,writeProject,readDirectoryProject,writeDirectoryProject} from './browser-storage.js';
import {studioControlsProject} from './studio-controls-builtin.js';
import {openScadProject} from './openscad-builtin.js';
import {sampleProject} from './sample-project.js';
import {mountStudioShell} from './studio-shell.js';
import {mountMinimalShell} from './minimal-shell.js';
import {createStudioEvents,studioEventTypes} from './studio-events.js';
import {createEventScope} from './event-scope.js';
import {createLatestOnly} from './latest-only.js';
import {createIdeCommands} from './mcp-commands.js';
import {renderHtmlPreview} from './html-preview.js';
import {mountDevBridge} from './dev-bridge.js';
const studioEvents=createStudioEvents();
const requestedProject=new URLSearchParams(location.search).get('project');
const isStudioControls=requestedProject==='studio-controls';
const isSampleWorkspace=requestedProject==='knowledge-workspace';
const isOpenScad=!isStudioControls&&!isSampleWorkspace;
const storageKey=name=>isStudioControls?name+':studio-controls-guide':isOpenScad?name+':openscad-studio-v5':name;
import {designPreset,collectionScenario} from './design-presets.js';
let designPresetName='original',nativeSource=null;
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
import {copyElement,extractElement,insertElement,moveAmongSiblings,moveIntoContainer,removeElement} from './element-edit.js';
import {restoreSelection,selectionIdentities} from './selection-restore.js';
import {findNodeByStart} from './element-tree.js';
import {mountEditor} from './editor.js';
import {detectTokenizerLanguage} from './tokenizer-highlight.js';
import {createPropertyInspector} from './property-inspector.js';
import {createBulkEditor} from './bulk-edit.js';
import {designReferencesInFiles} from './design-data.js';
import {insertableControls} from './control-catalog.js';
import {createSpacingOverlay} from './spacing-overlay.js';
import {nativeSnapshot} from './native-snapshot.js';
import {buildNativeRustExport} from './native-export.js';
import {loadVectorRuntime,createVectorPreview} from './vector-preview.js';
import {createComponentCompiler,sceneControlNodes} from './components.js';
import {evaluate} from './component-semantics.js';
import {writePreviewBinding,executePreviewStateAction,matchPreviewShortcut} from './preview-state.js';
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
let renderer=isStudioControls||isOpenScad||readStorage('localStorage',storageKey('forma-renderer'))==='vector'?'vector':'html';
import {parse,resolve,validateDesign,designStatePatch,formatSource,lintDocument} from './language.js';

const initialProject=isStudioControls?studioControlsProject:isOpenScad?openScadProject:sampleProject;
let files=restoreProject(readStorage('localStorage',storageKey('forma-project')),initialProject);
for(const [k,v] of Object.entries(initialProject))files[k]??=v;
if(isSampleWorkspace){files['components/Button.ui']??=sampleProject['components/Button.ui'];files['components/TextInput.ui']??=sampleProject['components/TextInput.ui'];files['ui/TwoButtons.ui']??=sampleProject['ui/TwoButtons.ui'];}
let appBackend=['egui','svet','ask'].includes(readStorage('localStorage',storageKey('forma-backend')))?readStorage('localStorage',storageKey('forma-backend')):'svet';
let appPlatform=['desktop','ios','android'].includes(readStorage('localStorage',storageKey('forma-platform')))?readStorage('localStorage',storageKey('forma-platform')):'desktop';
files['Cargo.toml']??=`[package]\nname = "forma-preview-app"\nversion = "0.1.0"\nedition = "2021"\n`;
files['src/main.rs']??=`mod actions;\n\nfn main() {\n    let mut state = actions::SearchState {\n        query: String::from("Архитектура"),\n        loading: false,\n        status: String::from("Готов"),\n    };\n    println!("Rust-приложение запущено");\n    println!("Запрос: {}", state.query);\n    actions::search(&mut state);\n    println!("После actions::search: loading={}, status={}", state.loading, state.status);\n}\n`;
let active=Object.keys(files)[0],entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui')), scenario=0,state={},compiled=null,selected=null,mode=readStorage('sessionStorage',storageKey('forma-canvas-mode'))==='interact'?'interact':'design',pending=null,breakOn=false,logs=[],lintWarnings=[],tab='problems',error='',saveTimer,compileTimer;
try{const view=JSON.parse(readStorage('localStorage',storageKey('forma-view')));if(view?.entry in files)entry=view.entry;if(view?.active in files)active=view.active;}catch{}
function persistView(){try{writeStorage('localStorage',storageKey('forma-view'),JSON.stringify({active,entry}));}catch{}}
const $=id=>document.getElementById(id);
// The file as it really stands: the editor's text for the open file, which `files` only catches up
// with on the next microtask, and the stored text for every other one.
const liveSource=path=>path===active?$('code').value:files[path];
function reportError(message){error=message;sourceDiagnostic=null;output();}
function downloadJson(name,text){const url=URL.createObjectURL(new Blob([text],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
const currentProjectId=isStudioControls?'studio-controls':isSampleWorkspace?'knowledge-workspace':'openscad';
const currentProjectName=isStudioControls?'Studio Controls':isSampleWorkspace?'knowledge-workspace':'open-scad-viewer';
const openProjectsList=[
 {id:'openscad',name:'open-scad-viewer',subtitle:'OpenSCAD Viewer · локальный проект',href:'/?project=openscad'},
 {id:'studio-controls',name:'Studio Controls',subtitle:'Контролы Studio · дизайн-система',href:'/?project=studio-controls'},
 {id:'knowledge-workspace',name:'knowledge-workspace',subtitle:'Библиотека знаний · пример проекта',href:'/?project=knowledge-workspace'},
];
document.querySelector('#app').innerHTML=`
<header><div class="brand"><span class="brandmark">F</span> forma <small>STUDIO</small></div><div class="project-title">${currentProjectName} <span> / локальный проект</span></div><a class="controls-link" href="${isOpenScad?'/?project=studio-controls':'/?project=openscad'}">${isOpenScad?'Контролы Studio':'OpenSCAD Studio'}</a><a class="controls-link" href="${isSampleWorkspace?'/?project=openscad':'/?project=knowledge-workspace'}">${isSampleWorkspace?'OpenSCAD Studio':'Библиотека знаний'}</a><a class="controls-link" href="/vector-ui/examples/controls.html">Контролы · Reveal</a><button id="open-folder" title="Открыть локальную папку проекта с автосохранением на диск">Папка</button><button id="import">Открыть</button><button id="export">Экспорт</button><button id="export-native" title="Экспортировать автономный Rust-проект (Cargo.toml + src/ui_bundle.rs + .ui) для нативной сборки">Экспорт Rust</button><select id="app-platform" aria-label="Целевая платформа" title="Выбор платформы запуска: Desktop, iOS или Android"><option value="desktop">Платформа: Desktop</option><option value="ios">Платформа: iOS</option><option value="android">Платформа: Android</option></select><select id="app-backend" aria-label="Бэкенд запуска" title="Выбор бэкенда запуска приложения: egui или svet"><option value="ask">Бэкенд: выбор (egui / svet)</option><option value="egui">Бэкенд: egui</option><option value="svet">Бэкенд: svet</option></select><button id="run" title="Переключить встроенный preview между дизайном и взаимодействием">${mode==='design'?'Взаимодействие':'Дизайн'}</button></header>
<div class="workspace"><aside class="explorer"><div class="section-title"><div class="project-switcher-wrap"><button id="project-switcher" class="project-switcher-btn" type="button" aria-haspopup="listbox" aria-expanded="false" title="Переключить открытый проект"><span id="project-switcher-label">${currentProjectName}</span><svg class="project-switcher-chevron" viewBox="0 0 12 12" aria-hidden="true"><polyline points="2.5 4.2 6 7.8 9.5 4.2" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg></button><div id="project-switcher-menu" class="project-switcher-menu" role="listbox" hidden>${openProjectsList.map(p=>`<a class="project-switcher-item ${p.id===currentProjectId?'active':''}" role="option" aria-selected="${p.id===currentProjectId}" href="${p.href}"><span class="project-switcher-item-main"><span class="project-switcher-item-name">${p.name}</span><small class="project-switcher-item-sub">${p.subtitle}</small></span><span class="project-switcher-check">${p.id===currentProjectId?'✓':''}</span></a>`).join('')}<div class="project-switcher-divider"></div><button type="button" class="project-switcher-item project-switcher-action" id="project-switcher-open-folder"><span class="project-switcher-item-main"><span class="project-switcher-item-name">Открыть локальную папку…</span><small class="project-switcher-item-sub">Подключить директорию с диска</small></span></button></div></div><div class="tree-header-actions"><button id="new" class="tree-action-btn" type="button" title="Новый файл"><svg viewBox="0 0 16 16"><path d="M4 2.5h5.2L12.5 5.8v7.7H4z" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round"/><line x1="8" y1="6.8" x2="8" y2="11.2" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/><line x1="5.8" y1="9" x2="10.2" y2="9" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/></svg></button><button id="new-folder" class="tree-action-btn" type="button" title="Новая папка"><svg viewBox="0 0 16 16"><path d="M2.2 4.2h4.2l1.3 1.6h6.1v6.7H2.2z" fill="none" stroke="currentColor" stroke-width="1.35" stroke-linejoin="round"/><line x1="8" y1="7.2" x2="8" y2="11" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/><line x1="6.1" y1="9.1" x2="9.9" y2="9.1" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/></svg></button><button id="tree-locate" class="tree-action-btn" type="button" title="Показать активный файл в дереве"><svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="4.2" fill="none" stroke="currentColor" stroke-width="1.35"/><circle cx="8" cy="8" r="1.3" fill="currentColor"/><line x1="8" y1="1.8" x2="8" y2="3.6" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/><line x1="8" y1="12.4" x2="8" y2="14.2" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/><line x1="1.8" y1="8" x2="3.6" y2="8" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/><line x1="12.4" y1="8" x2="14.2" y2="8" stroke="currentColor" stroke-width="1.35" stroke-linecap="round"/></svg></button><button id="tree-collapse-all" class="tree-action-btn" type="button" title="Свернуть все папки"><svg viewBox="0 0 16 16"><polyline points="4.5 6.8 8 3.5 11.5 6.8" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/><polyline points="4.5 12.5 8 9.2 11.5 12.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg></button><button id="tree-search-toggle" class="tree-action-btn" type="button" title="Поиск по дереву (⌘F)"><svg viewBox="0 0 16 16"><circle cx="7.2" cy="7.2" r="4.3" fill="none" stroke="currentColor" stroke-width="1.4"/><line x1="10.4" y1="10.4" x2="13.5" y2="13.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg></button></div></div><div id="tree-search-bar" class="tree-search-bar" hidden><svg class="tree-search-icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="7.2" cy="7.2" r="4.3" fill="none" stroke="currentColor" stroke-width="1.4"/><line x1="10.4" y1="10.4" x2="13.5" y2="13.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg><input id="tree-search-input" type="text" placeholder="Фильтр файлов и папок…" spellcheck="false" autocomplete="off"><button id="tree-search-clear" type="button" title="Очистить фильтр (Esc)">✕</button></div><div id="tree" tabindex="0" role="tree" aria-label="Файлы проекта"></div><div class="explorer-note"><span class="dot"></span> Локальное сохранение<br><small>Проект хранится в этом браузере.<br>Экспортируйте для резервной копии.</small></div></aside>
<main><div class="tabs"><span class="file-icon">◇</span><span id="filename"></span><span id="saved">Сохранено</span><button id="format-code" title="Форматировать код (Shift+Alt+F)">Формат</button><button id="entry" title="Показать этот UI в предпросмотре">Показать UI</button></div><div class="editor-preview"><section class="code-pane"><div class="pane-toolbar"><span>РАЗМЕТКА / КОД</span><span id="language">Forma UI</span></div><div class="code-wrap"><div id="lines"></div><textarea id="code" spellcheck="false" aria-label="Редактор исходного кода"></textarea></div></section><section class="preview-pane"><div class="pane-toolbar"><span>LIVE PREVIEW <i class="dot"></i></span><select id="scenario" aria-label="Состояние дизайна"></select></div><div class="preview-tools"><button id="desktop" class="chosen">Desktop</button><button id="mobile">Mobile</button><button id="theme">◐ Тема</button><span id="preview-name"></span></div><div id="canvas"><div id="preview"></div></div><div class="preview-caption" id="caption">Выберите элемент, чтобы найти его в разметке</div></section></div>
<section class="bottom"><div class="bottom-tabs"><button data-tab="problems" class="chosen">Диагностика <span id="problem-count">0</span></button><button data-tab="events">События <span id="event-count">0</span></button><button data-tab="state">Состояние</button><div class="debug-controls"><label><input type="checkbox" id="break"> Break on event</label><button id="continue" disabled>▶ Продолжить</button><button id="reset">↺ Сброс</button></div></div><div id="output"></div></section></main>
<aside class="inspector"><div class="section-title">ИНСПЕКТОР</div><div id="inspector"><div class="empty-icon">⌖</div><p>Выберите компонент</p><small>Свойства и привязки появятся здесь</small></div><div class="debug-info"><span>UI DEBUGGER</span><p id="debug-status">Готов</p><small>События и состояние дизайн-runtime.<br>Rust / DAP не подключён.</small></div></aside></div>
<footer><span class="dot"></span><span id="status">Готов</span><span class="footer-right">Forma UI · UTF-8 <span id="position">Ln 1, Col 1</span></span></footer><input type="file" id="upload" accept=".json" hidden>`;
const ESC_MAP={'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'};
const ESC_RE=/[&<>"']/g;
const esc=s=>String(s??'').replace(ESC_RE,c=>ESC_MAP[c]);
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
  button.onclick=()=>downloadJson('forma-project-recovery.json',recovery);
  $('export').after(button);
 }
 if(damaged!==null)saveStatus('damaged','Сохранённый проект повреждён: открыта исходная версия');
}
let directoryHandle=null,directorySavedFiles={},selectedTreeItem=active,treeFilter='',draggedTreePath=null,dragExpandTimer=0;
const workspaceEntries=new Map(),expandedDirs=new Set();
async function loadWorkspaceDir(dir=''){
 try{
  const res=await fetch(`/__forma_workspace/list?dir=${encodeURIComponent(dir)}`);
  if(!res.ok)return false;
  const data=await res.json();
  if(Array.isArray(data?.entries)){workspaceEntries.set(dir,data.entries);return true;}
 }catch{}
 return false;
}
async function refreshWorkspaceDirs(dirs=['']){
 const unique=[...new Set(['', ...expandedDirs, ...dirs.filter(Boolean)])];
 await Promise.all(unique.map(d=>loadWorkspaceDir(d)));
 tree(true);
}
function resetProjectEditing(){clearTimeout(saveTimer);clearTimeout(compileTimer);saveTimer=compileTimer=undefined;setDesignData({});codeEditor?.resetProject();}
function saveStatus(status,text){$('saved').textContent=text;studioEvents.emit(studioEventTypes.save,{status,text});}
function persist(){try{if(!writeProject(storageKey('forma-project'),files))throw Error('Storage unavailable');if(workspaceEntries.has('')&&active&&!active.startsWith('components/')&&!active.startsWith('assets/')){fetch(`/__forma_workspace/write?path=${encodeURIComponent(active)}`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({content:files[active]})}).then(()=>saveStatus('saved','Сохранено (open-scad-viewer)')).catch(()=>saveStatus('saved','Сохранено'));return;}if(directoryHandle){const snap={...files};writeDirectoryProject(directoryHandle,snap,directorySavedFiles).then(()=>{directorySavedFiles=snap;saveStatus('saved',`Сохранено (${directoryHandle.name})`);}).catch(()=>{directoryHandle=null;saveStatus('saved','Сохранено в браузере (доступ к папке потерян)');});}else saveStatus('saved','Сохранено');}catch{saveStatus('unsaved','Не сохранено: экспортируйте проект');}}
function selectTreeRow(targetPath){
 selectedTreeItem=targetPath;
 document.body.classList.toggle('is-ui-file',!!targetPath?.endsWith('.ui'));
 for(const b of $('tree').querySelectorAll('.file')){
  const p=b.dataset.path??b.dataset.folder;
  b.classList.toggle('active',p===selectedTreeItem);
 }
}
function parentDirOf(path=''){
 const idx=path.lastIndexOf('/');
 return idx>=0?path.slice(0,idx):'';
}
function selectedTargetDir(){
 if(!selectedTreeItem)return '';
 const btn=$('tree').querySelector(`.file[data-folder="${CSS.escape(selectedTreeItem)}"]`);
 if(btn||expandedDirs.has(selectedTreeItem)||workspaceEntries.has(selectedTreeItem))return selectedTreeItem;
 return parentDirOf(selectedTreeItem);
}
async function toggleFolder(folder,btn){
 const nowExpanded=!expandedDirs.has(folder);
 if(nowExpanded){expandedDirs.add(folder);if(!workspaceEntries.has(folder))await loadWorkspaceDir(folder);}
 else expandedDirs.delete(folder);
 if(btn&&!treeFilter){
  btn.setAttribute('aria-expanded',String(nowExpanded));
  const box=btn.nextElementSibling;
  if(box?.classList.contains('folder-children')){
   const inner=box.firstElementChild,level=Number(btn.getAttribute('aria-level')??1);
   if(nowExpanded&&inner&&!inner.childElementCount){
    inner.innerHTML=renderWorkspaceDir(folder,level+1);
    bindTreeEvents(inner);
    requestAnimationFrame(()=>box.classList.add('expanded'));
   }else box.classList.toggle('expanded',nowExpanded);
   return;
  }
 }
 tree(true);
}
function matchesTreeFilter(item){
 if(!treeFilter)return true;
 const q=treeFilter.toLowerCase();
 if(item.name.toLowerCase().includes(q)||item.path.toLowerCase().includes(q))return true;
 if(item.kind==='directory'){
  const children=workspaceEntries.get(item.path)??[];
  return children.some(matchesTreeFilter);
 }
 return false;
}
function renderWorkspaceDir(dir,level){
 const entries=(workspaceEntries.get(dir)??[]).filter(matchesTreeFilter);
 return entries.map((item,idx)=>{
  const indent=(level-1)*12,pad=8+indent,isSelected=item.path===selectedTreeItem;
  if(item.kind==='directory'){
   const expanded=expandedDirs.has(item.path)||(!!treeFilter&&(workspaceEntries.get(item.path)??[]).some(matchesTreeFilter));
   const count=item.count??(workspaceEntries.has(item.path)?workspaceEntries.get(item.path).length:0);
   return `<button class="file folder-item ${isSelected?'active':''}" draggable="true" data-folder="${esc(item.path)}" data-count="${count}" aria-expanded="${expanded}" aria-level="${level}" style="padding-left:${pad}px;--indent:${indent}px;--row-idx:${Math.min(idx,14)}"><svg class="tree-chevron" viewBox="0 0 12 12" aria-hidden="true"><polyline points="4 2.5 8.5 6 4 9.5" fill="none" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>${esc(item.name)}</button><div class="folder-children ${expanded?'expanded':''}" data-children-for="${esc(item.path)}" style="--guide-indent:${indent}px"><div class="folder-children-inner">${expanded?renderWorkspaceDir(item.path,level+1):''}</div></div>`;
  }
  return `<button class="file ${isSelected?'active':''}" draggable="true" data-path="${esc(item.path)}" aria-level="${level}" style="padding-left:${pad}px;--indent:${indent}px;--row-idx:${Math.min(idx,14)}"><span class="${item.path.endsWith('.rs')||item.path.endsWith('.toml')?'rust':'ui'}">${item.path.endsWith('.rs')?'R':item.path.endsWith('.toml')?'⚙':'◇'}</span>${esc(item.name)}</button>`;
 }).join('');
}
async function activateTreeFile(path){
 selectTreeRow(path);
 if(!(path in files)&&workspaceEntries.has('')){
  try{
   const res=await fetch(`/__forma_workspace/read?path=${encodeURIComponent(path)}`);
   const data=await res.json();
   if(res.ok&&typeof data.content==='string')files[path]=data.content;
   else{reportError(data?.error??`Не удалось открыть ${path}`);return;}
  }catch(e){reportError(e.message);return;}
 }
 if(path.endsWith('.ui')){
  const uiKey='ui/'+path.split('/').at(-1);
  files[uiKey]=files[path];
  if(!path.endsWith('.design.ui')){entry=uiKey;scenario=0;}
  controlTree?.setView({visible:true});
  open(path);
  if(!path.endsWith('.design.ui'))compile(true);
 }else{
  open(path);
  document.body.classList.add('show-code');
 }
}
const treeUndoStack=[],treeRedoStack=[];
function pushTreeHistory(op){
 treeUndoStack.push(op);
 if(treeUndoStack.length>50)treeUndoStack.shift();
 treeRedoStack.length=0;
}
async function undoTreeOperation(){
 const op=treeUndoStack.pop();
 if(!op)return false;
 const ok=await op.undo();
 if(ok!==false){treeRedoStack.push(op);$('status').textContent=`Отменено: ${op.label}`;}
 return true;
}
async function redoTreeOperation(){
 const op=treeRedoStack.pop();
 if(!op)return false;
 const ok=await op.redo();
 if(ok!==false){treeUndoStack.push(op);$('status').textContent=`Повторено: ${op.label}`;}
 return true;
}
async function applyTreeRename(fromPath,toPath){
 if(workspaceEntries.has('')){
  try{
   const res=await fetch('/__forma_workspace/rename',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({from:fromPath,to:toPath})});
   const data=await res.json();
   if(!res.ok){reportError(data?.error??'Ошибка перемещения');return false;}
   if(fromPath in files){files[toPath]=files[fromPath];delete files[fromPath];}
   if(active===fromPath)active=toPath;
   selectedTreeItem=toPath;
   if(expandedDirs.has(fromPath)){expandedDirs.delete(fromPath);expandedDirs.add(toPath);}
   const toParent=parentDirOf(toPath);
   if(toParent)expandedDirs.add(toParent);
   await refreshWorkspaceDirs([parentDirOf(fromPath),toParent]);
   return true;
  }catch(e){reportError(e.message);return false;}
 }
 if(fromPath in files){
  files[toPath]=files[fromPath];delete files[fromPath];
  if(active===fromPath)open(toPath);else{selectedTreeItem=toPath;tree(true);}
  persist();
  return true;
 }
 return false;
}
async function moveTreeItem(fromPath,targetDir){
 if(!fromPath)return;
 const name=fromPath.split('/').at(-1);
 const toPath=targetDir?`${targetDir}/${name}`:name;
 if(toPath===fromPath||toPath.startsWith(fromPath+'/'))return;
 if(await applyTreeRename(fromPath,toPath)){
  pushTreeHistory({
   label:`перемещение «${name}»`,
   undo:()=>applyTreeRename(toPath,fromPath),
   redo:()=>applyTreeRename(fromPath,toPath),
  });
 }
}
function bindTreeEvents(scope=$('tree')){
 scope.querySelectorAll('[data-folder]').forEach(b=>{
  const isArrowClick=e=>{
   const level=Number(b.getAttribute('aria-level')??1);
   return e.clientX-b.getBoundingClientRect().left<=18+(level-1)*12;
  };
  b.onclick=async e=>{
   const folder=b.dataset.folder;
   selectTreeRow(folder);
   if(isArrowClick(e))await toggleFolder(folder,b);
  };
  b.ondblclick=async e=>{
   if(isArrowClick(e))return;
   const folder=b.dataset.folder;
   selectTreeRow(folder);
   await toggleFolder(folder,b);
  };
 });
 scope.querySelectorAll('[data-path]').forEach(b=>{
  b.onclick=()=>activateTreeFile(b.dataset.path);
 });
 scope.querySelectorAll('.file').forEach(b=>{
  const itemPath=b.dataset.path??b.dataset.folder;
  b.ondragstart=e=>{
   draggedTreePath=itemPath;
   b.classList.add('dragging');
   e.dataTransfer.effectAllowed='move';
   e.dataTransfer.setData('text/plain',itemPath);
  };
  b.ondragend=()=>{
   draggedTreePath=null;
   clearTimeout(dragExpandTimer);
   b.classList.remove('dragging');
   $('tree').classList.remove('drop-root');
   $('tree').querySelectorAll('.drop-target').forEach(el=>el.classList.remove('drop-target'));
  };
  b.ondragover=e=>{
   if(!draggedTreePath)return;
   const targetFolder=b.dataset.folder??parentDirOf(b.dataset.path);
   if(targetFolder===draggedTreePath||targetFolder.startsWith(draggedTreePath+'/'))return;
   e.preventDefault();e.stopPropagation();
   e.dataTransfer.dropEffect='move';
   $('tree').classList.remove('drop-root');
   const highlightRow=b.dataset.folder?b:$('tree').querySelector(`.file[data-folder="${CSS.escape(targetFolder)}"]`);
   $('tree').querySelectorAll('.drop-target').forEach(el=>{if(el!==highlightRow)el.classList.remove('drop-target');});
   if(highlightRow){
    highlightRow.classList.add('drop-target');
    if(b.dataset.folder&&!expandedDirs.has(b.dataset.folder)&&!dragExpandTimer){
     dragExpandTimer=setTimeout(()=>{dragExpandTimer=0;toggleFolder(b.dataset.folder,b);},480);
    }
   }else $('tree').classList.add('drop-root');
  };
  b.ondragleave=()=>{clearTimeout(dragExpandTimer);dragExpandTimer=0;};
  b.ondrop=async e=>{
   e.preventDefault();e.stopPropagation();
   clearTimeout(dragExpandTimer);dragExpandTimer=0;
   const from=draggedTreePath||e.dataTransfer.getData('text/plain');
   const targetFolder=b.dataset.folder??parentDirOf(b.dataset.path);
   $('tree').classList.remove('drop-root');
   $('tree').querySelectorAll('.drop-target').forEach(el=>el.classList.remove('drop-target'));
   await moveTreeItem(from,targetFolder);
  };
 });
}
function tree(force=false){
 if(workspaceEntries.has('')){
  const buttons=[...$('tree').querySelectorAll('.file')];
  if(!force&&buttons.length){selectTreeRow(selectedTreeItem);return;}
  const html=renderWorkspaceDir('',1);
  $('tree').innerHTML=html||`<div class="tree-empty-hint">${treeFilter?'Ничего не найдено':'Папка пуста'}</div>`;
  bindTreeEvents();
  return;
 }
 const allPaths=Object.keys(files),hasAppUi=allPaths.some(p=>p.startsWith('ui/')),paths=(hasAppUi?allPaths.filter(p=>!p.startsWith('components/')&&!p.startsWith('assets/')):allPaths).filter(p=>!treeFilter||p.toLowerCase().includes(treeFilter.toLowerCase()));
 const buttons=[...$('tree').querySelectorAll('[data-path]')],existing=new Set(buttons.map(b=>b.dataset.path));
 if(!force&&buttons.length===paths.length&&paths.every(path=>existing.has(path))){selectTreeRow(selectedTreeItem);return;}
 const groups={},rootFiles=[];for(const path of paths){if(path.includes('/')){(groups[path.slice(0,path.lastIndexOf('/'))]??=[]).push(path);}else rootFiles.push(path);}
 const fileBtn=(p,root=false)=>`<button class="file ${root?'root-file ':''}${p===selectedTreeItem?'active':''}" draggable="true" data-path="${esc(p)}"><span class="${p.endsWith('.rs')||p.endsWith('.toml')?'rust':'ui'}">${p.endsWith('.rs')?'R':p.endsWith('.toml')?'⚙':'◇'}</span>${esc(p.split('/').at(-1))}</button>`;
 $('tree').innerHTML=Object.entries(groups).map(([folder,items])=>`<div class="folder">⌄ &nbsp; ${esc(folder)}</div>${items.map(p=>fileBtn(p)).join('')}`).join('')+rootFiles.map(p=>fileBtn(p,true)).join('');
 bindTreeEvents();
}
if(isOpenScad){loadWorkspaceDir('').then(ok=>{if(ok)tree(true);});}
function open(path){active=path;selectedTreeItem=path;persistView();codeEditor?.setLanguage(path);$('filename').textContent=path.split('/').at(-1);$('code').value=files[path];const langInfo=detectTokenizerLanguage(path);$('language').textContent=langInfo?langInfo.label:'Текст';$('entry').disabled=!path.endsWith('.ui')||path.endsWith('.design.ui');$('format-code').disabled=!path.endsWith('.ui');lines();tree();refreshControlTree();refreshStatesPanel();}
function countLines(text,end=text.length){let n=1;for(let i=0;i<end;i++)if(text.charCodeAt(i)===10)n++;return n;}
function lines(){
  if(codeEditor){const {line,column}=codeEditor.position();$('position').textContent=`Ln ${line}, Col ${column}`;}
  else {const text=$('code').value,total=countLines(text);$('lines').textContent=Array.from({length:total},(_,i)=>i+1).join('\n');const sel=$('code').selectionStart,line=countLines(text,sel),lastNl=text.lastIndexOf('\n',sel-1);$('position').textContent=`Ln ${line}, Col ${sel-lastNl}`;}
  highlightSource();
}
function highlightSource(){
  spacingOverlay?.update();canvasTools?.update();elementTools?.update();
  if(codeEditor){codeEditor.highlight(selected&&active===selectedPath?selected:null);return;}
  const code=$('code');let layer=$('source-highlight');
  if(!layer){layer=document.createElement('div');layer.id='source-highlight';layer.setAttribute('aria-hidden','true');code.parentElement.append(layer);}
  layer.hidden=!selected||active!==selectedPath;
  if(layer.hidden)return;
  const first=countLines(code.value,selected.start)-1;
  const last=countLines(code.value,selected.end)-1;
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
function output(){const problemTotal=(error?1:0)+(error?0:lintWarnings.length);$('problem-count').textContent=problemTotal;studioEvents.emit(studioEventTypes.problems,{count:problemTotal});$('event-count').textContent=logs.length;$('output').innerHTML=tab==='state'?`<pre>${esc(JSON.stringify(state,null,2))}</pre>`:tab==='events'?logs.map(l=>`<div class="log"><time>${l.time}</time>${esc(l.text)}</div>`).join('')||'<p class="muted">События появятся при взаимодействии с предпросмотром.</p>':error?`<p class="error">● ${esc(error)}</p>`:lintWarnings.length?lintWarnings.map(w=>`<p class="warn">⚠ ${esc(w.message)}</p>`).join(''):'<p class="ok">✓ Разметка проверена. Ошибок нет.</p>';diagnosticLinks();}
function diagnosticLinks(){
 if(tab!=='problems'||sourceDiagnostic?.message!==error)return;
 for(const [label,source]of [['К ошибке',sourceDiagnostic.source],['К определению узла',sourceDiagnostic.related]]){
  if(!source||typeof files[source.file]!=='string')continue;
  const button=document.createElement('button'),line=files[source.file].slice(0,source.from).split('\n').length;
  button.textContent=`${label} · ${source.file}:${line}`;
  button.onclick=()=>{open(source.file);$('code').setSelectionRange(source.from,source.to);$('code').focus();};$('output').append(button);
 }
}
function syncHotRunningApp(){
  if(!import.meta.hot||error||!compiled)return;
  try{
    if(renderer==='vector'&&vectorPreview){
      const linked=compileComponents(files,entry,state,{measureText:vectorPreview.measureText,designState:scenario<0?null:compiled?.states[scenario]?.name??null});
      nativeSource={entry,source:files[entry],nodes:structuredClone(compiled?.nodes??[])};
      import.meta.hot.send('forma:vector-update',linked);
    }else if(renderer==='html'){
      import.meta.hot.send('forma:native-update',{snapshot:nativeSnapshot($('preview'),compiled)});
    }
  }catch{}
}
function compile(reset=false){persistView();try{if(!entry||!files[entry])throw Error('Откройте .ui компонент и нажмите «Показать UI»');const next=parse(files[entry]);try{lintWarnings=lintDocument(files[entry])??[];}catch{lintWarnings=[];}const states=[];designFiles=[];for(const ref of next.designs){const base=entry.includes('/')?entry.slice(0,entry.lastIndexOf('/')+1):'';const path=base+ref.replace(/^\.\//,'');if(!(path in files))throw Error(`Дизайн-файл не найден: ${path}`);designFiles.push(path);const design=parse(files[path]);if(Object.keys(design.overrides).length||design.states.length){validateDesign(next,design);for(const key of Object.keys(design.overrides)){if(Object.hasOwn(next.overrides,key))throw Error(`Повторный дизайн-key ${key}`);Object.defineProperty(next.overrides,key,{value:design.overrides[key],enumerable:true});}for(const state of design.states){if(states.some(other=>other.name===state.name))throw Error(`Повторное состояние ${state.name}`);states.push({...state,file:path});}}}compiled=next;compiled.states=states;scenario=Math.min(scenario,Math.max(0,states.length-1));if(reset){const defState=Object.fromEntries(Object.entries(next.stateDefinitions??{}).filter(([,d])=>d.default!==undefined).map(([k,d])=>[k,resolve(d.default,{})]));state={...defState,...structuredClone(states[scenario]?.state??{})};}$('scenario').innerHTML=(states.length?'<option value="-1">Базовые значения</option>':'')+states.map((s,i)=>`<option value="${i}">${esc(s.name)}</option>`).join('')||'<option>Без состояний</option>';$('scenario').value=scenario;$('preview-name').textContent=next.name;error='';render();syncHotRunningApp();$('status').textContent='Live preview обновлён';}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;$('status').textContent='Ошибка · сохранён последний предпросмотр';}refreshControlTree();refreshStatesPanel();output();}
function render(designMode=mode==='design'){
if(!compiled)return;
if(renderer==='vector'){
  if(!vectorPreview)throw Error('Загрузка векторного Rust/WASM…');
  if(!files['components/Button.ui'])throw Error('Добавьте components/Button.ui с примитивами кнопки');
  const activeStateName=scenario<0?null:compiled.states[scenario]?.name??null;
  const linked=compileComponents(files,entry,state,{measureText:vectorPreview.measureText,designMode,designState:activeStateName,instanceProps:designMode?n=>designPreset(designPresetName,n):undefined,transformScene:designMode?scene=>collectionScenario(designPresetName,scene,files):undefined});
  lastVisuals=linked.visualNodes??[];lastPreviewControls=linked.previewControls??[];
  const ok=vectorPreview.render({container:$('preview'),...linked,designMode,nodes:designMode&&designPresetName!=='original'?linked.previewNodes:compiled.nodes,selectedStart:selectedPath===entry?selected?.start:null});
  if(!ok)throw Error(error||'Ошибка векторного компонента');
  $('preview').style.width=vectorPreview.layoutSnapshot().width+'px';$('preview').style.minHeight='0';$('preview').style.overflow='visible';
  $('caption').textContent=designMode?'Rust/WASM · компонент из примитивов · щёлкните для выбора':'Rust/WASM · hover / pressed + transitions · события в журнале';
  if(designMode&&designPresetName!=='original')$('caption').textContent='Дизайнерский сценарий · исходник не изменён · редактирование частей отключено';
  $('canvas').classList.toggle('interacting',!designMode);canvasTools?.update();layoutInspector?.update();elementTools?.update();return;
}
renderHtmlPreview({preview:$('preview'),compiled,state,interactive:mode!=='design',design:designMode?designStatePatch(compiled,compiled.states[scenario]?.name):null,onSelect:select,onEvent:dispatch,onLog:log,onChange:schedulePreviewRefresh});
$('canvas').classList.toggle('interacting',mode!=='design');spacingOverlay?.update();canvasTools?.update();elementTools?.update();}
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
function select(n){selected=n;selectedPath=entry;vectorPreview?.select(n.start);layoutInspector?.update();if(active!==entry)open(entry);$('code').setSelectionRange(n.start,n.start);const line=countLines(files[entry],n.start);$('code').scrollTop=Math.max(0,(line-4)*23);$('lines').scrollTop=$('code').scrollTop;document.querySelectorAll('.ui-node').forEach(el=>el.classList.toggle('selected',Number(el.dataset.start)===n.start));inspectorNote='';renderInspector();lines();controlTree?.select(n.start,entry);}
// The mirror of `select` for the moment there is nothing to select: every surface that was drawing the
// control lets it go, so no row survives that could write into markup the file has dropped.
function clearSelection(){selected=null;selectedPath=null;vectorPreview?.select(null);elementTools?.setSelection([]);layoutInspector?.update();document.querySelectorAll('.ui-node.selected').forEach(el=>el.classList.remove('selected'));inspectorNote='';renderInspector();lines();controlTree?.select(null,null);}
function refreshControlTree(){
  const isUi=workspaceEntries.has('')?!!selectedTreeItem?.endsWith('.ui'):!!active?.endsWith('.ui');
  document.body.classList.toggle('is-ui-file',isUi);
  if(!controlTree)return;
  const path=active?.endsWith('.ui')?active:(controlTree.scope==='designer'?entry:active);
  let doc=null,message='',stale=false;
  if(!path?.endsWith('.ui'))message='Откройте файл .ui';
  else try{const source=files[path],previous=studioCache.get('tree:'+path)??(activeTreeDocument?.path===path?activeTreeDocument:null);doc=previous?.source===source?previous.document:parse(source);if(previous?.document!==doc)studioCache.set('tree:'+path,{source,document:doc});activeTreeDocument={path,source,document:doc};if(controlTree.scope==='designer'&&error)message=error;}
  catch(e){doc=(activeTreeDocument?.path===path?activeTreeDocument.document:studioCache.get('tree:'+path)?.document)??null;message=e.message;stale=!!doc;}
  controlTree.update({document:doc,path,error:message,stale,selectedStart:selected?.start,selectedPath:selectedPath===entry&&path===active?active:selectedPath,files});
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
    const source=liveSource(path);
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
  return {path,source:liveSource(path)};
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
  if(item.path===entry||files[item.path]===files[entry]){
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
function dispatch(action,n,event='clicked'){if(pending){log('Событие пропущено: debugger приостановлен');return;}const args=(n.eventArgExpressions?.[event]??n.eventArgs?.[event]??[]).map(value=>evaluate(value,compiled.defaults,state,[],false,n.environment));log(`Событие ${n.type}.${event} → ${action}(${args.map(value=>JSON.stringify(value)).join(', ')})`);if(breakOn){pending={action,n,args};$('continue').disabled=false;$('debug-status').textContent='Пауза перед '+action;tab='state';syncTabs();output();return;}execute(action,args);}
function execute(action,args=[]){if(action==='actions.search'){state.status=`Дизайн-обработчик: запрос «${state.query||'пусто'}»`;log('Выполнен дизайн-обработчик search (без Rust/backend)');}else{const defState=Object.fromEntries(Object.entries(compiled?.stateDefinitions??{}).filter(([,d])=>d.default!==undefined).map(([k,d])=>[k,resolve(d.default,{})]));const res=executePreviewStateAction(state,action,args,defState);if(res.handled)log(res.log);else log(`Обработчик ${action} не подключён`);}render();output();}
function syncTabs(){document.querySelectorAll('[data-tab]').forEach(b=>b.classList.toggle('chosen',b.dataset.tab===tab));}
function formatActiveFile(){if(!active?.endsWith('.ui'))return;try{const current=$('code').value,formatted=formatSource(current);if(formatted&&formatted!==current){codeEditor.edit({from:0,to:current.length,insert:formatted});$('status').textContent='Код отформатирован';}}catch(e){reportError(e.message);}}
$('format-code').onclick=formatActiveFile;
$('code').oninput=()=>{files[active]=$('code').value;lines();saveStatus('changed','Изменено');clearTimeout(saveTimer);saveTimer=setTimeout(persist,350);clearTimeout(compileTimer);compileTimer=setTimeout(()=>compile(active.endsWith('.design.ui')),250);};$('code').onscroll=()=>{$('lines').scrollTop=$('code').scrollTop;};$('code').onclick=lines;$('code').onkeyup=lines;
$('code').onkeydown=e=>{if(e.key==='Tab'){e.preventDefault();const el=e.target;el.setRangeText('    ',el.selectionStart,el.selectionEnd,'end');el.dispatchEvent(new Event('input'));}if((e.metaKey||e.ctrlKey)&&e.key==='s'){e.preventDefault();persist();}if(e.shiftKey&&e.altKey&&e.key.toLowerCase()==='f'){e.preventDefault();formatActiveFile();}};
$('scenario').onchange=e=>{scenario=Number(e.target.value);compile(true);};$('entry').onclick=()=>{if(renderer==='vector'&&active==='components/Button.ui'){compile();return;}entry=active;scenario=0;compile(true);};
$('run').onclick=()=>{mode=mode==='design'?'interact':'design';writeStorage('sessionStorage',storageKey('forma-canvas-mode'),mode);studioEvents.emit(studioEventTypes.mode,{mode});$('run').textContent=mode==='design'?'Взаимодействие':'Дизайн';$('caption').textContent=mode==='design'?'Выберите элемент, чтобы найти его в разметке':'События выполняются в дизайн-runtime · Rust не подключён';render();};
$('desktop').onclick=()=>{$('preview').style.width='520px';$('desktop').classList.add('chosen');$('mobile').classList.remove('chosen');};$('mobile').onclick=()=>{$('preview').style.width='320px';$('mobile').classList.add('chosen');$('desktop').classList.remove('chosen');};$('theme').onclick=()=>$('preview').classList.toggle('light');
$('break').onchange=e=>breakOn=e.target.checked;$('continue').onclick=()=>{if(pending){const p=pending;pending=null;$('continue').disabled=true;$('debug-status').textContent='Готов';execute(p.action,p.args??[]);}};$('reset').onclick=()=>{pending=null;$('continue').disabled=true;$('debug-status').textContent='Готов';compile(true);log('Состояние предпросмотра восстановлено');};
document.querySelectorAll('[data-tab]').forEach(b=>b.onclick=()=>{tab=b.dataset.tab;syncTabs();output();});
let treeContextMenu=null;
function closeTreeContextMenu(){if(treeContextMenu)treeContextMenu.hidden=true;}
function startTreeInlineInput({mode:inputMode,targetPath='',dir=''}){
 closeTreeContextMenu();
 const existing=$('tree').querySelector('.tree-inline-row');if(existing)existing.remove();
 const row=document.createElement('div');row.className='tree-inline-row';
 const input=document.createElement('input');input.type='text';input.className='tree-inline-input';input.spellcheck=false;
 const initialName=inputMode==='rename'?targetPath.split('/').at(-1):inputMode==='new-folder'?'new-folder':'NewComponent.ui';
 input.value=initialName;
 row.append(input);
 let anchorBtn=null,indent=0;
 if(inputMode==='rename'){
  anchorBtn=$('tree').querySelector(`.file[data-path="${CSS.escape(targetPath)}"],.file[data-folder="${CSS.escape(targetPath)}"]`);
  if(anchorBtn){
   const lvl=Number(anchorBtn.getAttribute('aria-level')??1);
   indent=(lvl-1)*12;
   row.style.paddingLeft=(8+indent)+'px';
   anchorBtn.after(row);
   anchorBtn.hidden=true;
  }
 }
 if(!row.isConnected){
  const folderBtn=dir?$('tree').querySelector(`.file[data-folder="${CSS.escape(dir)}"]`):null;
  const childrenBox=folderBtn?.nextElementSibling?.querySelector('.folder-children-inner');
  if(childrenBox){
   const lvl=Number(folderBtn.getAttribute('aria-level')??1);
   row.style.paddingLeft=(8+lvl*12)+'px';
   childrenBox.prepend(row);
  }else{
   row.style.paddingLeft='8px';
   $('tree').prepend(row);
  }
 }
 input.focus();
 const dotIdx=initialName.lastIndexOf('.');
 if(dotIdx>0&&inputMode!=='new-folder')input.setSelectionRange(0,dotIdx);else input.select();
 let done=false;
 const cleanup=()=>{if(anchorBtn)anchorBtn.hidden=false;row.remove();};
 const commit=async()=>{
  if(done)return;done=true;
  const val=input.value.trim();
  cleanup();
  if(!val||(inputMode==='rename'&&val===initialName))return;
  if(inputMode==='rename'){
   const parent=parentDirOf(targetPath);
   const toPath=parent?`${parent}/${val}`:val;
   if(await applyTreeRename(targetPath,toPath)){
    pushTreeHistory({
     label:`переименование «${initialName}» → «${val}»`,
     undo:()=>applyTreeRename(toPath,targetPath),
     redo:()=>applyTreeRename(targetPath,toPath),
    });
   }
  }else if(inputMode==='new-folder'){
   const rel=dir?`${dir}/${val}`:val;
   if(workspaceEntries.has('')){
    const res=await fetch(`/__forma_workspace/mkdir?path=${encodeURIComponent(rel)}`,{method:'POST'});
    const data=await res.json();
    if(!res.ok){reportError(data?.error??'Ошибка создания папки');return;}
    expandedDirs.add(rel);if(dir)expandedDirs.add(dir);
    selectedTreeItem=rel;
    await refreshWorkspaceDirs([dir,rel]);
    pushTreeHistory({
     label:`создание папки «${val}»`,
     undo:async()=>{const r=await fetch('/__forma_workspace/delete',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:rel})});if(!r.ok)return false;expandedDirs.delete(rel);await refreshWorkspaceDirs([dir]);return true;},
     redo:async()=>{const r=await fetch(`/__forma_workspace/mkdir?path=${encodeURIComponent(rel)}`,{method:'POST'});if(!r.ok)return false;expandedDirs.add(rel);await refreshWorkspaceDirs([dir,rel]);return true;},
    });
   }
  }else if(inputMode==='new-file'){
   const rel=dir?`${dir}/${val}`:val;
   const tpl=rel.endsWith('.ui')?`component ${val.replace(/\.ui$/,'').replace(/[^a-zA-Z0-9_]/g,'')||'NewWindow'} {\n    Frame {\n        Text {\n            text: 'Новое окно';\n        }\n    }\n}\n`:'';
   if(workspaceEntries.has('')){
    const res=await fetch(`/__forma_workspace/write?path=${encodeURIComponent(rel)}`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({content:tpl})});
    const data=await res.json();
    if(!res.ok){reportError(data?.error??'Ошибка создания файла');return;}
    files[rel]=tpl;
    if(dir)expandedDirs.add(dir);
    await refreshWorkspaceDirs([dir]);
    await activateTreeFile(rel);
    pushTreeHistory({
     label:`создание файла «${val}»`,
     undo:async()=>{const r=await fetch('/__forma_workspace/delete',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:rel})});if(!r.ok)return false;delete files[rel];await refreshWorkspaceDirs([dir]);return true;},
     redo:async()=>{const r=await fetch(`/__forma_workspace/write?path=${encodeURIComponent(rel)}`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({content:tpl})});if(!r.ok)return false;files[rel]=tpl;await refreshWorkspaceDirs([dir]);return true;},
    });
   }else{
    files[rel]=tpl;open(rel);persist();
   }
  }
 };
 input.onkeydown=e=>{
  if(e.key==='Enter'){e.preventDefault();commit();}
  else if(e.key==='Escape'){e.preventDefault();done=true;cleanup();$('tree').focus();}
 };
 input.onblur=()=>commit();
}
async function createInTree(kind='file',baseDir=selectedTargetDir()){
 if(baseDir&&!expandedDirs.has(baseDir)){
  expandedDirs.add(baseDir);
  if(workspaceEntries.has('')&&!workspaceEntries.has(baseDir))await loadWorkspaceDir(baseDir);
  tree(true);
 }
 startTreeInlineInput({mode:kind==='folder'?'new-folder':'new-file',dir:baseDir});
}
async function duplicateTreeItem(targetPath=selectedTreeItem){
 if(!targetPath)return;
 if(workspaceEntries.has('')){
  const res=await fetch('/__forma_workspace/duplicate',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:targetPath})});
  const data=await res.json();
  if(!res.ok){reportError(data?.error??'Ошибка дублирования');return;}
  const dupPath=data.path;
  selectedTreeItem=dupPath;
  await refreshWorkspaceDirs([parentDirOf(targetPath)]);
  if(!dupPath.endsWith('/'))await activateTreeFile(dupPath);
  pushTreeHistory({
   label:`дублирование «${targetPath.split('/').at(-1)}»`,
   undo:async()=>{const r=await fetch('/__forma_workspace/delete',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:dupPath})});if(!r.ok)return false;delete files[dupPath];await refreshWorkspaceDirs([parentDirOf(targetPath)]);return true;},
   redo:async()=>{const r=await fetch('/__forma_workspace/duplicate',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:targetPath})});if(!r.ok)return false;await refreshWorkspaceDirs([parentDirOf(targetPath)]);return true;},
  });
 }else if(targetPath in files){
  const ext=targetPath.includes('.')?targetPath.slice(targetPath.lastIndexOf('.')):'';
  const stem=ext?targetPath.slice(0,-ext.length):targetPath;
  const copyPath=`${stem}-copy${ext}`;
  files[copyPath]=files[targetPath];open(copyPath);persist();
 }
}
async function deleteTreeItem(targetPath=selectedTreeItem){
 if(!targetPath)return;
 const name=targetPath.split('/').at(-1);
 if(!confirm(`Удалить «${name}»?`))return;
 if(workspaceEntries.has('')){
  const res=await fetch('/__forma_workspace/delete',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:targetPath})});
  const data=await res.json();
  if(!res.ok){reportError(data?.error??'Ошибка удаления');return;}
  delete files[targetPath];
  expandedDirs.delete(targetPath);
  const parent=parentDirOf(targetPath);
  selectedTreeItem=parent;
  await refreshWorkspaceDirs([parent]);
 }else if(targetPath in files){
  delete files[targetPath];
  const nextFile=Object.keys(files)[0];
  if(nextFile)open(nextFile);else tree(true);
  persist();
 }
}
async function locateActiveInTree(){
 if(!active)return;
 const parts=active.split('/');
 if(parts.length>1){
  let cur='';
  for(let i=0;i<parts.length-1;i++){
   cur=cur?`${cur}/${parts[i]}`:parts[i];
   expandedDirs.add(cur);
   if(workspaceEntries.has('')&&!workspaceEntries.has(cur))await loadWorkspaceDir(cur);
  }
  tree(true);
 }
 selectTreeRow(active);
 const btn=$('tree').querySelector(`.file[data-path="${CSS.escape(active)}"]`);
 btn?.scrollIntoView({block:'nearest',behavior:'smooth'});
}
$('new').onclick=()=>createInTree('file');
if($('new-folder'))$('new-folder').onclick=()=>createInTree('folder');
if($('tree-locate'))$('tree-locate').onclick=()=>locateActiveInTree();
if($('tree-collapse-all'))$('tree-collapse-all').onclick=()=>{expandedDirs.clear();tree(true);};
const treeSearchBar=$('tree-search-bar'),treeSearchInput=$('tree-search-input'),treeSearchToggle=$('tree-search-toggle'),treeSearchClear=$('tree-search-clear');
function toggleTreeSearch(forceOpen){
 if(!treeSearchBar)return;
 const open=forceOpen!==undefined?forceOpen:treeSearchBar.hidden;
 treeSearchBar.hidden=!open;
 treeSearchToggle?.classList.toggle('active',open);
 if(open){treeSearchInput?.focus();treeSearchInput?.select();}
 else if(treeFilter){treeFilter='';if(treeSearchInput)treeSearchInput.value='';tree(true);}
}
if(treeSearchToggle)treeSearchToggle.onclick=()=>toggleTreeSearch();
if(treeSearchInput){
 treeSearchInput.oninput=()=>{treeFilter=treeSearchInput.value.trim();tree(true);};
 treeSearchInput.onkeydown=e=>{if(e.key==='Escape'){e.stopPropagation();toggleTreeSearch(false);$('tree').focus();}};
}
if(treeSearchClear)treeSearchClear.onclick=()=>{if(treeSearchInput)treeSearchInput.value='';treeFilter='';tree(true);treeSearchInput?.focus();};
$('tree').ondragover=e=>{
 if(!draggedTreePath||e.target.closest('.file'))return;
 e.preventDefault();e.dataTransfer.dropEffect='move';
 $('tree').querySelectorAll('.drop-target').forEach(el=>el.classList.remove('drop-target'));
 $('tree').classList.add('drop-root');
};
$('tree').ondragleave=e=>{if(e.target===$('tree'))$('tree').classList.remove('drop-root');};
$('tree').ondrop=async e=>{
 if(!draggedTreePath||e.target.closest('.file'))return;
 e.preventDefault();
 $('tree').classList.remove('drop-root');
 await moveTreeItem(draggedTreePath,'');
};
$('tree').oncontextmenu=e=>{
 e.preventDefault();
 const btn=e.target.closest('.file');
 const itemPath=btn?(btn.dataset.path??btn.dataset.folder):null;
 const isFolder=!!btn?.dataset.folder;
 if(itemPath)selectTreeRow(itemPath);
 const targetDir=itemPath?(isFolder?itemPath:parentDirOf(itemPath)):'';
 if(!treeContextMenu){
  treeContextMenu=document.createElement('div');
  treeContextMenu.className='tree-context-menu';
  treeContextMenu.hidden=true;
  document.body.append(treeContextMenu);
 }
 treeContextMenu.innerHTML=`
  <button type="button" class="tree-context-item" data-cmd="new-file"><span>Новый файл…</span><span class="tree-context-kbd">N</span></button>
  <button type="button" class="tree-context-item" data-cmd="new-folder"><span>Новая папка…</span><span class="tree-context-kbd">⇧N</span></button>
  <div class="tree-context-sep"></div>
  <button type="button" class="tree-context-item" data-cmd="rename" ${itemPath?'':'disabled'}><span>Переименовать</span><span class="tree-context-kbd">F2</span></button>
  <button type="button" class="tree-context-item" data-cmd="duplicate" ${itemPath?'':'disabled'}><span>Дублировать</span><span class="tree-context-kbd">⌘D</span></button>
  <button type="button" class="tree-context-item" data-cmd="copy-path" ${itemPath?'':'disabled'}><span>Скопировать путь</span><span class="tree-context-kbd">⌥⌘C</span></button>
  <div class="tree-context-sep"></div>
  <button type="button" class="tree-context-item" data-cmd="collapse-all"><span>Свернуть все папки</span></button>
  <div class="tree-context-sep"></div>
  <button type="button" class="tree-context-item danger" data-cmd="delete" ${itemPath?'':'disabled'}><span>Удалить</span><span class="tree-context-kbd">⌫</span></button>
 `;
 treeContextMenu.hidden=false;
 const x=Math.min(e.clientX,window.innerWidth-220),y=Math.min(e.clientY,window.innerHeight-260);
 treeContextMenu.style.left=x+'px';treeContextMenu.style.top=y+'px';
 treeContextMenu.querySelectorAll('[data-cmd]').forEach(el=>el.onclick=async()=>{
  const cmd=el.dataset.cmd;
  closeTreeContextMenu();
  if(cmd==='new-file')await createInTree('file',targetDir);
  else if(cmd==='new-folder')await createInTree('folder',targetDir);
  else if(cmd==='rename'&&itemPath)startTreeInlineInput({mode:'rename',targetPath:itemPath});
  else if(cmd==='duplicate'&&itemPath)await duplicateTreeItem(itemPath);
  else if(cmd==='copy-path'&&itemPath){try{await navigator.clipboard.writeText(itemPath);$('status').textContent=`Скопировано: ${itemPath}`;}catch{}}
  else if(cmd==='collapse-all'){expandedDirs.clear();tree(true);}
  else if(cmd==='delete'&&itemPath)await deleteTreeItem(itemPath);
 });
};
$('tree').onkeydown=async e=>{
 if(e.target.matches('input,textarea'))return;
 const visibleRows=[...$('tree').querySelectorAll('.file')].filter(b=>!b.hidden&&b.offsetParent!==null);
 if(!visibleRows.length)return;
 const idx=visibleRows.findIndex(b=>(b.dataset.path??b.dataset.folder)===selectedTreeItem);
 const curBtn=idx>=0?visibleRows[idx]:visibleRows[0];
 const curPath=curBtn?(curBtn.dataset.path??curBtn.dataset.folder):null;
 if(e.key==='ArrowDown'){
  e.preventDefault();
  const next=visibleRows[Math.min(visibleRows.length-1,idx+1)]??visibleRows[0];
  const p=next.dataset.path??next.dataset.folder;
  selectTreeRow(p);next.scrollIntoView({block:'nearest'});
 }else if(e.key==='ArrowUp'){
  e.preventDefault();
  const prev=visibleRows[Math.max(0,idx-1)]??visibleRows[0];
  const p=prev.dataset.path??prev.dataset.folder;
  selectTreeRow(p);prev.scrollIntoView({block:'nearest'});
 }else if(e.key==='ArrowRight'&&curBtn?.dataset.folder){
  e.preventDefault();
  if(!expandedDirs.has(curBtn.dataset.folder))await toggleFolder(curBtn.dataset.folder,curBtn);
  else if(visibleRows[idx+1]){const p=visibleRows[idx+1].dataset.path??visibleRows[idx+1].dataset.folder;selectTreeRow(p);}
 }else if(e.key==='ArrowLeft'&&curPath){
  e.preventDefault();
  if(curBtn?.dataset.folder&&expandedDirs.has(curBtn.dataset.folder))await toggleFolder(curBtn.dataset.folder,curBtn);
  else{const parent=parentDirOf(curPath);if(parent)selectTreeRow(parent);}
 }else if(e.key==='Enter'&&curBtn){
  e.preventDefault();
  if(curBtn.dataset.folder)await toggleFolder(curBtn.dataset.folder,curBtn);
  else if(curBtn.dataset.path)await activateTreeFile(curBtn.dataset.path);
 }else if(e.key==='F2'&&curPath){
  e.preventDefault();
  startTreeInlineInput({mode:'rename',targetPath:curPath});
 }else if((e.key==='Delete'||e.key==='Backspace')&&curPath){
  e.preventDefault();
  await deleteTreeItem(curPath);
 }else if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='f'){
  e.preventDefault();
  toggleTreeSearch(true);
 }
};
$('open-folder').onclick=async()=>{if(typeof window.showDirectoryPicker!=='function'){alert('Браузер не поддерживает File System Access API (showDirectoryPicker). Используйте Открыть / Экспорт JSON.');return;}try{const handle=await window.showDirectoryPicker({mode:'readwrite'});const data=await readDirectoryProject(handle);resetProjectEditing();directoryHandle=handle;directorySavedFiles={...data};files=data;workspaceEntries.clear();expandedDirs.clear();if($('project-switcher-label'))$('project-switcher-label').textContent=handle.name;const titleNode=document.querySelector('.project-title')?.childNodes?.[0];if(titleNode?.nodeType===3)titleNode.textContent=handle.name+' ';active=Object.keys(files)[0];entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui'));scenario=0;open(active);persist();compile(true);}catch(e){if(e?.name!=='AbortError')alert(e.message);}};
const projectSwitcher=$('project-switcher'),projectSwitcherMenu=$('project-switcher-menu'),projectSwitcherWrap=projectSwitcher?.parentElement;
function closeProjectMenu(){if(!projectSwitcherMenu||projectSwitcherMenu.hidden)return;projectSwitcherMenu.hidden=true;projectSwitcher?.setAttribute('aria-expanded','false');}
if(projectSwitcher&&projectSwitcherMenu){
 projectSwitcher.onclick=e=>{e.stopPropagation();const open=projectSwitcherMenu.hidden;projectSwitcherMenu.hidden=!open;projectSwitcher.setAttribute('aria-expanded',String(open));};
 $('project-switcher-open-folder').onclick=()=>{closeProjectMenu();$('open-folder').click();};
}
$('export').onclick=()=>downloadJson('forma-project.json',JSON.stringify(files,null,2));
$('export-native').onclick=async()=>{try{const bundle=buildNativeRustExport(files,entry,{name:isStudioControls?'forma-studio-controls':'forma-workspace-app'});if(directoryHandle){await writeDirectoryProject(directoryHandle,bundle.files,directorySavedFiles);directorySavedFiles={...bundle.files};files={...bundle.files};tree();saveStatus('saved',`Rust-крейт записан (${directoryHandle.name})`);}else{downloadJson(`${bundle.name}-rust-crate.json`,JSON.stringify(bundle.files,null,2));$('status').textContent=`Экспортирован нативный Rust-проект (${bundle.entry})`;}}catch(e){reportError(e.message);}};
$('import').onclick=()=>$('upload').click();$('upload').onchange=async e=>{try{const f=e.target.files[0];if(!f)return;const data=validateProject(JSON.parse(await f.text()));if(!confirm('Заменить текущий проект? При необходимости сначала экспортируйте его.'))return;resetProjectEditing();directoryHandle=null;directorySavedFiles={};files=data;active=Object.keys(files)[0];entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui'));scenario=0;open(active);persist();compile(true);}catch(e){alert(e.message);}finally{e.target.value='';}};
const codeEvents=createEventScope();
codeEvents.listen(document,'pointerdown',e=>{if(projectSwitcherWrap&&!projectSwitcherWrap.contains(e.target))closeProjectMenu();if(treeContextMenu&&!treeContextMenu.contains(e.target))closeTreeContextMenu();});
codeEvents.listen(document,'keydown',e=>{
 if(e.key==='Escape'){closeProjectMenu();closeTreeContextMenu();return;}
 if((e.metaKey||e.ctrlKey)&&!e.altKey&&!e.target.closest('.cm-editor,input,textarea,#canvas,#preview')){
  const isZ=e.code==='KeyZ'||e.key.toLowerCase()==='z'||e.key.toLowerCase()==='я';
  const isY=e.code==='KeyY'||e.key.toLowerCase()==='y'||e.key.toLowerCase()==='н';
  if(isZ&&!e.shiftKey&&treeUndoStack.length){e.preventDefault();undoTreeOperation();}
  else if(((isZ&&e.shiftKey)||isY)&&treeRedoStack.length){e.preventDefault();redoTreeOperation();}
 }
});
codeEvents.listen($('code'),'scroll',highlightSource);
codeEvents.listen($('code'),'input',()=>{selected=null;selectedPath=null;controlTree?.select(null,null);refreshControlTree();highlightSource();document.querySelectorAll('.ui-node.selected').forEach(el=>el.classList.remove('selected'));});
controlTree=createControlTree({explorer:document.querySelector('.explorer'),toolbar:document.querySelector('.preview-tools'),onSelect:selectTreeControl,onScopeChange:refreshControlTree,onOpenTemplate:path=>{open(path);controlTree.setView({scope:'file',visible:true});},onExtract:extractTreeControl,canReorder:()=>designPresetName==='original',onReorder:reorderTreeControl});
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
  if(liveSource(file)!==source)throw Error('Исходник изменился — повторите операцию');
  parse(source.slice(0,from)+insert+source.slice(to));
  if(active!==file)open(file);
  codeEditor.edit({from,to,insert});
  // A change that adds or removes a row rebuilds its panel once the editor has published the
  // input event and the new AST exists. `after` runs at the very end, after the host has republished
  // the reselected control: the editor's own input event drops the selection, so a caller that keeps
  // a batch of offsets — the ones this change just moved — has to restore it over that handoff.
  if(reselect!==undefined)queueMicrotask(()=>{clearTimeout(compileTimer);compile();const found=findNodeByStart(compiled?.nodes,reselect);if(found)select(found);after?.();});
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
async function extractTreeControl({path,start,node}){
  const source=liveSource(path)??(activeTreeDocument?.path===path?activeTreeDocument.source:files[path]);
  if(source===undefined){$('caption').textContent='Файл не найден';return;}
  const rawKey=typeof node?.props?.key==='string'?node.props.key.replace(/[^A-Za-z0-9_]/g,''):'';
  const suggestedBase=rawKey?rawKey[0].toUpperCase()+rawKey.slice(1):`Custom${node?.type||'Component'}`;
  let suggested=suggestedBase,idx=2;
  while(files[`components/${suggested}.ui`])suggested=`${suggestedBase}${idx++}`;
  const input=prompt('Имя нового компонента (.ui в папке components/):',suggested);
  if(!input)return;
  const name=input.trim().replace(/\.ui$/i,'');
  try{
    const compPath=`components/${name}.ui`;
    if(files[compPath]&&!confirm(`Файл ${compPath} уже существует. Перезаписать?`))return;
    const change=extractElement(source,start,name);
    files[change.componentPath]=change.componentSource;
    if(workspaceEntries.has('')){
      const wsCompDir=active?.includes('/')&&!active.startsWith('components/')&&!active.startsWith('ui/')?`${parentDirOf(active)}/components`:'forma-studio/components';
      const wsCompPath=`${wsCompDir}/${name}.ui`;
      try{
        await fetch(`/__forma_workspace/mkdir?path=${encodeURIComponent(wsCompDir)}`,{method:'POST'});
        await fetch(`/__forma_workspace/write?path=${encodeURIComponent(wsCompPath)}`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({content:change.componentSource})});
        files[wsCompPath]=change.componentSource;
        expandedDirs.add(wsCompDir);
        await refreshWorkspaceDirs([parentDirOf(wsCompDir),wsCompDir]);
      }catch{}
    }
    commitSource({file:path,source,from:change.from,to:change.to,insert:change.insert,reselect:path===entry?change.start:undefined});
    persist();
    tree(true);
    $('status').textContent=`Вынесен в ${change.componentPath}`;
    $('caption').textContent=`Создан компонент ${change.componentPath}`;
  }catch(e){reportError(e.message);$('caption').textContent=e.message;}
}
propertyInspector=createPropertyInspector({container:$('inspector'),designTokens:()=>designReferencesInFiles(files),
 // The inspector renders from a snapshot of the source, so both hooks below read the file as it
 // really stands: `files` only catches the editor up on the next microtask, and a committed row
 // stays on screen with the snapshot it was rendered from.
 liveSource,
 commit:commitSource,
 onError:reportError});
canvasTools=createCanvasTools({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.preview-tools'),
getScene:()=>renderer==='vector'?vectorPreview?.layoutSnapshot():null,
getSelection:()=>{if(!selected||selectedPath!==entry)return null;const children=compiled?.nodes?.[0]?.children??[];const nodes=sceneControlNodes(children);return {index:nodes.findIndex(n=>n.start===selected.start),root:selected.start===compiled?.nodes?.[0]?.start};},
getMode:()=>mode,setMode:next=>{if(mode!==next)$('run').click();},onChange:()=>{vectorPreview?.refreshScale?.();spacingOverlay?.update();layoutInspector?.update();elementTools?.update();}});
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
function domLayoutSnapshot(){
 const preview=$('preview');if(!preview)return null;
 const rootEl=preview.firstElementChild;
 const r=preview.getBoundingClientRect();
 const scale=r.width/(preview.offsetWidth||r.width||1);
 const width=preview.offsetWidth||(r.width/scale)||360,height=preview.offsetHeight||(r.height/scale)||220;
 const controls=[],containers=[];
 const containerSet=new Set(['frame','row','column','grid','stack','scroll','modal']);
 for(const el of preview.querySelectorAll('[data-start]')){
  if(el===rootEl)continue;
  const start=Number(el.dataset.start);
  const er=el.getBoundingClientRect(),pr=(el.parentElement??preview).getBoundingClientRect();
  const bounds=[(er.left-r.left)/scale,(er.top-r.top)/scale,er.width/scale,er.height/scale];
  const coordinateBox=[(pr.left-r.left)/scale,(pr.top-r.top)/scale,pr.width/scale,pr.height/scale];
  const isContainer=[...el.classList].some(c=>containerSet.has(c));
  if(isContainer)containers.push({start,bounds,coordinateBox});
  else controls.push({index:controls.length,start,bounds,coordinateBox});
 }
 return {width,height,clip:false,scrollable:false,scrollOffset:[0,0],controls,containers};
}
elementTools=createElementTools({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.canvas-tools'),
context:()=>{if(mode!=='design'||designPresetName!=='original'||error||(renderer==='vector'&&!vectorPreview))return null;const root=compiled?.nodes[0],children=root?.children??[],scene=renderer==='vector'?vectorPreview.layoutSnapshot():domLayoutSnapshot();if(!scene)return null;return {source:files[entry],path:entry,root,start:selectedPath===entry?selected?.start:null,top:sceneControlNodes(children),pick:start=>findNodeByStart(root?[root]:[],start),scene,grid:lastVisuals.find(v=>v.control===-1)?.grid,inserts:projectInserts(),visuals:lastVisuals,state};},
select,extract:extractTreeControl,report:message=>{$('caption').textContent=message;},
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
 parse(next);if(renderer==='vector'&&vectorPreview)compileComponents({...files,[entry]:next},entry,state,{measureText:vectorPreview.measureText,designMode:mode==='design',designState:scenario<0?null:compiled?.states[scenario]?.name??null});
 if(active!==entry)open(entry);codeEditor.edit({from:change.from,to:change.to,insert:change.insert});
 // The editor publishes its input event first; reselect against the new AST afterwards.
 queueMicrotask(()=>{clearTimeout(compileTimer);compile();const found=findNodeByStart(compiled?.nodes,change.start);if(found)select(found);if(change.starts)elementTools.setSelection(change.starts);elementTools.update();});
}});
// Below the rows of one control, the panel offers the names the whole canvas selection has in
// common. Their write spans the batch, so the offsets it reports are what keeps the selection alive:
// the handoff to the reselected control leaves the panel holding one control, and the batch is put
// back over it before the rows are drawn again.
bulkEditor=createBulkEditor({container:$('inspector'),
 commit:({file,source,primary,...change})=>commitSource({file,source,...change,reselect:primary,after:()=>{elementTools.setSelection(change.starts);renderInspector();}}),
 onError:reportError});
spacingOverlay=createSpacingOverlay($('canvas'),()=>mode==='design'&&selectedPath===entry?selected:null);
open(active);compile(true);
let studioShell,shellDisposed=false;
studioEvents.emit(studioEventTypes.mode,{mode});
const minimalShell=mountMinimalShell(document.querySelector('#app'),{events:studioEvents,newFile:()=>$('new').click(),clearSelection:()=>{clearSelection();controlTree?.select(null,null);},inspector:propertyInspector});
mountStudioShell(document.querySelector('#app')).then(shell=>{if(shellDisposed)shell.destroy();else studioShell=shell;}).catch(e=>log('Контролы Studio: '+e.message));
import.meta.hot?.dispose(()=>{shellDisposed=true;vectorLoads.cancel();codeEvents.dispose();treeContextMenu?.remove();controlTree?.destroy();minimalShell.destroy();clearTimeout(saveTimer);clearTimeout(compileTimer);studioShell?.destroy();elementTools?.destroy();canvasTools?.destroy();layoutInspector?.destroy();spacingOverlay?.destroy();vectorPreview?.destroy();});
const backendPicker=$('app-backend'),platformPicker=$('app-platform');
if(backendPicker){backendPicker.value=appBackend;backendPicker.onchange=()=>{appBackend=backendPicker.value;writeStorage('localStorage',storageKey('forma-backend'),appBackend);import.meta.hot?.send('forma:backend',{backend:appBackend});};}
if(platformPicker){platformPicker.value=appPlatform;platformPicker.onchange=()=>{appPlatform=platformPicker.value;writeStorage('localStorage',storageKey('forma-platform'),appPlatform);import.meta.hot?.send('forma:platform',{platform:appPlatform});};}
const rendererPicker=document.createElement('select');rendererPicker.id='renderer';rendererPicker.setAttribute('aria-label','Рендерер предпросмотра');rendererPicker.innerHTML='<option value="html">HTML · прежний</option><option value="vector">Вектор · Rust/WASM</option>';$('scenario').before(rendererPicker);rendererPicker.value=renderer;
rendererPicker.onchange=()=>{renderer=rendererPicker.value;writeStorage('localStorage',storageKey('forma-renderer'),renderer);selected=null;vectorPreview?.destroy();vectorPreview=undefined;initVector();compile();};
// Each load gets a generation; a load that finishes after dispose or after a newer load builds
// nothing, so it cannot leave behind a preview (WASM runtime, GPU device, listeners) no one destroys.
const vectorLoads=createLatestOnly();
function initVector(){
 const current=vectorLoads.begin();
 loadVectorRuntime().then(runtime=>{
  if(shellDisposed||!current()||vectorPreview)return;
  vectorPreview=createVectorPreview({runtime,onSelect:n=>{if(designPresetName==='original')select(n);},onError:message=>{error=String(message);output();},onAction:(action,n)=>{if(n)dispatch(action,n);},onControlKey:e=>{if(!compiled?.shortcuts?.length)return false;for(const s of compiled.shortcuts)if(matchPreviewShortcut(e,s.key)){const stateDefaults=Object.fromEntries(Object.entries(compiled.stateDefinitions??{}).filter(([,d])=>d.default!==undefined).map(([k,d])=>[k,d.default]));const previewEnv={enums:{...compiled.enums,...compiled.structs},tokens:compiled.tokens,i18n:compiled.i18n,styles:compiled.styles,stateDefaults,allowMissingState:true};const syntheticNode={type:'Shortcut',start:s.from??0,props:{},events:{shortcut:s.action},eventArgs:{shortcut:(s.args??[]).map(a=>resolve(a,state))},eventArgExpressions:{shortcut:s.args??[]},environment:previewEnv};dispatch(s.action,syntheticNode,'shortcut');return true;}return false;},onBindingChange:changes=>{for(const {node,path,value}of changes)if(writePreviewBinding(state,node,path,value)){log(`${path} = ${JSON.stringify(value)}`);if(node.events.changed)dispatch(node.events.changed,node,'changed');}schedulePreviewRefresh();}});
  if(renderer==='vector')compile();
 }).catch(e=>{if(renderer==='vector'){error='Векторный WASM не собран: '+e.message;output();}});
}
initVector();

// The MCP transport drives the editor through one facade: live state by getter and setter,
// the panels a command addresses, and the operations the editor already exposes to its own UI.
const ideCommand=createIdeCommands({
  $,hot:import.meta.hot,compileComponents,
  get files(){return files;},set files(v){files=v;},
  get active(){return active;},set active(v){active=v;},
  get entry(){return entry;},set entry(v){entry=v;},
  get scenario(){return scenario;},set scenario(v){scenario=v;},
  get selected(){return selected;},set selected(v){selected=v;},
  get logs(){return logs;},set logs(v){logs=v;},
  get nativeSource(){return nativeSource;},set nativeSource(v){nativeSource=v;},
  get compiled(){return compiled;},get state(){return state;},get error(){return error;},get mode(){return mode;},
  get renderer(){return renderer;},get backend(){return appBackend;},set backend(v){if(['egui','svet','ask'].includes(v)){appBackend=v;if(backendPicker)backendPicker.value=v;writeStorage('localStorage',storageKey('forma-backend'),v);import.meta.hot?.send('forma:backend',{backend:v});}},get platform(){return appPlatform;},set platform(v){if(['desktop','ios','android'].includes(v)){appPlatform=v;if(platformPicker)platformPicker.value=v;writeStorage('localStorage',storageKey('forma-platform'),v);import.meta.hot?.send('forma:platform',{platform:v});}},get breakOn(){return breakOn;},get pending(){return pending;},
  get vectorPreview(){return vectorPreview;},get controlTree(){return controlTree;},get codeEditor(){return codeEditor;},
  get spacingOverlay(){return spacingOverlay;},get rendererPicker(){return rendererPicker;},
  open,persist,compile,render,output,select,commitSource,designStateFile,designStateEdit,resetProjectEditing,dispatch,
  liveSource,
  propertyInput:name=>Array.from(document.querySelectorAll('[data-prop]')).find(el=>el.dataset.prop===name),
});
if(import.meta.hot)mountDevBridge(import.meta.hot,{
  anchor:$('run'),log,setDesignData,designReferences,ideCommand,showNativeInspection,
  get compiled(){return compiled;},get files(){return files;},
  showEvents(){tab='events';syncTabs();output();},
  setError(message){error=message;output();},
  compile:()=>compile(),persist,
});

