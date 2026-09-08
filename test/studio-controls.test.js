import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import {createStudioControlsProject} from '../src/studio-controls-project.js';
import {compileComponents} from '../src/components.js';
import init,{Runtime,text_metrics} from '../public/vector-pkg/forma.js';

test('Studio control project compiles and renders every page through Rust/WASM',async()=>{
 await init({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
 const read=(root,folders)=>Object.fromEntries(folders.flatMap(folder=>readdirSync(new URL(`../${root}/${folder}/`,import.meta.url)).map(name=>[`${folder}/${name}`,readFileSync(new URL(`../${root}/${folder}/${name}`,import.meta.url),'utf8')])));
 const project=createStudioControlsProject(read('vector-ui/controls',['components','assets']),read('studio-controls',['ui']));
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
