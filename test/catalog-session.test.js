import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import init,* as runtime from '../public/vector-pkg/forma.js';
import {createCatalogSession} from '../vector-ui/controls/catalog-session.js';
import {sectionSource,sections,catalogProject} from '../vector-ui/controls/catalog.js';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
import {compileComponents} from '../src/components.js';
import {parse} from '../src/language.js';
await init({module_or_path:readFileSync(new URL('../public/vector-pkg/forma_bg.wasm',import.meta.url))});
const files={};
for(const dir of ['components','assets'])for(const name of readdirSync(new URL(`../vector-ui/controls/${dir}/`,import.meta.url)))files[`${dir}/${name}`]=readFileSync(new URL(`../vector-ui/controls/${dir}/${name}`,import.meta.url),'utf8');
const compile=(section,state)=>compileComponents({...materializeTheme(files),'ui/Test.ui':sectionSource(section,'light','regular',state)},'ui/Test.ui',{}, {measureText:runtime.text_metrics});

test('catalog uses real Rust check, exclusive selection, range and tree models',()=>{
  const changes=[], session=createCatalogSession(runtime,(...args)=>changes.push(args));
  try {
    assert.equal(session.state().check,true);session.dispatch('actions.check');assert.equal(session.state().check,false);
    assert.equal(session.state().mixed,2);session.dispatch('actions.mixed');assert.notEqual(session.state().mixed,2);
    session.dispatch('actions.protocol2');assert.equal(session.state().radio,2);
    session.dispatch('actions.chip1');assert.equal(session.state().chips[1],true);
    session.dispatch('actions.rangeMore');assert.equal(session.state().range,9);
    session.pointer('start',{node:{props:{key:'rangeSlider'}},x:-100,width:304});assert.equal(session.state().range,1);
    session.pointer('move',{node:{props:{key:'rangeSlider'}},x:500,width:304});assert.equal(session.state().range,16);
    session.key({key:'Home'},{props:{key:'rangeSlider'}});assert.equal(session.state().range,1);
    session.dispatch('actions.tree2');assert.deepEqual(session.state().treeRows.map(row=>row.index),[0,1,2]);
    session.key({key:'ArrowRight'},{props:{key:'tree2'}});assert.equal(session.state().treeRows.length,5);
    assert.ok(changes.some(([section])=>section==='range'));
  } finally {session.destroy();}
});

test('popup, dialog and document examples recompose all actual visual variants',()=>{
  const changes=[],session=createCatalogSession(runtime,(...args)=>changes.push(args));
  try {
    for(const action of ['selectToggle','target2','disclosure','showDialog','demoDialogClose','showTooltip','docClose0','docClose0','docClose0','resetDocuments']) {
      session.dispatch(`actions.${action}`);
      const section=changes.at(-1)[0],linked=compile(section,session.state()),model=new runtime.Runtime();
      try{model.load_component(linked.source,linked.template);assert.ok(model.gpu_commands(1.25,true).length>0);}finally{model.free();}
    }
    session.dispatch('actions.showDialog');
    assert.equal(session.key({key:'Tab',shiftKey:true},{props:{key:'demoDialogClose'}}),true);
    assert.deepEqual(changes.at(-1),['feedback','demoDialogConfirm']);
    session.key({key:'Escape'},{props:{key:'demoDialogConfirm'}});assert.equal(session.state().dialog,false);
    assert.deepEqual(changes.at(-1),['feedback','showDialog']);
  }finally{session.destroy();}
});

test('slider thumb and track share one percentage at arbitrary widths',()=>{
  for(const width of [200,304])for(const percent of [0,50,100]) {
    const entry=`component Test {Frame {width:${width};height:32;padding:0;Slider{width:${width};value:${percent}%;}}}`;
    const linked=compileComponents({...materializeTheme(files),'ui/Test.ui':entry},'ui/Test.ui',{}, {measureText:runtime.text_metrics});
    const model=new runtime.Runtime();
    try {
      model.load_component(linked.source,linked.template);
      const pixels=model.pixels(width,32,1),center=Math.min(width-1,Math.floor(8+(width-16)*percent/100));
      const alpha=pixels[(10*width+center)*4+3];assert.ok(alpha>0);
      assert.match(linked.template,new RegExp(`x: ${(width-16)*percent/100}; y: 8; width: 16; height: 16; radius: 8`));
    }finally{model.free();}
  }
});

test('catalog export captures the current session and keeps every section reachable through Scroll',()=>{
  const session=createCatalogSession(runtime);
  try {
    for(const action of ['check','protocol2','rangeMore','rangeMore','docClose0','tree2','target2','selectToggle','showDialog','format3'])session.dispatch(`actions.${action}`);
    const snapshot=session.state(),before=JSON.stringify(snapshot);
    const project=JSON.parse(JSON.stringify(catalogProject(files,'dark','touch',{state:snapshot,reducedMotion:true})));
    assert.equal(JSON.stringify(snapshot),before,'export must not mutate the live snapshot');
    const root=parse(project['ui/FormaControls.ui']).nodes[0];
    assert.deepEqual([root.props.width,root.props.height],[720,780]);
    assert.equal(root.children.length,1);assert.equal(root.children[0].type,'Scroll');
    const rows=root.children[0].children,byKey=key=>rows.find(node=>node.props.key===key);
    assert.equal(rows.filter(node=>/^heading\d+$/.test(node.props.key)).length,sections.length);
    assert.equal(byKey('check').props.checked,false);
    assert.equal(byKey('protocol2').props.checked,true);assert.equal(byKey('protocol1').props.checked,false);
    assert.equal(byKey('rangeCaption').props.text,'Параллельные соединения: 10');
    assert.equal(byKey('doc0').props.text,'lib.rs');
    assert.ok(!rows.some(node=>/^docDirty\d+$/.test(node.props.key)),'closing ui.rs removes its dirty indicator instead of moving it to lib.rs');
    assert.ok(byKey('tree2'));assert.equal(byKey('tree3'),undefined);assert.equal(byKey('tree4'),undefined);
    assert.equal(byKey('selectToggle').props.value,'Mobile');assert.equal(byKey('target2').props.selected,true);
    assert.ok(byKey('demoDialogConfirm'));assert.equal(byKey('showDialog'),undefined);
    assert.equal(byKey('format3').props.selected,true);
    const linked=compileComponents(project,'ui/FormaControls.ui',{}, {measureText:runtime.text_metrics});
    const model=new runtime.Runtime();
    try {
      model.load_component(linked.source,linked.template);
      assert.equal(model.scrollable(),true);assert.deepEqual(Array.from(model.viewport()),[0,0,720,780]);
      assert.equal(model.control_count(),rows.length);
      model.scroll(0,1_000_000);
      assert.ok(model.scroll_offset()[1]>0);
      const [,y,,height]=model.control_bounds(model.control_count()-1);
      assert.ok(y>=0&&y+height<=780,'the final exported control is reachable at the scroll end');
      assert.ok(model.gpu_commands(1.25,true).length>0);
    }finally{model.free();}
  }finally{session.destroy();}
});
