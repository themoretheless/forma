import {svgShapes} from '../src/svg-shapes.js';
import {canvasIcons} from '../src/studio-canvas-icons.js';
import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import {compileComponents} from '../src/components.js';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
import {shellControl,shellScene} from '../src/studio-shell-controls.js';
import init,{Runtime,text_metrics} from '../public/vector-pkg/forma.js';

test('Studio shell uses guideline controls and renders their states through shared Rust',async()=>{
 await init({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
 const library=materializeTheme(Object.fromEntries(['components','assets'].flatMap(folder=>readdirSync(new URL(`../vector-ui/controls/${folder}/`,import.meta.url)).map(name=>[`${folder}/${name}`,readFileSync(new URL(`../vector-ui/controls/${folder}/${name}`,import.meta.url),'utf8')]))),'dark');
 const types={primary:'PrimaryButton',button:'SecondaryButton',ghost:'GhostButton',icon:'IconButton',menu:'MenuItem',notice:'Alert',tab:'TabButton',tree:'TreeItem',check:'Checkbox',field:'TextField',select:'SelectTrigger',status:'StatusBar'};
 for(const [kind,type]of Object.entries(types)){
  for(const selected of [false,true]){
   const spec=shellControl({kind,text:'Source.ui',width:240,height:38,selected,disabled:selected,checked:selected,branch:true,expanded:selected});
   assert.equal(spec.type,type);
   const out=compileComponents({...library,'ui/Shell.ui':shellScene(spec)},'ui/Shell.ui',{}, {measureText:text_metrics});
   const model=new Runtime();try{model.load_component(out.source,out.template);assert.equal(model.control_count(),1);assert.equal(model.control_disabled(0),selected);assert.equal(model.content_pixels(240,38,1).length,240*38*4);}finally{model.free();}
  }
 }
 const proximitySpec=shellControl({kind:'button',text:'Reveal',width:160,height:32});
 const proximityOut=compileComponents({...library,'ui/Shell.ui':shellScene(proximitySpec)},'ui/Shell.ui',{}, {measureText:text_metrics});
 const proximityModel=new Runtime();try{
  proximityModel.load_component(proximityOut.source,proximityOut.template);proximityModel.set_reduced_motion(true);
  const before=proximityModel.content_pixels(160,32,1);
  proximityModel.reveal_pointer(165,16,true);proximityModel.tick(500);
  assert.notDeepEqual(proximityModel.content_pixels(160,32,1),before,'Reveal reaches a neighbouring control without pointer hover');
  proximityModel.reveal_pointer(0,0,false);proximityModel.tick(500);
  assert.deepEqual(proximityModel.content_pixels(160,32,1),before,'leaving the window removes proximity paint');
 }finally{proximityModel.free();}
 const literal=shellScene(shellControl({text:"' ${state.secret} \\ ",width:100,height:32}));
 const out=compileComponents({...library,'ui/Shell.ui':literal},'ui/Shell.ui',{}, {measureText:text_metrics});assert.ok(out.template.includes('state.secret'));
});

test('canvas SVG icons have renderable strokes in the supported subset',()=>{for(const [,name]of canvasIcons){const svg=readFileSync(new URL(`../vector-ui/controls/assets/${name}.svg`,import.meta.url),'utf8');assert.ok(svgShapes(svg,[0,0,20,20]).length>0,name);}});
