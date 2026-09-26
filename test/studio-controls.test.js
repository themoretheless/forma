import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import {createStudioControlsProject} from '../src/studio-controls-project.js';
import {compileComponents} from '../src/components.js';
import {insertableControls} from '../src/control-catalog.js';
import {insertElement} from '../src/element-edit.js';
import {parse} from '../src/language.js';
import init,{Runtime,text_metrics} from '../public/vector-pkg/forma.js';

const read=(root,folders)=>Object.fromEntries(folders.flatMap(folder=>readdirSync(new URL(`../${root}/${folder}/`,import.meta.url)).map(name=>[`${folder}/${name}`,readFileSync(new URL(`../${root}/${folder}/${name}`,import.meta.url),'utf8')])));
const project=createStudioControlsProject(read('vector-ui/controls',['components','assets']),read('studio-controls',['ui']));

test('Studio control project compiles and renders every page through Rust/WASM',async()=>{
 await init({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
 assert.equal(Object.keys(project)[0],'ui/StudioControls.ui');
 for(const entry of Object.keys(project).filter(path=>path.startsWith('ui/'))){
  const compiled=compileComponents(project,entry,{}, {measureText:text_metrics});
  const runtime=new Runtime();
  try{
   runtime.load_component(compiled.source,compiled.template);
   assert.ok(runtime.control_count()>0,entry);
   assert.ok(runtime.control_count()<256,entry);
   const pixels=runtime.pixels(900,780,1);assert.equal(pixels.length,900*780*4);
   assert.ok(compiled.previewControls.some(node=>['Surface','PrimaryButton','TabButton','TreeItem','TextField','Checkbox','Alert'].includes(node.type)),entry);
  }finally{runtime.free();}
 }
});
test('the designer palette offers exactly what this project can put on a page',()=>{
 const entry='ui/StudioControls.ui',source=project[entry];
 const scroll=parse(source).nodes[0].children[0];
 assert.equal(scroll.type,'Scroll');
 const place=markup=>{const change=insertElement(source,scroll.start,markup,false,'inside');
  return compileComponents({...project,[entry]:source.slice(0,change.from)+change.insert+source.slice(change.to)},entry,{}, {measureText:(text,size)=>[text.length*size*0.55,size]});};
 const items=insertableControls(project),types=items.map(item=>item.type);
 // A page control is a component whose definition is one Rectangle, so both a real control
 // compiles and the compiler itself refuses the two groups the palette drops.
 assert.ok(types.includes('Button'));
 assert.ok(items.find(item=>item.type==='Button').markup);
 place('Button { }');
 for(const [markup,reason] of [['Rectangle { width: 40; height: 40; }','Компонент не найден'],['Icon { width: 40; height: 40; }','Базовая кнопка требует один Rectangle']]){
  const type=markup.split(' ')[0];
  assert.ok(!types.includes(type),type+' is not a page control');
  assert.throws(()=>place(markup),new RegExp(reason));
 }
 assert.ok(!types.includes('Image'),'a picture needs a file to point at, so it is not a built-in control');
});
