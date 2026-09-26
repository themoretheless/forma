import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse,designStatePatch} from '../src/language.js';
import {setDesignData,designReferencesInFiles} from '../src/design-data.js';
import {findEntry} from '../src/design-states.js';
import {propertyFields,createPropertyInspector} from '../src/property-inspector.js';
import {pickerHex} from '../src/color-values.js';
import {valueTextEdit,statementTextEdit,propertyAddEdit,propertyRemoveEdit} from '../src/property-edit.js';
import {addableProperties,propertyStart} from '../src/property-catalog.js';

const source=`component SearchWindow {
    Frame {
        Text {
            color: #e8edf7;
            background: '#98a4ba';
            font.size: design.titleSize;
            text: 'Библиотека знаний'; /* подпись */
        }
        TextInput {
            value <-> state.query;
            placeholder: 'Что найти?';
        }
        Button {
            clicked -> actions.search('docs');
            disabled: state.loading;
            opacity: 0.8;
        }
    }
}`;
const nodeOf=type=>{let found;const walk=nodes=>{for(const n of nodes){found??=n.type===type?n:null;walk(n.children);}};walk(parse(source).nodes);return found;};
const byKey=fields=>Object.fromEntries(fields.map(f=>[f.key,f]));

test('a color field is recognised in both quoted and bare form',()=>{
  const fields=byKey(propertyFields(nodeOf('Text'),source));
  assert.equal(fields.color.kind,'color');
  assert.equal(fields.color.value,'#e8edf7');
  assert.equal(fields.color.css,'#e8edf7');
  assert.equal(fields.background.kind,'color');
  assert.equal(fields.background.value,'#98a4ba');
  // Only the six-digit form is losslessly editable by the native picker.
  assert.equal(pickerHex('#e8edf7'),'#e8edf7');
  assert.equal(pickerHex('  #ABC  '),'#aabbcc');
  assert.equal(pickerHex('#00000080'),null);
  assert.equal(pickerHex('argb(255, 1, 2, 3)'),null);
});
test('a design token field keeps its reference text and commits it as markup',()=>{
  setDesignData({'design.titleSize':'26'});
  const field=byKey(propertyFields(nodeOf('Text'),source))['font.size'];
  assert.equal(field.kind,'token');
  assert.equal(field.ref,'design.titleSize');
  assert.equal(field.text,'design.titleSize');
  assert.equal(field.commit,'value');
});
test('expressions and literals are told apart, and the value text round-trips',()=>{
  const text=byKey(propertyFields(nodeOf('Text'),source));
  assert.equal(text.text.kind,'string');
  assert.equal(text.text.value,'Библиотека знаний');
  const button=byKey(propertyFields(nodeOf('Button'),source));
  assert.equal(button.disabled.kind,'expression');
  assert.equal(button.disabled.text,'state.loading');
  assert.equal(button.opacity.kind,'number');
  assert.equal(button.opacity.value,0.8);
});
test('bindings and handlers are fields of their own, not values',()=>{
  // A preview node carries its bound name back as a computed prop the definition has no
  // range for; the statement row is the one that writes it, so the frozen twin is dropped.
  const bound=nodeOf('TextInput');
  const input=propertyFields({...bound,props:{...bound.props,value:'Что найти?'}},source);
  assert.deepEqual(input.filter(field=>field.key==='value').map(f=>f.kind),['binding']);
  const view=byKey(input);
  assert.equal(view.value.op,'<->');
  assert.equal(view.value.text,'state.query');
  assert.equal(view.value.commit,'statement');
  const button=byKey(propertyFields(nodeOf('Button'),source));
  assert.equal(button.clicked.kind,'handler');
  assert.equal(button.clicked.text,"actions.search('docs')");
});
test('a token catalogue lists references from markup files only',()=>{
  assert.deepEqual(designReferencesInFiles({'a.ui':'x: design.STATUS; y: design.a() ; z: design.STATUS;','b.design.ui':'w: design.other;','src/design.rs':'design::IGNORED'}),
    ['design.STATUS','design.a()','design.other']);
  // The design attribute and the .design.ui suffix name a file, so they are not tokens.
  assert.deepEqual(designReferencesInFiles({'ui/SearchWindow.ui':"#[design('./SearchWindow.design.ui')]\ncomponent SearchWindow {\n    Text { text: design.STATUS; }\n}"}),['design.STATUS']);
  assert.deepEqual(designReferencesInFiles({'a.ui':'x: design.text.title; y: design.call(1);'}),[]);
});
test('editing a value text replaces only that value and keeps comments',()=>{
  const edit=valueTextEdit(source,nodeOf('Text').start,'text',"'Новое название'");
  const next=source.slice(0,edit.from)+edit.insert+source.slice(edit.to);
  assert.ok(next.includes('/* подпись */'));
  assert.ok(next.includes('color: #e8edf7;'));
  assert.equal(parse(next).nodes[0].children[0].props.text,'Новое название');
  assert.equal(parse(next).nodes[0].children[0].props.color.expr,'#e8edf7');
});
test('a value must be one complete value, not a snippet that reopens the node',()=>{
  assert.throws(()=>valueTextEdit(source,nodeOf('Text').start,'text',"'x' } Button { text: 'y"),/не разобрано/);
  assert.throws(()=>valueTextEdit(source,nodeOf('Text').start,'text',''),/Пустое значение/);
  assert.throws(()=>valueTextEdit(source,nodeOf('Text').start,'missing','1'),/изменилось/);
});
test('a binding or handler rewrite replaces the whole statement',()=>{
  const binding=statementTextEdit(source,nodeOf('TextInput').start,'value','state.filters.query');
  const next=source.slice(0,binding.from)+binding.insert+source.slice(binding.to);
  assert.ok(next.includes('value <-> state.filters.query;'));
  assert.ok(!next.includes('value <-> state.query;'));
  const handler=statementTextEdit(source,nodeOf('Button').start,'clicked',"actions.search('all')");
  assert.equal(handler.insert,"clicked -> actions.search('all');");
});
test('binding paths and handlers are validated before the source changes',()=>{
  assert.throws(()=>statementTextEdit(source,nodeOf('TextInput').start,'value','state'),/путь к полю/);
  assert.throws(()=>statementTextEdit(source,nodeOf('Button').start,'clicked','backend.search()'),/actions/);
  assert.throws(()=>statementTextEdit(source,nodeOf('Text').start,'color','state.accent'),/Оператор/);
});
const apply=(src,edit)=>src.slice(0,edit.from)+edit.insert+src.slice(edit.to);
const toolbar=`component Toolbar {
    Row {
        IconButton { icon: 'assets/plus.svg'; clicked -> actions.add(); }
        Button {}
    }
}`;
const toolbarNode=type=>{let found;const walk=nodes=>{for(const n of nodes){found??=n.type===type?n:null;walk(n.children);}};walk(parse(toolbar).nodes);return found;};

test('adding a property writes it after the last declaration of that node',()=>{
  const next=apply(source,propertyAddEdit(source,nodeOf('Text').start,'width','240'));
  // The trailing comment belongs to the statement above, so the new line goes after it.
  assert.ok(next.includes("text: 'Библиотека знаний'; /* подпись */\n            width: 240;\n        }"));
  const text=parse(next).nodes[0].children[0];
  assert.ok('width'in text.props);
  assert.equal(text.props.color.expr,'#e8edf7');
  assert.equal(text.props.text,'Библиотека знаний');
});
test('a dotted property name is written the way markup spells it',()=>{
  const next=apply(source,propertyAddEdit(source,nodeOf('Text').start,'line.height','18'));
  assert.ok(next.includes('\n            line.height: 18;'));
  assert.ok('line.height'in parse(next).nodes[0].children[0].props);
});
test('an empty block gets the statement and its closing brace on their own lines',()=>{
  const next=apply(toolbar,propertyAddEdit(toolbar,toolbarNode('Button').start,'width','240'));
  assert.ok(next.includes('        Button {\n            width: 240;\n        }\n    }'));
});
test('an inline block keeps its statements and gains the new one without trailing spaces',()=>{
  const next=apply(toolbar,propertyAddEdit(toolbar,toolbarNode('IconButton').start,'width','240'));
  assert.ok(next.includes("        IconButton { icon: 'assets/plus.svg'; clicked -> actions.add();\n            width: 240;\n        }"));
  assert.ok(!/[ \t]+$/m.test(next));
  assert.equal(parse(next).nodes[0].children[0].props.icon,'assets/plus.svg');
});
test('an added property cannot duplicate a name, smuggle markup or come from a stale node',()=>{
  const at=nodeOf('Text').start;
  assert.throws(()=>propertyAddEdit(source,at,'text',"'x'"),/уже объявлено/);
  assert.throws(()=>propertyAddEdit(source,at,'a..b','1'),/Некорректное имя/);
  assert.throws(()=>propertyAddEdit(source,at,'width','} Button {'),/не разобрано/);
  assert.throws(()=>propertyAddEdit(source,at,'width','   '),/Пустое значение/);
  assert.throws(()=>propertyAddEdit(source,at+7,'width','#ffffff'),/выберите элемент заново/);
});
test('removing a property or a binding cuts the whole line it owns',()=>{
  const withoutBackground=apply(source,propertyRemoveEdit(source,nodeOf('Text').start,'background'));
  assert.ok(!withoutBackground.includes('background:'));
  assert.ok(withoutBackground.includes("text: 'Библиотека знаний'; /* подпись */"));
  assert.ok('color'in parse(withoutBackground).nodes[0].children[0].props);
  const withoutBinding=apply(source,propertyRemoveEdit(source,nodeOf('TextInput').start,'value'));
  assert.ok(!withoutBinding.includes('<->'));
  assert.ok(withoutBinding.includes("placeholder: 'Что найти?';"));
});
test('a statement that shares its line stays with the source editor',()=>{
  assert.throws(()=>propertyRemoveEdit(toolbar,toolbarNode('IconButton').start,'icon'),/не на отдельной строке/);
  assert.throws(()=>propertyRemoveEdit(source,nodeOf('Text').start,'gap'),/не объявлено/);
  assert.throws(()=>propertyRemoveEdit(source,nodeOf('Text').start+7,'color'),/выберите элемент заново/);
});
test('adding then removing leaves every other declaration of the block intact',()=>{
  const at=toolbarNode('IconButton').start;
  const added=apply(toolbar,propertyAddEdit(toolbar,at,'width','240'));
  const back=apply(added,propertyRemoveEdit(added,at,'width'));
  assert.ok(!back.includes('width:'));
  assert.equal(parse(back).nodes[0].children[0].props.icon,'assets/plus.svg');
  assert.equal(parse(back).nodes[0].children[0].events.clicked,'actions.add');
});
test('the catalogue offers each name with a start value the compiler accepts',()=>{
  const declared=Object.keys(nodeOf('Button').props);
  const names=addableProperties('Button',declared);
  assert.ok(names.includes('width'));
  assert.ok(!names.includes('disabled'),'a declared property is not offered');
  assert.ok(!names.some(name=>name.includes('.')),'dotted aliases stay out of the list');
  for(const name of names){
    const next=apply(source,propertyAddEdit(source,nodeOf('Button').start,name,propertyStart[name]));
    assert.ok(name in parse(next).nodes[0].children[2].props,`${name} did not land`);
  }
});
// A tiny DOM: enough for the panel to build rows, find them back and run their handlers.
const matches=(el,selector)=>{
  const parts=/^([a-z]*)((?:\.[\w-]+)*)(?:\[data-(\w+)=(?:"([^"]*)"|([^\]]*))\])?$/.exec(selector);
  if(!parts)throw Error(`shim: unknown selector ${selector}`);
  const [,tag,classes,attr,a,b]=parts;
  if(tag&&el.tagName!==tag)return false;
  const own=new Set(String(el.className).split(/\s+/).filter(Boolean));
  for(const name of classes.split('.').filter(Boolean))if(!own.has(name))return false;
  return attr===undefined||String(el.dataset[attr]??'')===String(a??b??'');
};
class El{
  constructor(tag){this.tagName=tag;this.children=[];this.dataset={};this.attrs=new Map();this.className='';this.style={};this.validity='';}
  setAttribute(key,value){this.attrs.set(key,String(value));}
  getAttribute(key){return this.attrs.get(key)??null;}
  append(...children){for(const child of children){child.parent=this;this.children.push(child);if(this.tagName==='select'&&this.value===undefined&&child.value!==undefined)this.value=child.value;}}
  replaceChildren(...children){this.children=[];this.append(...children);}
  *walk(){for(const child of this.children){yield child;yield*child.walk();}}
  querySelector(selector){for(const el of this.walk())if(matches(el,selector))return el;return null;}
  querySelectorAll(selector){return [...this.walk()].filter(el=>matches(el,selector));}
  setCustomValidity(message){this.validity=message;}
  reportValidity(){}
  focus(){}
}
class Option extends El{constructor(text,value){super('option');this.value=value??text;this.textContent=text;}}
const panelFile='ui/Panel.ui';
const panelSource=`component Panel {
    Frame {
        gap: 8;
        padding: 12;
    }
}`;
// A component whose controls the design file addresses by key, and a design file that patches each
// of its three layers: the base block overrides `gap` and adds a background markup never declares,
// the state overrides two more frame properties, and one entry overrides a single title property.
const keyedFile='ui/Keyed.ui';
const keyedSource=`component Panel {
    Frame {
        key: 'frame';
        width: 320;
        gap: 8;
        padding: 12;
        opacity: 0.9;
        Text { key: 'title'; text: 'Заголовок'; }
    }
}`;
const designFile='ui/Keyed.design.ui';
const designSource=`design Panel {
    Frame {
        key: 'frame';
        gap: 6;
        background: '#111318';
    }

    state 'узкий' {
        Frame {
            key: 'frame';
            padding: 4;
            opacity: 0.5;
        }
        Text { key: 'title'; text: 'Уже'; }
    }
}`;
// The host of Studio keeps the editor document ahead of `files`; a test host does the same.
function mountPanel({source=panelSource,liveSource=true,node=null,design=null,file=panelFile}={}){
  const files={[file]:source,...(design?{[designFile]:design.source}:{})};
  const container=new El('div');
  const document={createElement:tag=>new El(tag)};
  const saved=Object.getOwnPropertyDescriptor(globalThis,'document');
  Object.defineProperty(globalThis,'document',{value:document,configurable:true});
  const savedOption=Object.getOwnPropertyDescriptor(globalThis,'Option');
  Object.defineProperty(globalThis,'Option',{value:Option,configurable:true});
  const inspector=createPropertyInspector({container,designTokens:()=>[],liveSource:liveSource?path=>files[path]:null,
    commit:({file:path,source:text,from,to,insert})=>{
      if(files[path]!==text)throw Error('Исходник изменился — повторите операции');
      const next=text.slice(0,from)+insert+text.slice(to);
      parse(next);
      files[path]=next;
    }});
  const targetNode=node??parse(files[file]).nodes[0];
  // The host resolves the two design entries the panel shows rows for; a state that is not on screen
  // gives no layer at all, which is how the panel behaves in the original scenario.
  const state=design?{name:design.name,path:designFile,source:files[designFile],
    entry:findEntry(files[designFile],design.name,targetNode.props.key),
    base:findEntry(files[designFile],null,targetNode.props.key)}:null;
  inspector.render({node:targetNode,path:file,source:files[file],editable:true,state});
  return {container,files,node:targetNode,state,
    panel:name=>container.querySelector(`input[data-prop=${name}]`),
    rowOf:name=>[...container.querySelectorAll('label.property')].find(row=>row.querySelector('span')?.textContent===name),
    restore(){
      if(saved)Object.defineProperty(globalThis,'document',saved);else delete globalThis.document;
      if(savedOption)Object.defineProperty(globalThis,'Option',savedOption);else delete globalThis.Option;
    }};
}
const mountKeyed=(which='frame')=>mountPanel({file:keyedFile,source:keyedSource,
  node:which==='frame'?parse(keyedSource).nodes[0]:parse(keyedSource).nodes[0].children[0],
  design:{name:'узкий',source:designSource}});
test('a row writes the file as the editor holds it, so one selection serves several edits',t=>{
  const {container,files,panel,restore}=mountPanel();
  t.after(restore);
  const padding=panel('padding'),gap=panel('gap');
  padding.value='13';padding.onchange();
  assert.ok(files[panelFile].includes('padding: 13;'));
  // gap comes first in the block, so every row below it moved: the offsets of the snapshot the
  // panel was rendered from no longer describe the file.
  gap.value='20';gap.onchange();
  assert.ok(files[panelFile].includes('gap: 20;'),files[panelFile]);
  padding.value='140';padding.onchange();
  assert.equal(padding.validity,'');
  const frame=parse(files[panelFile]).nodes[0];
  assert.deepEqual({gap:frame.props.gap,padding:frame.props.padding},{gap:20,padding:140});
  assert.equal(container.querySelector('.property-notice').textContent??'','');
});
test('without a live document the host still refuses a splice out of a stale snapshot',t=>{
  const {files,panel,restore}=mountPanel({liveSource:false});
  t.after(restore);
  const padding=panel('padding');
  padding.value='13';padding.onchange();
  assert.ok(files[panelFile].includes('padding: 13;'));
  padding.value='14';padding.onchange();
  assert.match(padding.validity,/Исходник изменился/);
  assert.ok(!files[panelFile].includes('padding: 14;'));
});
test('declaring twice from one panel puts both statements after the last one',t=>{
  const {container,files,restore}=mountPanel({source:`component Panel {\n    Frame {\n        gap: 8;\n    }\n}`});
  t.after(restore);
  const add=container.querySelector('.property-add'),pick=add.querySelector('select'),value=add.querySelector('input'),button=add.querySelector('button');
  for(const [name,start]of [['width','240'],['height','40']]){
    pick.value=name;pick.onchange();
    assert.equal(value.value,propertyStart[name]);
    assert.equal(button.textContent,`Добавить ${name}`);
    value.value=start;button.onclick();
  }
  const notice=container.querySelector('.property-notice');
  assert.equal(notice.textContent??'','');
  const frame=parse(files[panelFile]).nodes[0];
  assert.deepEqual({gap:frame.props.gap,width:frame.props.width,height:frame.props.height},{gap:8,width:240,height:40});
  assert.match(files[panelFile],/gap: 8;\n        width: 240;\n        height: 40;\n    }/);
  // The rows still on screen describe the file before these statements, so a removal reaches
  // the fresh source rather than the offsets the panel was rendered with.
  const gapRow=[...container.querySelectorAll('label.property')].find(row=>row.querySelector('span')?.textContent==='gap');
  gapRow.querySelector('.property-remove').onclick();
  assert.ok(!files[panelFile].includes('gap:'));
  assert.deepEqual(Object.keys(parse(files[panelFile]).nodes[0].props).sort(),['height','width']);
});
test('a name a binding already occupies is not offered for declaring',t=>{
  const {container,restore}=mountPanel({source:`component Search {\n    TextInput {\n        value <-> state.query;\n    }\n}`});
  t.after(restore);
  const options=container.querySelector('.property-add').querySelector('select').children.map(option=>option.value);
  assert.ok(!options.includes('value'),'a bound name is taken, not addable');
  assert.ok(options.includes('placeholder'));
  assert.deepEqual([...container.querySelectorAll('label.property')].map(row=>row.querySelector('span')?.textContent),['value']);
});
// ── the same panel with a design state on screen ──────────────────────────────────────────────
const stateEntry=(files,key)=>parse(files[designFile]).states[0].nodes.find(node=>node.props.key===key);
const baseEntry=files=>parse(files[designFile]).entries.find(node=>node.props.key==='frame');
const preview=(files,key,layer)=>designStatePatch(parse(files[designFile]),layer)[key];

test('a value is read from the layer that holds it: state, then base, then markup',()=>{
  const fields=propertyFields(parse(keyedSource).nodes[0],keyedSource,
    {name:'узкий',path:designFile,source:designSource,entry:findEntry(designSource,'узкий','frame'),base:findEntry(designSource,null,'frame')});
  // Markup order first, then the names only the design file declares.
  assert.deepEqual(fields.map(field=>[field.key,field.layer]),
    [['key','markup'],['width','markup'],['gap','base'],['padding','state'],['opacity','state'],['background','base']]);
  const by=Object.fromEntries(fields.map(field=>[field.key,field]));
  assert.equal(by.padding.value,4,'the state value is the one on screen');
  assert.equal(by.gap.value,6);
  assert.equal(by.background.kind,'color');
  assert.equal(by.background.css,'#111318','a design-only property still gets a row of its own');
  for(const key of ['gap','padding','opacity','background'])assert.equal(by[key].commit,'state');
  // The design key is the address an override hangs on: editing it in markup would orphan the rows.
  assert.equal(by.key.commit,'literal');
  assert.equal(by.width.commit,'literal','a property the design file never mentions stays markup');
});
test('a design row writes its own block while the component keeps the rest',t=>{
  const {files,panel,rowOf,restore}=mountKeyed();
  t.after(restore);
  const padding=panel('padding');
  padding.value='9';padding.onchange();
  assert.equal(padding.validity,'');
  assert.equal(stateEntry(files,'frame').props.padding,9);
  assert.equal(parse(files[keyedFile]).nodes[0].props.padding,12,'the markup keeps its own padding');
  // A base row writes the base block, which the state below it still falls back to.
  const gap=panel('gap');
  gap.value='11';gap.onchange();
  assert.equal(baseEntry(files).props.gap,11);
  assert.deepEqual(preview(files,'frame','узкий').gap,11);
  // And a row no design block holds still writes the component, exactly as before.
  panel('width').value='400';panel('width').onchange();
  assert.equal(parse(files[keyedFile]).nodes[0].props.width,400);
  assert.equal(rowOf('padding').className,'property number state');
  assert.equal(rowOf('gap').querySelector('.property-layer').textContent,'базовые значения');
  assert.equal(rowOf('width').querySelector('.property-layer'),null,'a markup row needs no badge');
});
test('one selection serves several design edits, so each row is found again in the live file',t=>{
  const {files,panel,restore}=mountKeyed();
  t.after(restore);
  // `padding` comes first in the entry, so writing it moves `opacity` out of the snapshot's offsets.
  const padding=panel('padding'),opacity=panel('opacity');
  padding.value='10';padding.onchange();
  opacity.value='0.25';opacity.onchange();
  assert.equal(opacity.validity,'');
  assert.deepEqual({padding:stateEntry(files,'frame').props.padding,opacity:stateEntry(files,'frame').props.opacity},{padding:10,opacity:0.25});
  const gap=panel('gap');
  gap.value='7';gap.onchange();
  assert.equal(gap.validity,'');
  assert.equal(baseEntry(files).props.gap,7);
});
test('a string override is written quoted and a bare colour stays bare',t=>{
  const {files,panel,rowOf,restore}=mountKeyed('title');
  t.after(restore);
  assert.equal(rowOf('text').querySelector('.property-layer').textContent,'состояние «узкий»');
  const text=panel('text');
  assert.equal(text.value,'Уже','the row shows the value, not its quotes');
  text.value='Уже очень';text.onchange();
  assert.equal(text.validity,'');
  assert.match(files[designFile],/text: 'Уже очень';/);
  assert.equal(stateEntry(files,'title').props.text,'Уже очень');
  assert.equal(parse(files[keyedFile]).nodes[0].children[0].props.text,'Заголовок');
});
test('− gives the layer below back, and an entry that patched only this goes with it',t=>{
  const {container,files,rowOf,restore}=mountKeyed();
  t.after(restore);
  // The frame's state entry patches two properties, so − cuts one line and keeps the override.
  rowOf('padding').querySelector('.property-remove').onclick();
  assert.equal(container.querySelector('.property-notice').textContent??'','');
  assert.equal(stateEntry(files,'frame').props.padding,undefined);
  assert.equal(stateEntry(files,'frame').props.opacity,0.5);
  assert.equal(preview(files,'frame','узкий').padding,undefined,'the state stopped overriding it');
  assert.ok(files[keyedFile].includes('padding: 12;'),'the component was not touched');
  // The base block patches gap and a background, so removing gap leaves the background alone.
  rowOf('gap').querySelector('.property-remove').onclick();
  assert.equal(baseEntry(files).props.gap,undefined);
  assert.equal(baseEntry(files).props.background,'#111318');
});
test('− on a single-property entry removes that entry from the state',t=>{
  const {files,container,rowOf,restore}=mountKeyed('title');
  t.after(restore);
  rowOf('text').querySelector('.property-remove').onclick();
  assert.equal(container.querySelector('.property-notice').textContent??'','');
  assert.deepEqual(stateEntry(files,'title'),undefined,'no key-only block shadows the layer below');
  assert.equal(preview(files,'title','узкий'),undefined,'the title is back to its own markup');
});
test('with a state on screen the add form declares the override, not the property',t=>{
  const {container,files,restore}=mountKeyed();
  t.after(restore);
  const add=container.querySelector('.property-add'),pick=add.querySelector('select'),value=add.querySelector('input'),button=add.querySelector('button');
  const options=pick.children.map(option=>option.value);
  assert.ok(options.includes('width'),'the markup has it but this state does not, so it can be overridden');
  assert.ok(!options.includes('padding'),'the state already patches it');
  assert.ok(!options.includes('key'),'the entry is created under the control’s own key, never a new one');
  pick.value='width';pick.onchange();
  assert.equal(button.textContent,'Добавить width в состояние «узкий»');
  value.value='200';button.onclick();
  assert.equal(stateEntry(files,'frame').props.width,200);
  assert.equal(parse(files[keyedFile]).nodes[0].props.width,320,'the component keeps its own width');
});
test('a state the file no longer holds is refused instead of spliced into',t=>{
  const {container,files,panel,rowOf,restore}=mountKeyed();
  t.after(restore);
  // The designer deleted the state in another tab: only its rows are on screen now.
  files[designFile]=`${designSource.slice(0,designSource.indexOf('\n\n    state'))}\n}`;
  const padding=panel('padding');
  padding.value='9';padding.onchange();
  assert.match(padding.validity,/не объявлено/);
  assert.ok(!files[designFile].includes('padding'),'nothing was written into the remaining blocks');
  rowOf('padding').querySelector('.property-remove').onclick();
  assert.match(container.querySelector('.property-notice').textContent,/Состояние узкий не объявлено/);
  assert.equal(baseEntry(files).props.gap,6,'the base block is still intact');
});
