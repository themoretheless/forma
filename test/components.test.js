import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compileComponents as compileLinked,evaluate} from '../src/components.js';
// Compiler structure tests supply explicit deterministic metrics; integration
// tests below use the actual Rust font. Production has no guessed-width fallback.
const compileComponents=(files,entry,state)=>compileLinked(files,entry,state,{measureText:()=>[80,20]});
import {parse} from '../src/language.js';
import {svgShapes} from '../src/svg-shapes.js';
import {parser} from '../src/forma-parser.js';

const base=`component Button {
 width: 200; height: 50; text: 'Base'; color: #ffffff; compact: false;
 Rectangle {
 background: Brush { color: #123456; hover: #abcdef; transition: 140ms; };
 Frame { ContentPresenter { key: 'content'; Text { text: props.text; color: props.color; } } }
 Frame { ContentPresenter { key: 'badge'; Text { text: 'badge'; } } }
 PointerArea { clicked -> events.clicked(); }
 }
}`;
const derived=`component ImageButton : Button {
 icon: 'assets/search.svg';
 override content: match props.compact {
 true => Image { source: props.icon; width: 20; height: 20; };
 false => Frame { columns: [20, -]; gap: 8;
 Image { cell: 1 1; source: props.icon; width: 20; height: 20; }
 Text { cell: 1 2; text: props.text; }
 };
 };
}`;
const svg='<svg viewBox="0 0 24 24"><circle cx="10" cy="10" r="7" fill="none" stroke="currentColor" stroke-width="2"/><line x1="15" y1="15" x2="22" y2="22" stroke="#ffffff" stroke-width="2"/></svg>';
const files={'components/Button.ui':base,'components/ImageButton.ui':derived,'ui/Demo.ui':`component Demo { Frame { ImageButton { text: 'Search'; clicked -> actions.search(); } } }`,'assets/search.svg':svg};
test('inheritance, slot override and sibling frames survive compilation',()=>{
 const out=compileComponents(files,'ui/Demo.ui');assert.match(out.source,/Button \{/);assert.match(out.source,/actions.search/);assert.match(out.template,/ContentShape/);assert.match(out.template,/Search/);assert.match(out.template,/badge/);assert.match(out.template,/#123456/);assert.match(out.template,/events.clicked/);
});
test('conditional replacement and tuple match',()=>{
 const f={...files,'ui/Demo.ui':files['ui/Demo.ui'].replace("text: 'Search';","text: 'Search'; compact: true;")};const out=compileComponents(f,'ui/Demo.ui');assert.doesNotMatch(out.template,/text: 'Search'/);assert.match(out.template,/badge/);
 const p=parse("component Demo { padding: match (state.compact, state.loading) { (true, _) => 8; (_, false) => 16; _ => 24; }; }");assert.equal(evaluate(p.defaults.padding,{}, {compact:true,loading:false}),8);assert.equal(evaluate(p.defaults.padding,{}, {compact:false,loading:false}),16);
});
test('base.content means the immediate base and never self-recurses',()=>{
 const f={...files,'components/Middle.ui':"component Middle : Button { override content: Text { text: 'Middle'; }; }",'components/ImageButton.ui':"component ImageButton : Middle { override content: match state.loading { true => Text { text: 'Loading'; }; false => base.content; }; }"};
 assert.match(compileComponents(f,'ui/Demo.ui',{loading:false}).template,/Middle/);assert.match(compileComponents(f,'ui/Demo.ui',{loading:true}).template,/Loading/);
});
test('unknown slots, inherited tree edits, cycles, missing branches and props fail',()=>{
 for(const source of ["component ImageButton : Button { override absent: Text {}; }","component ImageButton : Button { Frame {} }","component ImageButton : ImageButton {}","component ImageButton : Button { override content: base.badge; }","component ImageButton : Button { override content: match props.compact { true => Text {}; }; }","component ImageButton : Button { override content: Text { text: props.missing; }; }"]){assert.throws(()=>compileComponents({...files,'components/ImageButton.ui':source},'ui/Demo.ui'),undefined,source);}
 assert.throws(()=>compileComponents({...files,'components/ImageButton.ui':"component ImageButton : Button { override content: match state.loading { true => Text {}; }; }"},'ui/Demo.ui'),/state.loading/);
});
test('instance overrides base and derived defaults including false and zero',()=>{
 const f={...files,'components/ImageButton.ui':"component ImageButton : Button { width: 180; disabled: true; }",'ui/Demo.ui':"component Demo { Frame { ImageButton { width: 90; disabled: false; borderWidth: 0; } } }"};const out=compileComponents(f,'ui/Demo.ui');assert.match(out.source,/width: 90;/);assert.match(out.source,/disabled: false;/);assert.match(out.source,/borderWidth: 0;/);
});
test('editor grammar recognizes inheritance and override match without errors',()=>{
 const errors=[];parser.parse(derived).iterate({enter(n){if(n.type.isError)errors.push(n.from);}});assert.deepEqual(errors,[]);
});
test('nested Frame clip is emitted around its children',()=>{
 const f={...files,'components/ImageButton.ui':"component ImageButton : Button { override content: Frame { radius: 12; clip: true; Text { text: 'Clipped'; } }; }"};const out=compileComponents(f,'ui/Demo.ui');assert.match(out.template,/ContentClip \{[^}]*radius: 12;/);assert.match(out.template,/Clipped[^]*ContentClipEnd/);
 assert.throws(()=>compileComponents({...f,'components/ImageButton.ui':f['components/ImageButton.ui'].replace('clip: true','clip: 1')},'ui/Demo.ui'),/boolean/);
});
test('SVG subset is vector geometry and rejects scripts, external references and unsupported shapes',()=>{
 const shapes=svgShapes(svg,[10,20,24,24]);assert.ok(shapes.length>10);assert.ok(shapes.every(s=>s.points.every(p=>p.every(Number.isFinite))));
 for(const s of ['<svg viewBox="0 0 24 24"><script/></svg>','<svg viewBox="0 0 24 24"><image href="https://x"/></svg>','<svg viewBox="0 0 24 24"><path d="M0 0L1 1"/></svg>','<svg viewBox="0 0 24 24"><circle r="4" transform="scale(2)"/></svg>'])assert.throws(()=>svgShapes(s,[0,0,24,24]));
});
