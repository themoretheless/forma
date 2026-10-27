import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import {compileComponents} from '../src/components.js';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
import {composeStudioWindow,keyText} from '../src/studio-window.js';
import init,{Runtime,text_metrics} from '../public/vector-pkg/forma.js';

const read=(root,folders)=>Object.fromEntries(folders.flatMap(folder=>readdirSync(new URL(`../${root}/${folder}/`,import.meta.url)).map(name=>[`${folder}/${name}`,readFileSync(new URL(`../${root}/${folder}/${name}`,import.meta.url),'utf8')])));
await init({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
const files=materializeTheme({...read('vector-ui/controls',['components','assets']),...read('studio-ui',['components','ui'])},'dark');
const fit=(text,size,pad)=>Math.ceil(text_metrics(text,size)[0])+2*pad;
const tabs=['Базовые значения','Загрузка'].map((label,i)=>({id:i,label,selected:!i,width:fit(label,12,10)}));
const items=['Показать код','Слои'].map((label,i)=>({id:i,label,hint:'',active:!i}));
const regionState={
 'ui/Title.ui':{title:'Forma',titleWidth:fit('Forma',13,12)},
 'ui/Run.ui':{showMode:true,modeText:'Дизайн',modeWidth:80,canLaunch:true,launchText:'Запустить',launchWidth:100},
 'ui/States.ui':{tabs},
 'ui/Corner.ui':{zoomable:true,unsaved:true,unsavedText:'Не сохранено',unsavedWidth:100,zoom:'100%',problemsText:'2',problemsWidth:40,bad:true},
 'ui/Hints.ui':{hints:[{key:'Cmd K',label:'команды',keyWidth:40,labelWidth:60}]},
 'ui/Palette.ui':{search:true,placeholder:'Команда…',query:'',items},
 'ui/Inspector.ui':{title:'Button',hasNote:false,note:'',canAdd:true,hasNotice:false,notice:'',
  rows:[{key:'text',text:'OK',editable:true,readonly:false,layered:false,error:false},{key:'x',text:'12',editable:false,readonly:true,layered:false,error:false}],
  links:[{key:'clicked',label:'clicked ->',text:'actions.save',error:false}]},
};
function load(compiled){const runtime=new Runtime();runtime.load_component(compiled.source,compiled.template);return runtime;}

test('every Studio chrome region compiles from studio-ui and renders through Rust/WASM',()=>{
 assert.deepEqual(Object.keys(regionState).sort(),Object.keys(files).filter(p=>p.startsWith('ui/')).sort());
 for(const [entry,state] of Object.entries(regionState)){
  const runtime=load(compileComponents(files,entry,state,{measureText:text_metrics}));
  try{
   assert.ok(runtime.control_count()>0,entry);
   // A region is exactly as large as its controls, so it floats over the canvas without a box.
   assert.ok(runtime.render_width()<600&&runtime.render_height()<300,entry);
  }finally{runtime.free();}
 }
});

test('clicks carry their evaluated arguments, so a host needs no expression evaluator',()=>{
 const compiled=compileComponents(files,'ui/States.ui',{tabs},{measureText:text_metrics});
 assert.deepEqual(compiled.previewControls.map(node=>node.eventArgs.clicked),[[0],[1]]);
 assert.equal(compiled.previewControls[0].events.clicked,'actions.state');
});

test('the native window composes the same regions into one scene with the palette as a modal',()=>{
 const source=materializeTheme({'ui/W.ui':composeStudioWindow(files,{width:900,height:600,paletteHeight:88})},'dark')['ui/W.ui'];
 for(const key of ['menu','mode','launch','problems','query'])assert.ok(source.includes(`key: '${key}'`),key);
 const state={...Object.assign({},...Object.values(regionState)),search:false,zoomable:false,showMode:false,paletteOpen:true,canvasText:'ui/App.ui'};
 const compiled=compileComponents({...files,'ui/W.ui':source},'ui/W.ui',state,{measureText:text_metrics});
 const keys=compiled.previewControls.map(node=>node.props.key);
 assert.ok(keys.includes('menu')&&keys.includes('launch')&&keys.includes('canvas'));
 assert.ok(!keys.includes('query'),'the native palette has no search field the host could not read');
 const runtime=load(compiled);
 try{assert.equal(runtime.render_width(),900);assert.equal(runtime.modal_count(),1);assert.ok(runtime.modal_is_open(0));}
 finally{runtime.free();}
});

test('shortcut hints avoid glyphs the bundled font does not have',()=>{
 assert.equal(keyText('⌘K'),'Cmd K');
 assert.equal(keyText('⌘↵'),'Cmd Enter');
 assert.equal(keyText(undefined),'');
});

test('a root sized to content fits its controls, and an unsized root keeps the 360×220 frame',()=>{
 const scene=size=>`component Fit { Row { ${size} padding: 4; gap: 2; Button { width: 50; height: 20; } Button { width: 30; height: 24; } } }`;
 const frame=size=>{const runtime=load(compileComponents({...files,'ui/Fit.ui':scene(size)},'ui/Fit.ui',{}, {measureText:text_metrics}));try{return [runtime.width(),runtime.height()];}finally{runtime.free();}};
 assert.deepEqual(frame('width: content; height: content;'),[90,32]);
 assert.deepEqual(frame(''),[360,220]);
});

test('the inspector region edits each row through a field keyed by its property name',()=>{
 const compiled=compileComponents(files,'ui/Inspector.ui',regionState['ui/Inspector.ui'],{measureText:text_metrics});
 const fields=compiled.previewControls.filter(node=>node.type==='TextField');
 assert.deepEqual(fields.map(node=>String(node.props.key).split('/').pop()),['text','clicked']);
 assert.deepEqual(fields.map(node=>node.props.value),['OK','actions.save']);
 // A read-only row has no field and no remove button; an editable one removes by its name.
 assert.deepEqual(compiled.previewControls.filter(node=>node.events?.clicked==='actions.remove').map(node=>node.eventArgs.clicked),[['text']]);
 const runtime=load(compiled);
 try{assert.equal(runtime.width(),258);}finally{runtime.free();}
});
