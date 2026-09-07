import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import {compileComponents} from '../src/components.js';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
import init,{Runtime,text_metrics} from '../public/vector-pkg/forma.js';
await init({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
const files={};for(const dir of ['components','assets'])for(const file of readdirSync(`vector-ui/controls/${dir}`))files[`${dir}/${file}`]=readFileSync(`vector-ui/controls/${dir}/${file}`,'utf8');
function field(type='TextField',props='') {
 const source=`component Demo { Frame { width:300; height:120; ${type} { key:'editor'; width:240; height:100; value:${type==='Slider'?'50%':"'Привет'"}; ${props} } } }`;
 const compiled=compileComponents({...materializeTheme(files,'dark'),'ui/Test.ui':source},'ui/Test.ui',{}, {measureText:text_metrics});
 const model=new Runtime();model.load_component(compiled.source,compiled.template);return {model,compiled};
}
test('real input edits Unicode, selects, deletes, and restores undo/redo in Rust',()=>{
 const {model}=field();try{
 assert.equal(model.control_editable(0),true);assert.equal(model.text_insert('ignored'),false);
 model.focus_control(0);model.text_key('a',false,true);model.text_insert('тест🙂');
 assert.equal(model.text_value(0),'тест🙂');model.text_key('Backspace',false,false);assert.equal(model.text_value(0),'тест');
 model.text_key('z',false,true);assert.equal(model.text_value(0),'тест🙂');model.text_key('z',true,true);assert.equal(model.text_value(0),'тест');
 model.text_key('Home',false,false);model.text_key('ArrowRight',true,false);assert.equal(model.text_selected(),'т');
 model.text_insert('Т');assert.equal(model.text_value(0),'Тест');
 }finally{model.free();}
});
test('Textarea accepts line breaks, while single-line paste normalizes them',()=>{
 for(const type of ['TextField','TextArea']){const {model}=field(type);try{
 model.focus_control(0);model.text_key('a',false,true);model.text_insert('one\r\ntwo');
 assert.equal(model.text_value(0),type==='TextArea'?'one\ntwo':'one two');
 model.text_key('Enter',false,false);assert.equal(model.text_value(0),type==='TextArea'?'one\ntwo\n':'one two');
 }finally{model.free();}}
});
test('disabled fields reject text, pointer selection works, and edits survive unrelated snapshot compilation',()=>{
 const disabled=field('TextField','disabled:true;').model;try{assert.equal(disabled.focus_control(0),false);assert.equal(disabled.text_insert('x'),false);assert.equal(disabled.text_value(0),'Привет');}finally{disabled.free();}
 const {model,compiled}=field();const next=new Runtime();try{
 const [x,y,w,h]=model.control_bounds(0);model.pointer(x+12,y+h/2,1);model.pointer(x+12,y+h/2,2);model.text_insert('!');assert.equal(model.text_value(0),'!Привет');
 next.load_component(compiled.source,compiled.template);next.preserve_interaction(model);assert.equal(next.text_value(0),'!Привет');
 model.pointer(x+12,y+h/2,1);model.pointer(x+w-20,y+h/2,0);model.pointer(x+w-20,y+h/2,2);assert.equal(model.text_selected(),'!Привет');
 }finally{model.free();next.free();}
});
test('input pixels and GPU geometry change with text and selection; preedit does not commit twice',()=>{
 const {model}=field();try{
 model.focus_control(0);const before=model.content_pixels(300,120,1);const commands=model.gpu_commands(1,false);
 model.text_key('a',false,true);assert.notDeepEqual(model.content_pixels(300,120,1),before);
 model.text_preedit('日本');assert.equal(model.text_value(0),'Привет');model.text_insert('日本');assert.equal(model.text_value(0),'日本');
 assert.notDeepEqual(model.gpu_commands(1,false),commands);assert.equal(model.text_caret_bounds().length,4);
 model.text_key('a',false,true);model.text_insert('A'.repeat(200));const [x,,w]=model.control_bounds(0);assert.ok(model.text_caret_bounds()[0]<x+w);
 }finally{model.free();}
});

test('gallery export keeps edited field values as native TextInput initial values',async()=>{
 const {catalogProject}=await import('../vector-ui/controls/catalog.js');
 const project=catalogProject(files,'dark','regular',{state:{fields:{projectField:'новый проект',searchField:'query',descriptionField:'строка 1\nстрока 2'}}});
 const compiled=compileComponents(project,'ui/FormaControls.ui',{}, {measureText:text_metrics});
 const model=new Runtime();try {
 model.load_component(compiled.source,compiled.template);
 for(const [key,value] of Object.entries({projectField:'новый проект',searchField:'query',descriptionField:'строка 1\nстрока 2'})){
 const i=Array.from({length:model.control_count()},(_,i)=>i).find(i=>model.control_key(i)===key);assert.equal(model.text_value(i),value);
 }
 }finally{model.free();}
});

test('runtime drives smooth caret and character fade, and settles under reduced motion',()=>{
 const {model}=field();try{
 model.focus_control(0); model.set_reduced_motion(true);model.set_reduced_motion(false);
 const first=model.content_pixels(300,120,1);
 assert.equal(model.is_animating(),true);
 model.tick(275);assert.notDeepEqual(model.content_pixels(300,120,1),first);
 model.text_key('a',false,true);model.text_insert('Fade🙂');
 assert.equal(model.text_value(0),'Fade🙂');
 const initial=model.content_pixels(300,120,1);
 model.tick(110);assert.notDeepEqual(model.content_pixels(300,120,1),initial);
 model.set_reduced_motion(true);assert.equal(model.is_animating(),false);
 const settled=model.content_pixels(300,120,1);
 model.tick(550);assert.deepEqual(model.content_pixels(300,120,1),settled);
 model.set_reduced_motion(false);model.focus(false);model.tick(1000);
 assert.equal(model.is_animating(),false);
 }finally{model.free();}
});

test('shared Rust slider captures drags outside bounds and supports keyboard without host callbacks',()=>{
 const {model}=field('Slider');try{
 const [x,y,w,h]=model.control_bounds(0);
 model.pointer(x+8,y+h/2,1);assert.equal(model.range_value(0),0);
 model.pointer(x+w+100,y+h/2,0);assert.equal(model.range_value(0),1);
 model.pointer(x+w+100,y+h/2,2);
 const pixels=model.content_pixels(300,120,1);
 model.range_key('Home');assert.equal(model.range_value(0),0);
 assert.notDeepEqual(model.content_pixels(300,120,1),pixels);
 model.range_key('ArrowRight');assert.ok(Math.abs(model.range_value(0)-0.01)<1e-6);
 model.range_key('End');assert.equal(model.range_value(0),1);
 model.range_key('ArrowRight');assert.equal(model.range_value(0),1);
 model.focus(false);assert.equal(model.range_key('Home'),false);
 }finally{model.free();}
 const disabled=field('Slider','disabled:true;').model;try{
 const [x,y]=disabled.control_bounds(0);disabled.pointer(x+20,y+20,1);
 assert.equal(disabled.range_key('End'),false);
 }finally{disabled.free();}
});

test('visual composition inspection carries actual local bounds and source locations',()=>{
 const {compiled,model}=field('Slider');try{
 assert.ok(compiled.visualNodes.length>0);
 const grid=compiled.visualNodes.find(n=>n.grid&&n.props.columns);
 assert.ok(grid);assert.equal(grid.source.file,'components/SliderTrack.ui');
 assert.ok(grid.propertySources.columns);assert.ok(grid.grid.columns.length===2);
 assert.ok(compiled.visualNodes.every(n=>n.bounds.every(Number.isFinite)));
 }finally{model.free();}
});

test('all design presets compile and render without changing source files',async()=>{
 const {designPreset}=await import('../src/design-presets.js');
 const source="component Demo {Frame {width:320;height:100;TextField {value:'hello';}}}";
 const project={...materializeTheme(files,'dark'),'ui/Presets.ui':source};
 for(const name of ['original','long','empty','disabled']){
  const c=compileComponents(project,'ui/Presets.ui',{}, {measureText:text_metrics,instanceProps:n=>designPreset(name,n)});
  const r=new Runtime();try{r.load_component(c.source,c.template);r.content_pixels(320,100,1);if(name==='disabled')assert.equal(r.control_disabled(0),true);}finally{r.free();}
 }
 assert.equal(project['ui/Presets.ui'],source);
});

test('root Grid is lowered identically for native and WASM with source metadata',()=>{
 const source="component Grid { Frame { width:320; height:120; padding:10; gap:8; columns:[100,*]; rows:[*]; TextField {key:'a';column:1;} TextField {key:'b';column:2;} } }";
 const c=compileComponents({...materializeTheme(files,'dark'),'ui/Grid.ui':source},'ui/Grid.ui',{}, {measureText:text_metrics});
 const r=new Runtime();try{
 r.load_component(c.source,c.template);
 assert.deepEqual(Array.from(r.control_bounds(0)),[10,10,100,100]);
 assert.deepEqual(Array.from(r.control_bounds(1)),[118,10,192,100]);
 const grid=c.visualNodes.find(v=>v.control===-1);
 assert.deepEqual(grid.grid.columns,[100,192]);assert.equal(grid.propertySources.columns.file,'ui/Grid.ui');
 }finally{r.free();}
});

test('collection fixtures cover empty, populated, loading and error without source mutation',async()=>{
 const {collectionScenario}=await import('../src/design-presets.js');
 const source="component Collection {Frame {width:360;height:220;TableRow {first:'real';}}}";
 const project={...materializeTheme(files,'dark'),'ui/Collection.ui':source};
 for(const [name,count] of [['list-empty',1],['list-12',12],['list-100',100],['loading',6],['error',1]]){
  const c=compileComponents(project,'ui/Collection.ui',{}, {measureText:text_metrics,transformScene:s=>collectionScenario(name,s,project)});
  const r=new Runtime();try{r.load_component(c.source,c.template);assert.equal(r.control_count(),count);r.content_pixels(360,220,1);assert.equal(r.scrollable(),true);}finally{r.free();}
 }
 assert.equal(project['ui/Collection.ui'],source);
});
test('deleting the last control compiles to a real empty Runtime scene',()=>{
 const compiled=compileComponents({'ui/Empty.ui':'component Empty { Frame { width: 200; height: 100; } }'},'ui/Empty.ui');
 const model=new Runtime();try{model.load_component(compiled.source,compiled.template);assert.equal(model.control_count(),0);assert.equal(model.content_pixels(200,100,1).length,200*100*4);assert.equal(model.label(),'');assert.equal(compiled.visualNodes.filter(v=>v.control>=0).length,0);}finally{model.free();}
});
