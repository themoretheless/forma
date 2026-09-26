import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {sharedFields,sharedEdit,createBulkEditor} from '../src/bulk-edit.js';

// Three controls the canvas can hold at once: one name they agree on (`height`), names they disagree
// on (`text`, `opacity`), and names that have no single value to write into all of them — a design
// token on two of them, a property only one declares, and the key the preview addresses them by.
const source=`component Batch {
    Frame {
        Button { key: 'one'; text: 'Найти'; height: 36; opacity: 0.4; color: '#e8edf7'; }
        Button { key: 'two'; text: 'Сброс'; height: 36; opacity: 0.75; color: design.ink; }
        Button { key: 'three'; text: 'Ещё'; height: 36; opacity: 1; color: design.ink; disabled: true; }
    }
}`;
const kidsOf=text=>parse(text).nodes[0].children;
const kids=kidsOf(source);
const starts=kids.map(node=>node.start);
const byKey=fields=>Object.fromEntries(fields.map(field=>[field.key,field]));
const apply=(text,change)=>text.slice(0,change.from)+change.insert+text.slice(change.to);

test('a batch row is a name every selected control declares as a literal of the same kind',()=>{
  const fields=byKey(sharedFields(source,starts));
  assert.deepEqual(Object.keys(fields),['text','height','opacity']);
  // The value all three agree on arrives ready to edit; a disagreement shows no value at all.
  assert.deepEqual({kind:fields.height.kind,text:fields.height.text},{kind:'number',text:'36'});
  assert.equal(fields.text.text,null);
  assert.equal(fields.opacity.text,null);
});
test('a name without one value to write gets no row',()=>{
  // `color` is a plain string on one button and a design token on two, `disabled` belongs to one of
  // them only, `key` is how the preview addresses each control by itself.
  const fields=byKey(sharedFields(source,starts));
  for(const key of ['color','disabled','key'])assert.equal(fields[key],undefined,key);
  const frame=parse(source).nodes[0];
  assert.deepEqual(sharedFields(source,[starts[0],frame.start]),[],'a container declares no text of its own');
});
test('one control is not a batch, and an offset the file lost stops it',()=>{
  assert.deepEqual(sharedFields(source,[]),[]);
  assert.deepEqual(sharedFields(source,[starts[0]]),[],'the panel above already edits a single control');
  assert.deepEqual(sharedFields(source,[starts[0],kids[0].end+1]),[],'a stale selection addresses nothing');
});
test('one write reaches every control of the batch as a single transaction',()=>{
  const change=sharedEdit(source,starts,'height',48);
  assert.deepEqual(kidsOf(apply(source,change)).map(node=>node.props.height),[48,48,48]);
  // One span across the three declarations is what the editor undoes as one step.
  assert.ok(change.from>=kids[0].start&&change.from<kids[0].end);
  assert.ok(change.to>kids.at(-1).start&&change.to<=kids.at(-1).end);
});
test('the batch reports where its controls stand afterwards, in the order it was given them',()=>{
  // The values written are longer than those they replace, so every control after the first moved.
  const next=apply(source,sharedEdit(source,starts,'text','Подтвердить'));
  assert.deepEqual(sharedEdit(source,starts,'text','Подтвердить').starts,kidsOf(next).map(node=>node.start));
  const order=[...starts].reverse();
  const back=apply(source,sharedEdit(source,order,'text','Ок'));
  assert.deepEqual(sharedEdit(source,order,'text','Ок').starts,kidsOf(back).map(node=>node.start).reverse());
});
test('a batch of two leaves the control that was dropped out',()=>{
  const change=sharedEdit(source,[starts[0],starts[1]],'height',40);
  assert.deepEqual(kidsOf(apply(source,change)).map(node=>node.props.height),[40,40,36]);
  assert.equal(change.starts.length,2);
});

// A tiny DOM: enough for the section to build rows, find them back and run their handlers.
const matches=(el,selector)=>{
  const parts=/^([a-z]*)((?:\.[\w-]+)*)(?:\[data-(\w+)(?:=(?:"([^"]*)"|([^\]]*)))?\])?$/.exec(selector);
  if(!parts)throw Error(`shim: unknown selector ${selector}`);
  const [,tag,classes,attr,a,b]=parts;
  if(tag&&el.tagName!==tag)return false;
  const own=new Set(String(el.className).split(/\s+/).filter(Boolean));
  for(const name of classes.split('.').filter(Boolean))if(!own.has(name))return false;
  if(attr===undefined)return true;
  return a===undefined&&b===undefined?attr in el.dataset:String(el.dataset[attr]??'')===String(a??b);
};
class El{
  constructor(tag){this.tagName=tag;this.children=[];this.dataset={};this.attrs=new Map();this.className='';this.style={};this.validity='';}
  setAttribute(key,value){this.attrs.set(key,String(value));}
  getAttribute(key){return this.attrs.get(key)??null;}
  append(...children){for(const child of children){child.parent=this;this.children.push(child);}}
  replaceChildren(...children){this.children=[];this.append(...children);}
  remove(){const at=this.parent?.children.indexOf(this)??-1;if(at>=0)this.parent.children.splice(at,1);}
  *walk(){for(const child of this.children){yield child;yield*child.walk();}}
  querySelector(selector){for(const el of this.walk())if(matches(el,selector))return el;return null;}
  querySelectorAll(selector){return [...this.walk()].filter(el=>matches(el,selector));}
  setCustomValidity(message){this.validity=message;}
  reportValidity(){}
  focus(){}
}
const file='ui/Batch.ui';
// The host of Studio is what a row commits to: it guards the text it holds, splices the change in and
// redraws both the batch and the canvas selection from the offsets the merge reported.
function mountBatch({text=source,selection=starts,primary=starts.at(-1),editable=true}={}){
  const files={[file]:text};
  const container=new El('div');
  const document={createElement:tag=>new El(tag)};
  const saved=Object.getOwnPropertyDescriptor(globalThis,'document');
  Object.defineProperty(globalThis,'document',{value:document,configurable:true});
  let selectionNow=[...selection],primaryNow=primary;
  const commits=[];
  const editor=createBulkEditor({container,
    commit:change=>{
      commits.push(change);
      if(files[change.file]!==change.source)throw Error('Исходник изменился — повторите операцию');
      files[change.file]=apply(change.source,change);
      selectionNow=change.starts;primaryNow=change.primary;
      draw();
    },
    onError:()=>{}});
  function draw(){editor.render({path:file,source:files[file],starts:selectionNow,primary:primaryNow,editable});}
  draw();
  return {container,files,editor,commits,selection:()=>selectionNow,
    row:name=>container.querySelector(`input[data-bulk=${name}]`),
    names:()=>container.querySelectorAll('input[data-bulk]').map(input=>input.dataset.bulk),
    restore(){if(saved)Object.defineProperty(globalThis,'document',saved);else delete globalThis.document;}};
}
test('the section offers one row per shared name and says what a batch means',t=>{
  const {container,names,row,restore}=mountBatch();
  t.after(restore);
  assert.deepEqual(names(),['text','height','opacity']);
  assert.equal(container.querySelectorAll('.bulk-edit').length,1,'one section, however often the panel redraws');
  assert.equal(container.querySelector('.bulk-note').textContent,'Значение записывается в разметку всех 3 выбранных контролов.');
  assert.equal(row('height').value,'36');
  assert.equal(row('text').value,'');
  assert.equal(row('text').placeholder,'значения различаются');
  // The bridge edits a single field by `data-prop`; a batch row must never be mistaken for one.
  assert.equal(container.querySelectorAll('input[data-prop]').length,0);
});
test('a row writes every control of the batch and keeps the batch selected',t=>{
  const {files,row,commits,selection,restore}=mountBatch();
  t.after(restore);
  row('height').value='48';row('height').onchange();
  assert.equal(commits.length,1);
  assert.deepEqual(kidsOf(files[file]).map(node=>node.props.height),[48,48,48]);
  // The host redraws from these offsets, so a second row is editable without touching the canvas.
  row('text').value='Готово';row('text').onchange();
  assert.equal(commits.length,2);
  assert.deepEqual(kidsOf(files[file]).map(node=>node.props.text),['Готово','Готово','Готово']);
  assert.deepEqual(kidsOf(files[file]).map(node=>node.start),selection(),'the batch still addresses the same controls');
  assert.equal(row('text').value,'Готово','the redraw shows the value the batch now holds');
  assert.equal(row('height').value,'48');
});
test('a row keeps the control the panel above belongs to',t=>{
  const {commits,row,restore}=mountBatch({primary:starts[1]});
  t.after(restore);
  row('opacity').value='1';row('opacity').onchange();
  assert.equal(commits[0].primary,sharedEdit(source,starts,'opacity',1).starts[1],'the primary moved with the text before it');
  assert.equal(commits[0].file,file);
});
test('a value the row cannot hold is refused inside the row',t=>{
  const {files,row,commits,restore}=mountBatch();
  t.after(restore);
  row('height').value='много';row('height').onchange();
  assert.equal(row('height').validity,'Некорректное значение свойства');
  assert.equal(commits.length,0);
  assert.equal(files[file],source);
});
test('a single selection, a read-only scenario and a lost batch all hide the section',t=>{
  const {container,editor,restore}=mountBatch();
  t.after(restore);
  assert.ok(container.querySelector('.bulk-edit'));
  const redraw=({selection=starts,editable=true}={})=>editor.render({path:file,source,starts:selection,primary:selection.at(-1),editable});
  redraw({selection:[starts[0]]});
  assert.equal(container.querySelector('.bulk-edit'),null,'one control is what the panel above edits');
  redraw({editable:false});
  assert.equal(container.querySelector('.bulk-edit'),null,'a design scenario leaves the markup alone');
  redraw({selection:[starts[0],kids[0].end+1]});
  assert.equal(container.querySelector('.bulk-edit'),null,'offsets the file lost are not a batch');
});
