#!/usr/bin/env node
// Forma Studio's chrome in a native Forma window (`--backend egui` by default):
//   npm run studio:native [-- <project folder>] [--svet]
// The window is studio-ui/ composed into one scene and drawn by the native runtime. This host
// keeps the state, as the browser shell does: clicks arrive on the window's stdout, and every
// change is hot-reloaded into the same window. "Запустить" opens the project's app beside it.
import {readFileSync,readdirSync,statSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {compileComponents} from '../src/components.js';
import {parse} from '../src/language.js';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
import {composeStudioWindow,keyText} from '../src/studio-window.js';
import {sampleProject} from '../src/sample-project.js';
import {createVectorRunner} from '../vector-runner.js';
import init,{text_metrics} from '../public/vector-pkg/forma.js';

const root=fileURLToPath(new URL('../',import.meta.url));
const args=process.argv.slice(2);
const backend=args.includes('--svet')?'svet':'egui';
const projectDir=args.find(arg=>!arg.startsWith('--'));
await init({module_or_path:readFileSync(path.join(root,'public/vector-pkg/forma_bg.wasm'))});

function readTree(base,folders,filter=()=>true){
 const out={};
 const walk=dir=>{for(const name of readdirSync(dir)){if(name.startsWith('.')||['target','node_modules','dist'].includes(name))continue;const full=path.join(dir,name);if(statSync(full).isDirectory())walk(full);else if(filter(name))out[path.relative(base,full).split(path.sep).join('/')]=readFileSync(full,'utf8');}};
 for(const folder of folders)walk(path.join(base,folder));
 return out;
}
const controlLibrary=readTree(path.join(root,'vector-ui/controls'),['components','assets']);
const shellFiles=materializeTheme({...controlLibrary,...readTree(path.join(root,'studio-ui'),['components','ui'])},'dark');
const rawProject=projectDir?readTree(path.resolve(projectDir),['.'],name=>/\.(ui|svg)$/.test(name)):readTree(path.join(root,'openscad-ui'),['ui'],name=>/\.(ui|svg)$/.test(name));
const project=materializeTheme({...controlLibrary,...rawProject},'dark');
const entry=rawProject['ui/OpenScadStudio.ui']?'ui/OpenScadStudio.ui':Object.keys(rawProject).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui')&&/^component\s/m.test(rawProject[p].replace(/^#.*$/gm,'')))??Object.keys(project).find(p=>p.endsWith('.ui')&&!p.endsWith('.design.ui')&&/^component\s/m.test(project[p].replace(/^#.*$/gm,'')));
if(!entry)throw Error('В проекте нет .ui компонента');
const title=projectDir?path.basename(path.resolve(projectDir)):'open-scad-viewer';

// Design states of the entry, as Studio's state tabs list them.
const states=[{name:'Базовые значения',state:{}}];
for(const ref of parse(project[entry]).designs??[]){
 const file=(entry.includes('/')?entry.slice(0,entry.lastIndexOf('/')+1):'')+ref.replace(/^\.\//,'');
 if(project[file])for(const s of parse(project[file]).states)states.push({name:s.name,state:s.state??{}});
}

const fit=(text,size,pad)=>Math.ceil(text_metrics(text,size)[0])+2*pad;
let selected=0,paletteOpen=false,appRunning=false,problem='',windowKeys=new Map();
const visibleRows=9;
function commands(){
 return [
  appRunning?{label:'Остановить приложение',run:stopApp}:{label:'Запустить приложение',key:'⌘↵',run:launchApp},
  {label:'Закрыть Studio',run:()=>{stopApp();shell.stop();}},
 ];
}
function windowState(){
 const items=paletteOpen?commands().slice(0,visibleRows):[];
 const problemsText=problem?'1':'0';
 return {
  title,titleWidth:fit(title,13,12),
  tabs:states.length>1?states.map((s,i)=>({id:i,label:s.name,selected:i===selected,width:fit(s.name,12,10)})):[],
  showMode:appRunning,modeText:'Остановить',modeWidth:fit('Остановить',13,12),
  canLaunch:true,launchText:appRunning?'Обновить':'Запустить',launchWidth:fit(appRunning?'Обновить':'Запустить',13,14),
  canvasText:problem||(appRunning?`${entry} · открыто в отдельном окне`:`${entry} · Запустить откроет приложение`),
  unsaved:false,zoomable:false,problemsText,problemsWidth:fit(problemsText,12,8)+22,bad:!!problem,
  search:false,paletteOpen,placeholder:'',query:'',
  items:items.map((item,i)=>({id:i,label:item.label,hint:keyText(item.key),active:false})),
 };
}
function compileWindow(){
 const state=windowState();
 const source=materializeTheme({'ui/StudioWindow.ui':composeStudioWindow(shellFiles,{width:1100,height:720,paletteHeight:16+Math.max(1,state.items.length)*36})},'dark')['ui/StudioWindow.ui'];
 const files={...shellFiles,'ui/StudioWindow.ui':source};
 const linked=compileComponents(files,'ui/StudioWindow.ui',state,{measureText:text_metrics});
 // The native window reports a click by the control's key; the key leads back to the node and
 // its already evaluated arguments, exactly as the browser host reads them.
 windowKeys=new Map(linked.previewControls.map(node=>[node.props.key,node]));
 return linked;
}
function compileApp(){
 const designState=selected===0?null:states[selected].name;
 const linked=compileComponents(project,entry,structuredClone(states[selected].state),{measureText:text_metrics,designState});
 return {source:linked.source,template:linked.template};
}

const log=prefix=>({kind,text})=>{if(text&&kind!=='stdout')process.stderr.write(`${prefix} ${text.trimEnd()}\n`);};
const app=createVectorRunner(event=>{log('[app]')(event);if(event.kind==='finished'){appRunning=false;refresh();}},{root});
function launchApp(){
 try{const next=compileApp();problem='';if(appRunning)app.update(next);else{appRunning=true;app.run({...next,backend});}}
 catch(error){problem=`Ошибка: ${error.message}`;}
 refresh();
}
function stopApp(){if(appRunning)app.stop();}
function selectState(i){selected=i;if(appRunning)launchApp();else refresh();}

const handlers={
 menu:()=>{paletteOpen=true;},
 closePalette:()=>{paletteOpen=false;},
 pick:i=>{const item=commands()[i];paletteOpen=false;item?.run();},
 state:i=>selectState(i),
 launch:launchApp,
 mode:stopApp,
 problems:()=>{},
};
let buffer='';
const shell=createVectorRunner(event=>{
 if(event.kind==='finished'){stopApp();process.exit(0);}
 if(event.kind!=='stdout'){log('[studio]')(event);return;}
 buffer+=event.text;
 const lines=buffer.split('\n');buffer=lines.pop();
 for(const line of lines){
  const click=/^(.*)\.clicked → actions\.(\w+)/.exec(line),dismiss=/\.dismissed → actions\.(\w+)/.exec(line);
  if(dismiss){handlers[dismiss[1]]?.();refresh();continue;}
  if(!click)continue;
  const node=windowKeys.get(click[1]);
  handlers[click[2]]?.(...(node?.eventArgs?.clicked??[]));
  refresh();
 }
},{root});
function refresh(){const {source,template}=compileWindow();shell.update({source,template});}

const {source,template}=compileWindow();
console.log(`Forma Studio (${backend}) · ${title} · ${entry}`);
shell.run({source,template,backend});
for(const signal of ['SIGINT','SIGTERM'])process.on(signal,()=>{stopApp();shell.stop();setTimeout(()=>process.exit(0),300);});
