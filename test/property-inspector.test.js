import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {setDesignData,designReferencesInFiles} from '../src/design-data.js';
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
// The host of Studio keeps the editor document ahead of `files`; a test host does the same.
function mountPanel({source=panelSource,liveSource=true}={}){
  const files={[panelFile]:source};
  const container=new El('div');
  const document={createElement:tag=>new El(tag)};
  const saved=Object.getOwnPropertyDescriptor(globalThis,'document');
  Object.defineProperty(globalThis,'document',{value:document,configurable:true});
  const savedOption=Object.getOwnPropertyDescriptor(globalThis,'Option');
  Object.defineProperty(globalThis,'Option',{value:Option,configurable:true});
  const inspector=createPropertyInspector({container,designTokens:()=>[],liveSource:liveSource?path=>files[path]:null,
    commit:({file,source:text,from,to,insert})=>{
      if(files[file]!==text)throw Error('Исходник изменился — повторите операции');
      const next=text.slice(0,from)+insert+text.slice(to);
      parse(next);
      files[file]=next;
    }});
  const node=parse(files[panelFile]).nodes[0];
  inspector.render({node,path:panelFile,source:files[panelFile],editable:true});
  return {container,files,panel:name=>container.querySelector(`input[data-prop=${name}]`),
    restore(){
      if(saved)Object.defineProperty(globalThis,'document',saved);else delete globalThis.document;
      if(savedOption)Object.defineProperty(globalThis,'Option',savedOption);else delete globalThis.Option;
    }};
}
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
