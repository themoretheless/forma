import {studioControlsProject} from './studio-controls-builtin.js';
import {mountStudioShell} from './studio-shell.js';
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
import {createControlTree} from './control-tree.js';
import {mountEditor} from './editor.js';
import {propertyEdit} from './property-edit.js';
import {createSpacingOverlay} from './spacing-overlay.js';
import {gridProperties,gridStyles} from './grid.js';
import {sizeProperties,sizeValue} from './sizing.js';
import {nativeSnapshot} from './native-snapshot.js';
import {loadVectorRuntime,createVectorPreview} from './vector-preview.js';
import {createComponentCompiler} from './components.js';
import {expandStructure,evaluate} from './component-semantics.js';
import {writePreviewBinding} from './preview-state.js';
let previewRefreshPending=false,lastPreviewControls=[];
function schedulePreviewRefresh(){if(previewRefreshPending)return;previewRefreshPending=true;queueMicrotask(()=>{previewRefreshPending=false;try{render();error='';}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;}output();});}
import {createCacheBudget} from './cache-budget.js';
const studioCache=createCacheBudget();
const compileComponents=createComponentCompiler({cache:studioCache});
import {setDesignData,designReferences} from './design-data.js';
let codeEditor;
let spacingOverlay;
let vectorPreview;
let controlTree,selectedPath=null;
// The active tree is live UI state, independent of eviction of inactive files.
let activeTreeDocument=null;
let renderer=isStudioControls||localStorage.getItem(storageKey('forma-renderer'))==='vector'?'vector':'html';
import {parse,resolve,validateDesign} from './language.js';

const initial={
 'ui/SearchWindow.ui':`#[design('./SearchWindow.design.ui')]
component SearchWindow {
    Frame {
        padding: 32;
        gap: 16;

        Text {
            text: 'Библиотека знаний';
            font.size: 26;
            color: #e8edf7;
        }

        Text {
            text: 'Найдите ответ в ваших документах';
            color: #98a4ba;
        }

        TextInput {
            key: 'query';
            value <-> state.query;
            placeholder: 'Что найти?';
            padding: 12;
        }

        Button {
            key: 'searchButton';
            style: primary;
            text: 'Найти документы';
            disabled: state.loading;
            clicked -> actions.search();
        }

        Text {
            key: 'status';
            text: state.status;
            color: '#98a4ba';
        }
    }
}`,
 'ui/SearchWindow.design.ui':`design SearchWindow {
    TextInput { key: 'query'; value: ''; }
    Button { key: 'searchButton'; disabled: false; }
    Text { key: 'status'; text: '24 документа в библиотеке'; }
}`, 
 'src/actions.rs':`// Контракт будущего Rust-backend.
// Этот файл редактируется, но не выполняется в web-preview.

pub struct SearchState {
    pub query: String,
    pub loading: bool,
    pub status: String,
}

pub fn search(state: &mut SearchState) {
    state.loading = true;
    // Подключите прикладной сервис поиска.
}
`,
 'README.md':'# Knowledge workspace\n\n.ui — разметка.\n.design.ui — сценарии предпросмотра.\n\nОтладчик останавливается перед событием UI.\nПродолжить — применяет демонстрационный обработчик.\nRust-код требует будущей интеграции DAP.\n'
};
const initialProject=isStudioControls?studioControlsProject:initial;
let files;try{files=JSON.parse(localStorage.getItem(storageKey('forma-project')))||initialProject;}catch{files=initialProject;}
files['Cargo.toml']??=`[package]\nname = "forma-preview-app"\nversion = "0.1.0"\nedition = "2021"\n`;
files['src/main.rs']??=`mod actions;\n\nfn main() {\n    let mut state = actions::SearchState {\n        query: String::from("Архитектура"),\n        loading: false,\n        status: String::from("Готов"),\n    };\n    println!("Rust-приложение запущено");\n    println!("Запрос: {}", state.query);\n    actions::search(&mut state);\n    println!("После actions::search: loading={}, status={}", state.loading, state.status);\n}\n`;
let active=Object.keys(files)[0],entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui')), scenario=0,state={},compiled=null,selected=null,mode=sessionStorage.getItem(storageKey('forma-canvas-mode'))==='interact'?'interact':'design',pending=null,breakOn=false,logs=[],tab='problems',error='',saveTimer,compileTimer;
try{const view=JSON.parse(localStorage.getItem(storageKey('forma-view')));if(view?.entry in files)entry=view.entry;if(view?.active in files)active=view.active;}catch{}
function persistView(){try{localStorage.setItem(storageKey('forma-view'),JSON.stringify({active,entry}));}catch{}}
const $=id=>document.getElementById(id);
document.querySelector('#app').innerHTML=`
<header><div class="brand"><span class="brandmark">F</span> forma <small>STUDIO</small></div><div class="project-title">${isStudioControls?'Studio Controls':'knowledge-workspace'} <span> / локальный проект</span></div><a class="controls-link" href="${isStudioControls?'/':'/?project=studio-controls'}">${isStudioControls?'Мой проект':'Контролы Studio'}</a><a class="controls-link" href="/vector-ui/examples/controls.html">Контролы · Reveal</a><button id="import">Открыть</button><button id="export">Экспорт</button><button id="run" class="accent">${mode==='design'?'▶ Взаимодействие':'⌖ Выбор элемента'}</button></header>
<div class="workspace"><aside class="explorer"><div class="section-title">ПРОЕКТ <button id="new" title="Создать файл">+</button></div><div id="tree"></div><div class="explorer-note"><span class="dot"></span> Локальное сохранение<br><small>Проект хранится в этом браузере.<br>Экспортируйте для резервной копии.</small></div></aside>
<main><div class="tabs"><span class="file-icon">◇</span><span id="filename"></span><span id="saved">Сохранено</span><button id="entry" title="Показать этот UI в предпросмотре">Показать UI</button></div><div class="editor-preview"><section class="code-pane"><div class="pane-toolbar"><span>РАЗМЕТКА / КОД</span><span id="language">Forma UI</span></div><div class="code-wrap"><div id="lines"></div><textarea id="code" spellcheck="false" aria-label="Редактор исходного кода"></textarea></div></section><section class="preview-pane"><div class="pane-toolbar"><span>LIVE PREVIEW <i class="dot"></i></span><select id="scenario" aria-label="Дизайн-сценарий"></select></div><div class="preview-tools"><button id="desktop" class="chosen">Desktop</button><button id="mobile">Mobile</button><button id="theme">◐ Тема</button><span id="preview-name"></span></div><div id="canvas"><div id="preview"></div></div><div class="preview-caption" id="caption">Выберите элемент, чтобы найти его в разметке</div></section></div>
<section class="bottom"><div class="bottom-tabs"><button data-tab="problems" class="chosen">Диагностика <span id="problem-count">0</span></button><button data-tab="events">События <span id="event-count">0</span></button><button data-tab="state">Состояние</button><div class="debug-controls"><label><input type="checkbox" id="break"> Break on event</label><button id="continue" disabled>▶ Продолжить</button><button id="reset">↺ Сброс</button></div></div><div id="output"></div></section></main>
<aside class="inspector"><div class="section-title">ИНСПЕКТОР</div><div id="inspector"><div class="empty-icon">⌖</div><p>Выберите компонент</p><small>Свойства и привязки появятся здесь</small></div><div class="debug-info"><span>UI DEBUGGER</span><p id="debug-status">Готов</p><small>События и состояние дизайн-runtime.<br>Rust / DAP не подключён.</small></div></aside></div>
<footer><span class="dot"></span><span id="status">Готов</span><span class="footer-right">Forma UI · UTF-8 <span id="position">Ln 1, Col 1</span></span></footer><input type="file" id="upload" accept=".json" hidden>`;
const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
function persist(){try{localStorage.setItem(storageKey('forma-project'),JSON.stringify(files));$('saved').textContent='Сохранено';}catch{$('saved').textContent='Не сохранено: экспортируйте проект';}}
function tree(){const buttons=[...$('tree').querySelectorAll('[data-path]')],paths=Object.keys(files),existing=new Set(buttons.map(b=>b.dataset.path));
 if(buttons.length===paths.length&&paths.every(path=>existing.has(path))){for(const b of buttons)b.classList.toggle('active',b.dataset.path===active);return;}
 const groups={};for(const path of Object.keys(files)){const folder=path.includes('/')?path.slice(0,path.lastIndexOf('/')):'Проект';(groups[folder]??=[]).push(path);} $('tree').innerHTML=Object.entries(groups).map(([folder,paths])=>`<div class="folder">⌄ &nbsp; ${esc(folder)}</div>${paths.map(p=>`<button class="file ${p===active?'active':''}" data-path="${esc(p)}"><span class="${p.endsWith('.rs')?'rust':'ui'}">${p.endsWith('.rs')?'R':'◇'}</span>${esc(p.split('/').at(-1))}</button>`).join('')}`).join('');document.querySelectorAll('[data-path]').forEach(b=>b.onclick=()=>open(b.dataset.path));}
function open(path){active=path;persistView();codeEditor?.setLanguage(path);$('filename').textContent=path.split('/').at(-1);$('code').value=files[path];$('language').textContent=path.endsWith('.rs')?'Rust · редактирование':path.endsWith('.ui')?'Forma UI':'Текст';$('entry').disabled=!path.endsWith('.ui')||path.endsWith('.design.ui');lines();tree();refreshControlTree();}
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
function output(){$('problem-count').textContent=error?1:0;$('event-count').textContent=logs.length;$('output').innerHTML=tab==='state'?`<pre>${esc(JSON.stringify(state,null,2))}</pre>`:tab==='events'?logs.map(l=>`<div class="log"><time>${l.time}</time>${esc(l.text)}</div>`).join('')||'<p class="muted">События появятся при взаимодействии с предпросмотром.</p>':error?`<p class="error">● ${esc(error)}</p>`:'<p class="ok">✓ Разметка проверена. Ошибок нет.</p>';diagnosticLinks();}
function diagnosticLinks(){
 if(tab!=='problems'||sourceDiagnostic?.message!==error)return;
 for(const [label,source]of [['К ошибке',sourceDiagnostic.source],['К определению узла',sourceDiagnostic.related]]){
  if(!source||typeof files[source.file]!=='string')continue;
  const button=document.createElement('button'),line=files[source.file].slice(0,source.from).split('\n').length;
  button.textContent=`${label} · ${source.file}:${line}`;
  button.onclick=()=>{open(source.file);$('code').setSelectionRange(source.from,source.to);$('code').focus();};$('output').append(button);
 }
}
function compile(reset=false){persistView();try{if(!entry||!files[entry])throw Error('Откройте .ui компонент и нажмите «Показать UI»');const next=parse(files[entry]);let scenarios=[];for(const ref of next.designs){const base=entry.includes('/')?entry.slice(0,entry.lastIndexOf('/')+1):'';const path=base+ref.replace(/^\.\//,'');if(!(path in files))throw Error(`Дизайн-файл не найден: ${path}`);const design=parse(files[path]);scenarios.push(...design.scenarios);if(Object.keys(design.overrides).length){validateDesign(next,design);for(const key of Object.keys(design.overrides)){if(Object.hasOwn(next.overrides,key))throw Error(`Повторный дизайн-key ${key}`);Object.defineProperty(next.overrides,key,{value:design.overrides[key],enumerable:true});}}}compiled=next;compiled.scenarios=scenarios;scenario=Math.min(scenario,Math.max(0,scenarios.length-1));if(reset)state=structuredClone(scenarios[scenario]?.state??{});$('scenario').innerHTML=scenarios.map((s,i)=>`<option value="${i}">${esc(s.name)}</option>`).join('')||'<option>Без сценария</option>';$('scenario').value=scenario;$('preview-name').textContent=next.name;error='';render();$('status').textContent='Live preview обновлён';}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;$('status').textContent='Ошибка · сохранён последний предпросмотр';}refreshControlTree();output();}
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
for(const kind of ['padding','margin'])for(const side of ['top','right','bottom','left'])cssMap[`${kind}.${side}`]=kind+side[0].toUpperCase()+side.slice(1);
function make(source,parent=null){const override=designMode&&Object.hasOwn(compiled.overrides,source.props.key)?compiled.overrides[source.props.key]:null;const n={...source,props:{...source.props,...override}};const types={Frame:'div',Column:'div',Row:'div',Grid:'div',Stack:'div',Scroll:'div',Text:'div',TextInput:'input',TextField:'input',Checkbox:'input',Slider:'input',Button:'button',Panel:'div'};if(!types[n.type])throw Error(`Компонент ${n.type} пока не поддерживается`);const el=document.createElement(types[n.type]);el.dataset.start=n.start;el.className='ui-node '+n.type.toLowerCase();if(n.type==='Checkbox')el.type='checkbox';if(n.type==='Slider')el.type='range';if(n.type==='Stack'){el.style.display='grid';}if(n.type==='Scroll')el.style.overflow='auto';if(n.type==='Frame'||n.type==='Column'||n.type==='Row'){el.style.display='flex';el.style.flexDirection=n.type==='Row'?'row':'column';}for(const [k,v]of Object.entries(n.props)){const x=resolve(v,state);if(k==='key'){el.dataset.key=x;continue;}if(k==='clip'){if(!['Frame','Row','Column','Grid','Stack'].includes(n.type)||typeof x!=='boolean')throw Error('Frame.clip ожидает true или false');if('overflow' in n.props)throw Error('Используйте clip или overflow, не оба');el.style.overflow=x?'clip':'visible';continue;}if(gridProperties.has(k))continue;else if(sizeProperties.has(k)){if(typeof x==='string'&&/^(?:\d+(?:\.\d+)?)?\*$/.test(x)&&((parent?.type==='Row'&&k==='width')||(['Column','Frame'].includes(parent?.type)&&k==='height'))){el.style.flex=`${x==='*'?1:Number(x.slice(0,-1))} 1 0px`;el.style[k==='width'?'minWidth':'minHeight']='0';}else el.style[k]=sizeValue(x);}else if(k==='text')el.textContent=x??'';else if(k==='value')el.value=x??'';else if(k==='placeholder')el.placeholder=x;else if(k==='disabled')el.disabled=!!x;else if(k==='checked'||k==='selected')el.checked=!!x;else if(k==='style')el.classList.add(String(x));else if(cssMap[k])el.style[cssMap[k]]=typeof x==='number'&&k!=='opacity'?x+'px':Array.isArray(v)?v.map(a=>typeof a==='number'?a+'px':resolve(a,state)).join(' '):x;else throw Error(`Свойство ${k} пока не поддерживается`);}
for(const [key,path]of Object.entries(n.bindings)){if(!['value','checked','selected'].includes(key))throw Error(`Неподдерживаемая привязка ${key} <-> ${path}`);const property=key==='value'?'value':'checked';el[property]=n.props[key]??(property==='value'?'':false);el.addEventListener('input',()=>{const value=property==='checked'?el.checked:n.type==='Slider'?Number(el.value):el.value;if(writePreviewBinding(state,n,path,value,{createMissing:true})){log(`${path} = ${JSON.stringify(value)}`);if(n.events.changed)dispatch(n.events.changed,n,'changed');schedulePreviewRefresh();}});}
el.addEventListener('click',e=>{e.stopPropagation();if(mode==='design'){select(source);return;}if(n.events.clicked)dispatch(n.events.clicked,n);});Object.assign(el.style,gridStyles(n,state,parent));for(const child of n.children)el.append(make(child,n));return el;}
const focused=document.activeElement;const focusIdentity=preview.contains(focused)?{key:focused.dataset.key,start:focused.dataset.start,selectionStart:focused.selectionStart,selectionEnd:focused.selectionEnd}:null;const fragment=document.createDocumentFragment();const expanded=expandStructure(compiled.nodes,compiled.defaults,state,{enums:compiled.enums,allowMissingState:true});for(const n of expanded)fragment.append(make(n));preview.replaceChildren(fragment);if(focusIdentity&&mode==='interact'){const restored=Array.from(preview.querySelectorAll('[data-start]')).find(el=>focusIdentity.key?el.dataset.key===focusIdentity.key:el.dataset.start===focusIdentity.start);if(restored){restored.focus({preventScroll:true});if(typeof focusIdentity.selectionStart==='number'&&restored.setSelectionRange)restored.setSelectionRange(focusIdentity.selectionStart,focusIdentity.selectionEnd);}}$('canvas').classList.toggle('interacting',mode!=='design');spacingOverlay?.update();}
function select(n){selected=n;selectedPath=entry;vectorPreview?.select(n.start);canvasTools?.update();elementTools?.update();layoutInspector?.update();if(active!==entry)open(entry);$('code').setSelectionRange(n.start,n.start);const line=files[entry].slice(0,n.start).split('\n').length;$('code').scrollTop=Math.max(0,(line-4)*23);$('lines').scrollTop=$('code').scrollTop;document.querySelectorAll('.ui-node').forEach(el=>el.classList.toggle('selected',Number(el.dataset.start)===n.start));$('inspector').innerHTML=`<h3>${esc(n.type)}</h3><div class="inspector-label">СВОЙСТВА</div>${Object.entries(n.props).map(([k,v])=>`<label class="property"><span>${esc(k)}</span><input data-prop="${esc(k)}" value="${esc(typeof v==='object'?JSON.stringify(v):v)}" ${typeof v==='object'?'disabled':''}></label>`).join('')}<div class="inspector-label">ПРИВЯЗКИ И СОБЫТИЯ</div>${Object.entries({...n.bindings,...n.events}).map(([k,v])=>`<div class="binding">${esc(k)}<code>${esc(v)}</code></div>`).join('')||'<small>Нет привязок</small>'}`;document.querySelectorAll('[data-prop]').forEach(input=>input.onchange=()=>{const key=input.dataset.prop;const old=n.props[key];const raw=typeof old==='number'?Number(input.value):typeof old==='boolean'?input.value==='true':input.value;if(typeof raw==='number'&&!Number.isFinite(raw))return;try{if(active!==entry)open(entry);codeEditor.edit(propertyEdit(files[entry],n.start,key,raw));}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;output();}});lines();controlTree?.select(n.start,entry);}
function refreshControlTree(){
  if(!controlTree)return;
  const path=controlTree.scope==='designer'?entry:active;
  let doc=null,message='',stale=false;
  if(!path?.endsWith('.ui'))message='Откройте файл .ui';
  else try{const source=files[path],previous=studioCache.get('tree:'+path)??(activeTreeDocument?.path===path?activeTreeDocument:null);doc=previous?.source===source?previous.document:parse(source);if(previous?.document!==doc)studioCache.set('tree:'+path,{source,document:doc});activeTreeDocument={path,source,document:doc};if(controlTree.scope==='designer'&&error)message=error;}
  catch(e){doc=(activeTreeDocument?.path===path?activeTreeDocument.document:studioCache.get('tree:'+path)?.document)??null;message=e.message;stale=!!doc;}
  controlTree.update({document:doc,path,error:message,stale,selectedStart:selected?.start,selectedPath,files});
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
  $('inspector').innerHTML=`<h3>${esc(item.node.type)}</h3><p>Узел шаблона</p><small>${esc(item.path)}<br>Изменяйте свойства в разметке. Геометрия конкретного экземпляра здесь не выбирается.</small>`;
  lines();controlTree.select(item.node.start,item.path);
}
function showNativeInspection(data){
 let panel=$('native-inspection');if(!panel){panel=document.createElement('details');panel.id='native-inspection';panel.className='layout-inspector';document.querySelector('.inspector').append(panel);}
 for(const other of document.querySelectorAll('.inspector>.layout-inspector'))if(other!==panel)other.open=false;
 panel.open=true;panel.replaceChildren();const title=document.createElement('summary');title.textContent='Нативное окно · живое дерево · F9: выбор';panel.append(title);
 const current=nativeSource&&files[nativeSource.entry]===nativeSource.source;
 const roots=nativeSource?.nodes?.[0]?.children??[],controls=roots[0]?.type==='Scroll'?roots[0].children:roots;
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
$('code').oninput=()=>{files[active]=$('code').value;lines();$('saved').textContent='Изменено';clearTimeout(saveTimer);saveTimer=setTimeout(persist,350);clearTimeout(compileTimer);compileTimer=setTimeout(()=>compile(active.endsWith('.design.ui')),250);};$('code').onscroll=()=>{$('lines').scrollTop=$('code').scrollTop;};$('code').onclick=lines;$('code').onkeyup=lines;
$('code').onkeydown=e=>{if(e.key==='Tab'){e.preventDefault();const el=e.target;el.setRangeText('    ',el.selectionStart,el.selectionEnd,'end');el.dispatchEvent(new Event('input'));}if((e.metaKey||e.ctrlKey)&&e.key==='s'){e.preventDefault();persist();}};
$('scenario').onchange=e=>{scenario=Number(e.target.value);compile(true);};$('entry').onclick=()=>{if(renderer==='vector'&&active==='components/Button.ui'){compile();return;}entry=active;scenario=0;compile(true);};
$('run').onclick=()=>{mode=mode==='design'?'interact':'design';sessionStorage.setItem(storageKey('forma-canvas-mode'),mode);$('run').textContent=mode==='design'?'▶ Взаимодействие':'⌖ Выбор элемента';$('caption').textContent=mode==='design'?'Выберите элемент, чтобы найти его в разметке':'События выполняются в дизайн-runtime · Rust не подключён';render();};
$('desktop').onclick=()=>{$('preview').style.width='520px';$('desktop').classList.add('chosen');$('mobile').classList.remove('chosen');};$('mobile').onclick=()=>{$('preview').style.width='320px';$('mobile').classList.add('chosen');$('desktop').classList.remove('chosen');};$('theme').onclick=()=>$('preview').classList.toggle('light');
$('break').onchange=e=>breakOn=e.target.checked;$('continue').onclick=()=>{if(pending){const p=pending;pending=null;$('continue').disabled=true;$('debug-status').textContent='Готов';execute(p.action);}};$('reset').onclick=()=>{pending=null;$('continue').disabled=true;$('debug-status').textContent='Готов';compile(true);log('Состояние сценария восстановлено');};
document.querySelectorAll('[data-tab]').forEach(b=>b.onclick=()=>{tab=b.dataset.tab;syncTabs();output();});
$('new').onclick=()=>{const path=prompt('Путь нового файла','ui/NewWindow.ui');if(!path)return;if(path in files){alert('Файл уже существует');return;}files[path]=path.endsWith('.ui')?'component NewWindow {\n    Frame {\n        Text {\n            text: \'Новое окно\';\n        }\n    }\n}\n':'';open(path);persist();};
$('export').onclick=()=>{const url=URL.createObjectURL(new Blob([JSON.stringify(files,null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='forma-project.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};$('import').onclick=()=>$('upload').click();$('upload').onchange=async e=>{try{const f=e.target.files[0];if(!f)return;const data=JSON.parse(await f.text());if(!data||Array.isArray(data)||typeof data!=='object'||!Object.keys(data).length||Object.values(data).some(v=>typeof v!=='string'))throw Error('Ожидается JSON проекта: пути файлов и текст');if(!confirm('Заменить текущий проект? При необходимости сначала экспортируйте его.'))return;files=data;active=Object.keys(files)[0];entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui'));scenario=0;open(active);persist();compile(true);}catch(e){alert(e.message);}finally{e.target.value='';}};
$('code').addEventListener('scroll',highlightSource);
$('code').addEventListener('input',()=>{selected=null;selectedPath=null;controlTree?.select(null,null);refreshControlTree();highlightSource();document.querySelectorAll('.ui-node.selected').forEach(el=>el.classList.remove('selected'));});
controlTree=createControlTree({explorer:document.querySelector('.explorer'),toolbar:document.querySelector('.preview-tools'),onSelect:selectTreeControl,onScopeChange:refreshControlTree,onOpenTemplate:path=>{open(path);controlTree.setView({scope:'file',visible:true});}});
codeEditor=mountEditor($('code'),{getProject:()=>({files,path:active})});
canvasTools=createCanvasTools({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.preview-tools'),
getScene:()=>renderer==='vector'?vectorPreview?.layoutSnapshot():null,
getSelection:()=>{if(!selected||selectedPath!==entry)return null;const children=compiled?.nodes?.[0]?.children??[];const nodes=children[0]?.type==='Scroll'?children[0].children:children;return {index:nodes.findIndex(n=>n.start===selected.start),root:selected.start===compiled?.nodes?.[0]?.start};},
getMode:()=>mode,setMode:next=>{if(mode!==next)$('run').click();},onChange:()=>{spacingOverlay?.update();layoutInspector?.update();elementTools?.update();}});
const presetPicker=document.createElement('select');presetPicker.className='design-preset';presetPicker.setAttribute('aria-label','Проверка дизайна');
for(const [value,label] of [['original','Исходный вид'],['long','Длинный текст'],['empty','Пустой текст'],['disabled','Недоступные контролы'],['list-empty','Список: пусто'],['list-12','Список: 12 строк'],['list-100','Список: 100 строк'],['loading','Список: загрузка'],['error','Список: ошибка']])presetPicker.add(new Option(label,value));
document.querySelector('.canvas-tools').append(presetPicker);
presetPicker.title='Только предпросмотр: исходники и нативное приложение не меняются';
presetPicker.onchange=()=>{designPresetName=presetPicker.value;try{if(mode!=='design')$('run').click();else render();error='';}catch(e){error=e.message;sourceDiagnostic=e.diagnostic??null;}output();};
layoutInspector=createLayoutInspector({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.canvas-tools'),
getVisuals:()=>designPresetName==='original'?lastVisuals:[],getRuntime:()=>vectorPreview?.layoutSnapshot(),getMode:()=>mode,
getControl:()=>lastPreviewControls.findIndex(n=>n.start===selected?.start),
readSource:path=>files[path],
readTracks:source=>{const raw=files[source.file].slice(source.from,source.to);const value=parse('component Tracks { Frame { columns: '+raw+'; } }').nodes[0].props.columns;return Array.isArray(value)?value:[value];},
openSource:source=>{open(source.file);$('code').setSelectionRange(source.from,source.to);$('code').focus();},
edit:(source,insert)=>{const previous=files[source.file];if(previous===undefined)throw Error('Файл не найден');const next=previous.slice(0,source.from)+insert+previous.slice(source.to);parse(next);if(active!==source.file)open(source.file);codeEditor.edit({from:source.from,to:source.to,insert});compile();}});
elementTools=createElementTools({viewport:$('canvas'),artboard:$('preview'),toolbar:document.querySelector('.canvas-tools'),
context:()=>{if(renderer!=='vector'||mode!=='design'||designPresetName!=='original'||error||!vectorPreview)return null;const root=compiled?.nodes[0],children=root?.children??[];return {source:files[entry],path:entry,root,start:selectedPath===entry?selected?.start:null,nodes:children[0]?.type==='Scroll'?children[0].children:children,scene:vectorPreview.layoutSnapshot(),grid:lastVisuals.find(v=>v.control===-1)?.grid};},
select,report:message=>{$('caption').textContent=message;},
history:action=>{
 const starts=elementTools.selection(),children=compiled?.nodes[0]?.children??[],before=children[0]?.type==='Scroll'?children[0].children:children;
 const identities=before.flatMap((n,index)=>starts.includes(n.start)?[{key:n.props.key,index,type:n.type}]:[]);
 if(active!==entry)open(entry);
 if(codeEditor[action]())queueMicrotask(()=>{clearTimeout(compileTimer);compile();const children=compiled?.nodes[0]?.children??[],after=children[0]?.type==='Scroll'?children[0].children:children;
  const restored=identities.map(id=>typeof id.key==='string'?after.find(n=>n.props.key===id.key):before.length===after.length&&after[id.index]?.type===id.type?after[id.index]:null).filter(Boolean);
  if(restored.length)select(restored[0]);elementTools.setSelection(restored.map(n=>n.start));elementTools.update();
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
spacingOverlay=createSpacingOverlay($('canvas'),()=>mode==='design'&&selectedPath===entry?selected:null);
open(active);compile(true);
let studioShell,shellDisposed=false;
mountStudioShell(document.querySelector('#app')).then(shell=>{if(shellDisposed)shell.destroy();else studioShell=shell;}).catch(e=>log('Контролы Studio: '+e.message));
import.meta.hot?.dispose(()=>{shellDisposed=true;studioShell?.destroy();elementTools?.destroy();canvasTools?.destroy();});
const rendererPicker=document.createElement('select');rendererPicker.id='renderer';rendererPicker.setAttribute('aria-label','Рендерер предпросмотра');rendererPicker.innerHTML='<option value="html">HTML · прежний</option><option value="vector">Вектор · Rust/WASM</option>';$('scenario').before(rendererPicker);rendererPicker.value=renderer;
rendererPicker.onchange=()=>{renderer=rendererPicker.value;localStorage.setItem(storageKey('forma-renderer'),renderer);selected=null;vectorPreview?.destroy();vectorPreview=undefined;initVector();compile();};
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
  const snapshot=()=>({active,entry,scenario:compiled?.scenarios[scenario]?.name,mode,error,selected:selected?.start??null,debug:{breakOn,paused:!!pending,action:pending?.action??null},renderer,capabilities:{vectorRenderer:!!vectorPreview,rustExecution:!!import.meta.hot,rustDap:false}});
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
      if(!args.path||args.path.split('/').some(p=>!p||p==='..'||p==='__proto__'||p==='constructor'||p==='prototype'))throw Error('Invalid project path');
      if(Object.hasOwn(files,args.path)&&args.expectedContent!==files[args.path])throw Error('File changed or expectedContent missing. Read it first.');
      files[args.path]=args.content;applyFiles();break;
    }
    case 'project_replace':{
      if(JSON.stringify(Object.entries(files).sort())!==JSON.stringify(Object.entries(args.expectedFiles).sort()))throw Error('Project changed. Read it first.');
      if(!Object.keys(args.files).length)throw Error('Project cannot be empty');
      files=Object.fromEntries(Object.entries(args.files));active=Object.keys(files)[0];entry=Object.keys(files).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui'));scenario=0;applyFiles();break;
    }
    case 'preview_renderer':rendererPicker.value=args.renderer;rendererPicker.onchange();break;
    case 'vector_status':return vectorPreview?.snapshot()??{ready:false};
    case 'preview_open':requireFile(args.path);entry=args.path;scenario=0;compile(true);break;
    case 'preview_scenario':{const index=compiled?.scenarios.findIndex(s=>s.name===args.name);if(index===undefined||index<0)throw Error('Scenario not found');scenario=index;$('scenario').value=index;$('scenario').onchange({target:$('scenario')});break;}
    case 'preview_mode':if(mode!==args.mode)$('run').click();break;
    case 'preview_viewport':$(args.device).click();$('preview').classList.toggle('light',args.theme==='light');break;
    case 'component_tree':return {entry,nodes:compiled?.nodes??[],error};
    case 'component_spacing':return spacingOverlay.read();
    case 'component_select':if(error)throw Error('Fix diagnostics before selecting compiled source');select(findNode(args.start));break;
    case 'component_property':{
      if(error||files[entry]!==args.expectedContent)throw Error('Stale or invalid source. Read entry file first.');
      const n=findNode(args.start);if(!Object.hasOwn(n.props,args.property)||typeof n.props[args.property]==='object')throw Error('Only existing literal properties are supported');
      if(typeof n.props[args.property]!==typeof args.value)throw Error('Property type mismatch');
      select(n);const input=Array.from(document.querySelectorAll('[data-prop]')).find(el=>el.dataset.prop===args.property);input.value=String(args.value);input.onchange();break;
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
  let designRequest;
  const refreshDesign=document.createElement('button');refreshDesign.textContent='↻ Данные дизайна';$('run').before(refreshDesign);
  refreshDesign.title='Выполнить локальный src/design.rs (без песочницы), получить текстовые константы и результаты методов';
  refreshDesign.onclick=()=>{
    if(!compiled)return;
    const refs=designReferences(compiled);
    if(!refs.length){log('Нет ссылок design в дизайне');return;}
    if(!confirm('Выполнить src/design.rs для получения дизайн-данных? Rust-код работает с правами вашего пользователя.'))return;
    designRequest={id:crypto.randomUUID(),source:JSON.stringify(files)};
    refreshDesign.disabled=true;
    import.meta.hot.send('forma:design-data',{id:designRequest.id,files,refs});
  };
  import.meta.hot.on('forma:design-data-result',data=>{
    if(data.id!==designRequest?.id)return;
    refreshDesign.disabled=false;
    if(designRequest.source!==JSON.stringify(files)){log('Проект изменился: обновите дизайн-данные ещё раз');return;}
    if(data.error){error=data.error;output();return;}
    setDesignData(data.values);compile();log('Дизайн-данные обновлены из Rust');
  });
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
  import.meta.hot.dispose(()=>{import.meta.hot.off('forma:command',handle);import.meta.hot.off('vite:ws:connect',hello);});
}
