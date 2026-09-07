import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { compileComponents } from '../src/components.js';
import { themes, sizes, themeState, materializeTheme, contrastRatio } from '../vector-ui/controls/tokens.js';
import { sections, sectionSource, specimens, catalogProject } from '../vector-ui/controls/catalog.js';
import init, { Runtime, text_metrics } from '../public/vector-pkg/forma.js';
await init({ module_or_path: readFileSync(new URL('../public/vector-pkg/forma_bg.wasm', import.meta.url)) });
const files = {};
for (const dir of ['components','assets']) for (const file of readdirSync(new URL(`../vector-ui/controls/${dir}/`, import.meta.url))) {
  files[`${dir}/${file}`] = readFileSync(new URL(`../vector-ui/controls/${dir}/${file}`, import.meta.url), 'utf8');
}
const metrics = { measureText: (text, size) => text_metrics(text, size) };
const load = (project, path) => { const linked = compileComponents(project, path, {}, metrics); const model = new Runtime(); model.load_component(linked.source, linked.template); return { linked, model }; };

test('action text stays readable across light/dark hover and pressed states', () => {
  assert.ok(contrastRatio('#FFFFFF', '#DE6A19') < 4.5, 'reference needs adaptation');
  for (const theme of Object.values(themes)) {
    for (const family of ['primary','danger']) for (const state of ['', 'Hover','Pressed']) {
      assert.ok(contrastRatio(theme[`${family}Text`], theme[`${family}${state}`]) >= 4.5, `${family}${state}`);
    }
    for (const surface of ['canvas','surface','surfaceRaised','surfaceHover','surfacePressed']) {
      assert.ok(contrastRatio(theme.textMuted, theme[surface]) >= 4.5, `muted/${surface}`);
    }
    for (const [ink, wash] of [['accentInk','accentWash'],['success','successWash'],['warning','warningWash'],['dangerInk','dangerWash']]) {
      assert.ok(contrastRatio(theme[ink], theme[wash]) >= 4.5, `${ink}/${wash}`);
    }
  }
});

test('every specimen compiles into the same WASM runtime in every theme and size', () => {
  for (const theme of Object.keys(themes)) for (const size of Object.keys(sizes)) for (const section of sections) {
    const source = sectionSource(section.id, theme, size);
    const project = { ...materializeTheme(files, theme), 'ui/Test.ui': source };
    const { model } = load(project, 'ui/Test.ui');
    try {
      assert.equal(model.control_count(), specimens(section.id, size).length);
      assert.equal(model.render_width(), 320);
      assert.equal(model.render_height(), section.height, `${section.id} cannot overflow the artboard`);
      for (const scale of [1,1.25,2]) assert.ok(model.gpu_commands(scale, true).length > 0);
    } finally { model.free(); }
  }
});

test('composed icon button keeps its own source parts and real click interaction', () => {
  const project = catalogProject(files);
  const { linked, model } = load(project, 'ui/FormaControls.ui');
  try {
    const sources = new Set(linked.templateTree.nodes.map(n => n.source?.file));
    for (const name of ['Surface','Icon','Label','IconLabel','MessageContent']) assert.ok(sources.has(`components/${name}.ui`), name);
    const index = Array.from({length:model.control_count()}, (_,i)=>i).find(i=>model.control_key(i)==='create');
    const [x,y,w,h] = model.control_bounds(index);
    model.pointer(x+w/2,y+h/2,1); model.pointer(x+w/2,y+h/2,2);
    assert.equal(model.control_clicks(index), 1);
    assert.equal(model.control_action(index), 'actions.create');
    const icon = Array.from({length:model.control_count()}, (_,i)=>i).find(i=>model.control_key(i)==='searchIcon');
    assert.equal(model.control_label(icon), 'Поиск', 'icon-only action retains an accessible name');
    const labels=[];
    model.focus(false);
    while (model.focus_next(false)) labels.push(model.control_key(model.focused_index()));
    assert.ok(!labels.includes('disabled'));
    assert.ok(!labels.includes('neutralBadge'));
    assert.ok(!labels.includes('progress60'));
    assert.ok(!labels.includes('savedNotice'));
  } finally { model.free(); }
});

test('theme exports are standalone and reduced motion settles colors immediately', () => {
  const project = catalogProject(files, 'dark', 'touch', {reducedMotion:true});
  assert.ok(!Object.values(project).some(source=>source.includes('state.theme.') || source.includes('state.motion.')));
  const { model } = load(project, 'ui/FormaControls.ui');
  try {
    const [x,y,w,h]=model.control_bounds(1);
    model.pointer(x+w/2,y+h/2,0);
    assert.equal(model.is_animating(),false);
  } finally { model.free(); }
  assert.throws(()=>themeState('missing'),/Unknown/);
});

test('theme materialization preserves strings and comments while resolving whole expressions', () => {
  const source = String.raw`component Caption {
    // state.theme.missing
    /* state.motion.missing /* state.theme.text */ state.theme.alsoMissing */
    text: 'state.theme.text \'state.motion.control\'';
    quoted: "state.theme.text \"state.motion.control\"";
    color: state.theme.text;
    transitionDuration: state.motion.control;
  }`;
  const result = materializeTheme({'components/Caption.ui':source, 'notes.txt':source}, 'dark', {reducedMotion:true});
  assert.equal(result['components/Caption.ui'], source
    .replace('color: state.theme.text;', `color: ${themes.dark.text};`)
    .replace('transitionDuration: state.motion.control;', 'transitionDuration: 0ms;'));
  assert.equal(result['notes.txt'],source);
  for (const reference of ['state.theme.missing','state.theme.textExtra','state.theme.constructor']) {
    assert.throws(()=>materializeTheme({'components/Bad.ui':`component Bad { color:${reference}; }`}),/Unknown theme constant/);
  }
});

test('progress uses a filled primitive with the supplied value, including empty/full endpoints', () => {
  const themed=materializeTheme(files);
  for (const [value,expected] of [['0%',0],['60%',120],['100%',200]]) {
    const source=`component Test { Frame { width:200; height:20; padding:0; ProgressBar {width:200; value:${value};} } }`;
    const { linked,model }=load({...themed,'ui/Test.ui':source},'ui/Test.ui');
    try {
      const rect = linked.templateTree.nodes.find(n=>n.type==='Rectangle'&&n.props.background?.expr===themes.light.primary);
      assert.ok(rect);
      const pixels=model.pixels(200,20,1);
      const at=x=>Array.from(pixels.slice((4*200+x)*4,(4*200+x)*4+3));
      const primary=themes.light.primary.match(/[0-9A-Fa-f]{2}/g).map(v=>parseInt(v,16));
      if (expected>10) assert.deepEqual(at(8),primary);
      if (expected<190) assert.notDeepEqual(at(190),primary);
    } finally {model.free();}
  }
});

test('checkbox and radio highlight only the indicator while the label remains clickable', () => {
  for (const theme of ['light','dark']) for (const type of ['Checkbox','RadioButton']) for (const disabled of [false,true]) {
    const source=`component Demo { Frame { width:240; height:40; ${type} { key:'choice'; width:240; height:40; text:'Label'; disabled:${disabled}; clicked -> actions.choose(); } } }`;
    const {model}=load({...materializeTheme(files,theme),'ui/Test.ui':source},'ui/Test.ui');
    try {
      model.set_reduced_motion(true);
      const before=model.content_pixels(240,40,1);
      model.pointer(180,20,1); model.pointer(180,20,2);
      const after=model.content_pixels(240,40,1);
      let changed=0;
      for(let y=0;y<40;y++)for(let x=0;x<240;x++) {
        const i=(y*240+x)*4;
        if(before.slice(i,i+4).some((v,k)=>v!==after[i+k])) {
          changed++;
          assert.ok(x>=9&&x<=28&&y>=10&&y<=29,`${type}: changed outside indicator at ${x},${y}`);
        }
      }
      assert.equal(model.control_clicks(0),disabled?0:1);
      assert.equal(changed>0,!disabled,'focus must illuminate the indicator, disabled stays unchanged');
    } finally {model.free();}
  }
});
