import {test} from 'node:test';
import assert from 'node:assert/strict';
import {compileComponents, createComponentCompiler, evaluate} from '../src/components.js';
import {parse} from '../src/language.js';

const button = `component Button {
 width: 100; height: 30; text: ''; color: #ffffff;
 Rectangle {
  background: props.background;
  Text { text: props.text; color: props.color; }
  PointerArea { clicked -> events.clicked(); }
 }
}`;
const compile = (body, state = {}, definitions = {}, declarations = '') => compileComponents({
 'ui/Demo.ui': `component Demo { ${declarations} Frame { width: 320; height: 180; ${body} } }`,
 'components/Button.ui': button,
 ...definitions,
}, 'ui/Demo.ui', state, {measureText: (text, size) => [text.length * size / 2, size]});

test('standalone text primitives work without component files and cached project overrides take precedence', () => {
 const files = {'ui/Demo.ui': `component Demo { Column {
  Text { text: 'Count: \${state.count}'; }
  TextInput { value <-> state.query; placeholder: 'Search'; }
 } }`};
 const compiler = createComponentCompiler();
 const first = compiler(files, 'ui/Demo.ui', {count: 2, query: 'Q'});
 assert.match(first.template, /ContentText .*text: 'Count: 2'/);
 assert.match(first.template, /ContentInput .*value: 'Q'/);
 assert.equal(first.previewControls[1].bindings.value, 'state.query');
 const custom = {...files, 'components/Text.ui': `component Text {
  width: 260; height: 30; text: '';
  Rectangle { Text { text: 'Custom: \${props.text}'; } }
 }`};
 assert.match(compiler(custom, 'ui/Demo.ui', {count: 3, query: ''}).template, /Custom: Count: 3/);
 assert.doesNotMatch(compiler(files, 'ui/Demo.ui', {count: 4, query: ''}).template, /Custom:/);
});

test('typed contracts validate required fields, enums, optionals and inherited overrides', () => {
 const definitions = {'components/Notice.ui': `component Notice : Button {
  enum Tone { Accent, Danger }
  required prop title: String;
  prop tone: Tone = Tone.Accent;
  prop subtitle: String?;
  text: props.title;
  match props.tone {
   Tone.Danger => { background: #ff0000; color: #ffffff; }
   _ => { background: #0000ff; color: #eeeeee; }
  }
 }`};
 const out = compile("Notice { title: 'Ошибка'; tone: Tone.Danger; color: #00ff00; }", {}, definitions);
 assert.match(out.source, /text: 'Ошибка'/);
 assert.match(out.source, /background: #ff0000/);
 assert.match(out.source, /color: #00ff00/);
 assert.throws(() => compile('Notice {}', {}, definitions), /обязательное свойство title/);
 assert.throws(() => compile('Notice { title: 1; }', {}, definitions), /ожидался String/);
 assert.throws(() => compile("Notice { title: 'X'; tone: Tone.Missing; }", {}, definitions), /Неизвестный вариант/);
 assert.throws(() => compile("Notice { title: 'X'; tone: 'Danger'; }", {}, definitions), /ожидался Tone/);
});

test('grouped match diagnoses conflicting groups and missing branches', () => {
 assert.throws(() => compile('Bad {}', {}, {'components/Bad.ui': `component Bad : Button {
  match true { true => { color: #ffffff; } }
  match true { true => { color: #000000; } }
 }`}), /Несколько групп match задают color/);
 assert.throws(() => compile('Bad {}', {}, {'components/Bad.ui': `component Bad : Button {
  match false { true => { color: #ffffff; } }
 }`}), /нет подходящей ветки/);
});

test('forward is explicit, type checked, and local properties override forwarding', () => {
 const defs = {
  'components/Label.ui': `component Label { prop text: String = ''; color: #ffffff; Text { text: props.text; color: props.color; } }`,
  'components/Host.ui': `component Host : Button {
   override content: Label {};
  }`,
  'components/Button.ui': `component Button { width: 100; height: 30; text: 'Hello'; color: #ffffff;
   Rectangle { ContentPresenter { key: 'content'; Label { forward props { text, color }; color: #ff0000; } } }
  }`,
 };
 const out = compile('Button {}', {}, defs);
 assert.match(out.template, /text: 'Hello'/);
 assert.match(out.template, /color: #ff0000/);
 const bad = {...defs, 'components/Button.ui': defs['components/Button.ui'].replace('text, color', 'missing')};
 assert.throws(() => compile('Button {}', {}, bad), /forward: неизвестное свойство/);
});

test('expressions and both quote styles re-evaluate without evaluating unused branches', () => {
 const body = `Button { text: 'Найдено: \${state.count}'; disabled: state.busy || !state.valid; width: clamp(state.width, 40, 120); }
 Button { text: "Найдено: \${state.count}"; }`;
 const a = compile(body, {count: 2, busy: false, valid: true, width: 200});
 assert.match(a.source, /Найдено: 2/);
 assert.match(a.source, /width: 120/);
 assert.match(a.source, /disabled: false/);
 const b = compile(body, {count: 3, busy: true, width: 20});
 assert.match(b.source, /Найдено: 3/);
 assert.match(b.source, /width: 40/);
 assert.match(b.source, /disabled: true/);
 const value = parse(`component A { text: state.missing?.title ?? 'Нет'; }`).defaults.text;
 assert.equal(evaluate(value, {}, {}), 'Нет');
});

test('keyed lists preserve identity on reorder, expand empty/if branches and evaluate event arguments', () => {
 const body = `Column { gap: 4;
  if state.loading { Button { text: 'Loading'; } } else { Button { text: 'Ready'; } }
  for item in state.items key item.id {
   Button { text: item.title; clicked -> events.open(item.id); }
  } empty { Button { text: 'Empty'; } }
 }`;
 const items = [{id: 'a', title: 'A'}, {id: 'b', title: 'B'}];
 const first = compile(body, {loading: false, items}, {}, 'event open(id: String);');
 const second = compile(body, {loading: true, items: [...items].reverse()}, {}, 'event open(id: String);');
 const controls = out => out.previewControls.filter(node => node.props.text === 'A' || node.props.text === 'B');
 assert.deepEqual(controls(first).map(node => node.props.key), controls(second).map(node => node.props.key).reverse());
 assert.deepEqual(controls(second).map(node => node.eventArgs.clicked), [['b'], ['a']]);
 assert.match(compile(body, {loading: false, items: []}, {}, 'event open(id: String);').source, /Empty/);
 assert.throws(() => compile(body, {loading: false, items: [items[0], items[0]]}, {}, 'event open(id: String);'), /Повторный ключ for/);
 assert.throws(() => compile(body, {loading: false, items}, {}, 'event open(id: Int);'), /ожидался Int/);
});

test('Row closes optional gaps, Grid auto-places children, dimensions respect constraints', () => {
 const body = `Row { gap: 8; height: 30;
  if state.icon { Button { width: 20; text: 'Icon'; } }
  Button { width: *; minWidth: 40; maxWidth: 180; text: 'Text'; }
 }`;
 const visible = compile(body, {icon: true}), hidden = compile(body, {icon: false});
 assert.equal(visible.previewControls[1].props.x, 28);
 assert.equal(hidden.previewControls[0].props.x, 0);
 assert.equal(hidden.previewControls[0].props.width, 180);
 const grid = compile(`Grid { columns: [100, *]; gap: 4;
  Button { text: '1'; } Button { text: '2'; } Button { text: '3'; }
 }`);
 assert.deepEqual(grid.previewControls.map(node => [node.props.x, node.props.y]), [[0, 0], [104, 0], [0, 34]]);
 assert.throws(() => compile('Button { minWidth: 100; maxWidth: 20; }'), /minWidth превышает maxWidth/);
});

test('two-way binding is evaluated for a Studio snapshot and preserved as editable metadata', () => {
 const out = compile('Button { text: state.name; checked <-> state.checked; }', {name: 'N', checked: true}, {
  'components/Button.ui': button.replace("text: '';", "text: ''; checked: false;"),
 });
 assert.equal(out.previewControls[0].props.checked, true);
 assert.equal(out.previewControls[0].bindings.checked, 'state.checked');
 assert.doesNotMatch(out.source, /<->/);
});

test('root contracts and grouped defaults are evaluated before the scene',()=>{
 const files={'components/Button.ui':button,'ui/Demo.ui':`component Demo {
  required prop title: String;
  prop compact: Bool = false;
  match props.compact { true => { caption: 'Short'; } _ => { caption: props.title; } }
  Frame { Button { text: props.caption; } }
 }`};
 assert.throws(()=>compileComponents(files,'ui/Demo.ui'),/обязательное свойство title/);
 const out=compileComponents(files,'ui/Demo.ui',{}, {properties:{title:'Full'}});
 assert.match(out.source,/text: 'Full'/);
 const compact=compileComponents(files,'ui/Demo.ui',{}, {properties:{title:'Full',compact:true}});
 assert.match(compact.source,/text: 'Short'/);
});

test('selected grouped property origins point to their actual source branch',()=>{
 const code=`component Button { width:100; height:30; Rectangle {
  Text { match state.error { true => { text: 'Error'; } _ => { text: 'OK'; } } }
 } }`;
 const out=compile('Button {}',{error:true},{'components/Button.ui':code});
 const text=out.templateTree.nodes.find(node=>node.type==='Text');
 const source=text.propertySources.text;
 assert.equal(source.file,'components/Button.ui');
 assert.equal(code.slice(source.from,source.to),"'Error'");
});
