import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createStatesPanel} from '../src/states-panel.js';

// Minimal DOM: the panel assigns handlers directly, so a test presses the real button and reads
// back the rows it produced instead of dispatching synthetic events.
class Element extends EventTarget {
 constructor(){super();this.children=[];this.dataset={};this.style={};this.attrs=new Map();this.parts=new Map();this.classList={toggle(){}};}
 set innerHTML(value){this.markup=value;}
 querySelector(selector){if(!this.parts.has(selector))this.parts.set(selector,new Element());return this.parts.get(selector);}
 querySelectorAll(){return [];}
 setAttribute(key,value){this.attrs.set(key,String(value));}
 getAttribute(key){return this.attrs.get(key);}
 removeAttribute(key){this.attrs.delete(key);}
 remove(){if(this.parent)this.parent.children=this.parent.children.filter(c=>c!==this);this.parent=null;}
 insertBefore(child,next){child.remove();const index=next?this.children.indexOf(next):this.children.length;this.children.splice(index,0,child);child.parent=this;}
 before(){}
 prepend(){}
 append(...children){for(const child of children){child.remove();child.parent=this;this.children.push(child);}}
 replaceChildren(...children){for(const child of this.children)child.parent=null;this.children=[];this.append(...children);}
 contains(element){return this===element||this.children.some(child=>child.contains(element));}
 closest(){return null;}
 getBoundingClientRect(){return {top:0,height:20,left:0,width:120};}
 focus(){globalThis.document.activeElement=this;this.focused=true;}
 select(){this.selected=true;}
 scrollIntoView(){}
}
const states=[{name:'поиск',count:2},{name:'пусто',count:1}];
function mount(t,handlers={}){
 const original=Object.getOwnPropertyDescriptor(globalThis,'document'),created=[];
 const document={createElement(){const element=new Element();created.push(element);return element;},activeElement:null};
 Object.defineProperty(globalThis,'document',{value:document,configurable:true});
 t.after(()=>{if(original)Object.defineProperty(globalThis,'document',original);else delete globalThis.document;});
 const calls=[];
 const wrap=name=>(...args)=>{calls.push([name,...args]);return handlers[name]?.(...args);};
 const panel=createStatesPanel({explorer:new Element(),onActivate:wrap('activate'),onCreate:wrap('create'),onRename:wrap('rename'),onDelete:wrap('delete')});
 panel.update({path:'ui/Search.design.ui',states,baseCount:3,active:'поиск',...handlers.update});
 const section=created[0];
 return {panel,calls,created,add:section.querySelector('[data-action=add]'),items:section.querySelector('.states-items'),status:section.querySelector('.states-status'),fileLine:section.querySelector('.states-file')};
}
const labels=items=>items.children.map(item=>item.children[0]?.textContent??null);

test('the list is the base block plus every state, with the active one marked',t=>{
 const {items,fileLine}=mount(t);
 assert.equal(fileLine.textContent,'ui/Search.design.ui');
 assert.deepEqual(labels(items),['Базовые значения','поиск','пусто']);
 assert.deepEqual(items.children.map(item=>item.children[0].getAttribute('aria-current')==='true'),[false,true,false]);
 assert.equal(items.children[0].children[1].textContent,'3 переопределённых ключа');
 assert.equal(items.children[1].children[1].textContent,'2 переопределённых ключа');
 assert.equal(items.children[2].children[1].textContent,'1 переопределённый ключ');
});
test('a row switches by state name and the base row switches to the base block',t=>{
 const {calls,items}=mount(t);
 items.children[2].children[0].onclick();
 items.children[0].children[0].onclick();
 assert.deepEqual(calls,[['activate','пусто'],['activate',null]]);
});
test('a refused switch leaves the list alone and explains itself',t=>{
 const {items,status,calls}=mount(t,{activate(){throw Error('Состояние не участвует в текущем предпросмотре');}});
 items.children[1].children[0].onclick();
 assert.equal(status.textContent,'Состояние не участвует в текущем предпросмотре');
 assert.equal(status.hidden,false);
 assert.deepEqual(labels(items),['Базовые значения','поиск','пусто']);
 assert.equal(calls.length,1);
});
test('adding asks the host for the typed name and the draft row goes away',t=>{
 const {calls,created,add,items}=mount(t);
 add.onclick();
 const draft=items.children.at(-1);
 assert.ok(draft.className.includes('states-draft'),'the draft is appended last');
 const input=draft.children[0];
 assert.equal(created.at(-1),input,'the input is the element that takes focus');
 assert.equal(globalThis.document.activeElement,input);
 assert.equal(input.value,'');assert.equal(input.getAttribute('aria-label'),'Имя нового состояния');
 input.value='ошибка';input.onkeydown({key:'Enter',preventDefault(){}});
 assert.deepEqual(calls,[['create','ошибка']]);
 assert.deepEqual(labels(items),['Базовые значения','поиск','пусто']);
});
test('Escape cancels a draft, and a name the file rejects stays on screen',t=>{
 const {calls,add,items,status}=mount(t,{create(name){if(name==='поиск')throw Error('Состояние поиск уже объявлено');}});
 add.onclick();
 items.children.at(-1).children[0].onkeydown({key:'Escape',preventDefault(){}});
 assert.deepEqual(calls,[],'Escape asks the host for nothing');
 assert.equal(items.children.length,3);
 add.onclick();
 const input=items.children.at(-1).children[0];
 input.value='поиск';input.onchange();
 assert.equal(status.textContent,'Состояние поиск уже объявлено');
 assert.equal(items.children.at(-1).children[0].value,'поиск','a refused name keeps the text that was typed');
});
test('renaming starts from the current name and hands the host both names',t=>{
 const {calls,items}=mount(t);
 items.children[1].children[2].onclick();
 assert.equal(items.children[1].className,'states-item states-draft','the draft replaces the row it renames');
 const input=items.children[1].children[0];
 assert.equal(input.value,'поиск');assert.equal(input.getAttribute('aria-label'),'Новое имя состояния');
 assert.equal(input.selected,true,'a rename starts with the old name selected');
 input.value='search';input.onkeydown({key:'Enter',preventDefault(){}});
 assert.deepEqual(calls,[['rename','поиск','search']]);
 assert.deepEqual(labels(items),['Базовые значения','поиск','пусто']);
});
test('deleting passes the state name, and only states can be renamed or deleted',t=>{
 const {calls,items}=mount(t);
 assert.deepEqual(items.children[0].children.map(c=>c.className),['states-row base','states-count']);
 items.children[2].children[3].onclick();
 assert.deepEqual(calls,[['delete','пусто']]);
});
test('a read-only scenario hides the authoring affordances',t=>{
 const {items,add}=mount(t,{update:{editable:false}});
 assert.deepEqual(items.children.map(item=>item.children.map(c=>c.className)),[
  ['states-row base','states-count'],['states-row','states-count'],['states-row','states-count']]);
 assert.equal(add.disabled,true);
});
test('a component without a design file says so instead of listing rows',t=>{
 const {items,add,fileLine}=mount(t,{update:{path:null,states:[],baseCount:0,active:undefined}});
 assert.equal(fileLine.textContent,'Дизайн-файл не выбран');
 assert.equal(items.children.length,1);
 assert.equal(items.children[0].className,'states-empty');
 assert.equal(add.disabled,true);
});
test('a state that left the file while being renamed ends its draft',t=>{
 const {panel,items}=mount(t);
 items.children[1].children[2].onclick();
 assert.ok(items.children[1].className.includes('states-draft'));
 panel.update({path:'ui/Search.design.ui',states:[{name:'пусто',count:1}],baseCount:3,active:null});
 assert.deepEqual(labels(items),['Базовые значения','пусто']);
});
test('the snapshot is the switcher the host already shows',t=>{
 const {panel,items}=mount(t);
 items.children[1].children[2].onclick();
 assert.deepEqual(panel.snapshot(),{path:'ui/Search.design.ui',editable:true,active:'поиск',status:'',
  rows:[{name:null,base:true,count:3,current:false},{name:'поиск',base:false,count:2,current:true,index:0},{name:'пусто',base:false,count:1,current:false,index:1}]});
});
