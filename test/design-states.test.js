import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse,validateDesign,designStatePatch} from '../src/language.js';
import {propertyEdit,propertyAddEdit,propertyRemoveEdit,sourceNode} from '../src/property-edit.js';
import {findState,findEntry,stateCreateEdit,stateRenameEdit,stateDeleteEdit,statePropertyEdit,stateEntryRemoveEdit} from '../src/design-states.js';

// The panel applies one edit at a time, exactly as the inspector does for the markup file.
const run=(source,edit)=>source.slice(0,edit.from)+edit.insert+source.slice(edit.to);
const patch=(source,state=null)=>designStatePatch(parse(source),state);
const occurrences=(source,pattern)=>(source.match(pattern)??[]).length;
const component="component Demo { Frame { key: 'root'; Text { key: 'status'; text: state.status; } Button { key: 'go'; text: 'Найти'; } } }";
const base="design Demo {\n    Text { key: 'status'; text: 'Готово'; }\n}";
const twoStates="design Demo {\n    Text { key: 'status'; text: 'Готово'; }\n\n    state 'поиск' {\n        Text { key: 'status';\n            text: 'Ищем…';\n        }\n    }\n\n    state 'пусто' { Button { key: 'go'; disabled: true; } }\n}";

test('a state keeps its entries as nodes the panel can edit',()=>{
 const state=parse(twoStates).states[0];
 assert.deepEqual(state.nodes.map(n=>[n.type,n.props.key]),[['Text','status']]);
 const [entry]=state.nodes;
 assert.ok(entry.propertyRanges.text.from<entry.propertyRanges.text.to);
 assert.ok(entry.statementRanges.text.to>entry.propertyRanges.text.to);
 assert.deepEqual(entry.children,[]);
 // The key locates the entry, so it stays on the node but never reaches the preview patch.
 assert.equal(entry.props.key,'status');
 assert.deepEqual(Object.keys(patch(twoStates,'поиск').status),['text']);
 assert.equal(sourceNode(twoStates,entry.start).props.key,'status');
});

test('a design file records where its state blocks may be added',()=>{
 const doc=parse(twoStates);
 assert.equal(twoStates.slice(doc.designBody.to,doc.designBody.to+1),'}');
 assert.ok(doc.designBody.from<=doc.states[0].start&&doc.states.at(-1).end<=doc.designBody.to);
 assert.equal(parse(base).designBody.to,base.length-1);
 assert.equal(parse(component).designBody,null);
});

test('a new state is appended to the design body and starts empty',()=>{
 const next=run(base,stateCreateEdit(base,'загрузка'));
 assert.deepEqual(parse(next).states.map(s=>s.name),['загрузка']);
 assert.deepEqual(patch(next),{status:{text:'Готово'}});
 assert.deepEqual(patch(next,'загрузка'),{status:{text:'Готово'}});
 assert.ok(/\n\n {4}state 'загрузка' \{\n {4}\}\n\}/.test(next));
 validateDesign(parse(component),parse(next));
 const after=run(twoStates,stateCreateEdit(twoStates,'ошибка'));
 assert.deepEqual(parse(after).states.map(s=>s.name),['поиск','пусто','ошибка']);
 assert.deepEqual(patch(after,'поиск'),{status:{text:'Ищем…'}});
 assert.throws(()=>stateCreateEdit(twoStates,'поиск'),/Состояние поиск уже объявлено/);
 assert.throws(()=>stateCreateEdit(twoStates,''),/Имя состояния должно быть непустым/);
 assert.throws(()=>stateCreateEdit(component,'новый'),/не является дизайн-файлом/);
});

test('a state takes a new name without disturbing its siblings',()=>{
 const next=run(twoStates,stateRenameEdit(twoStates,'поиск','загрузка'));
 assert.deepEqual(parse(next).states.map(s=>s.name),['загрузка','пусто']);
 assert.deepEqual(patch(next,'загрузка'),{status:{text:'Ищем…'}});
 assert.deepEqual(patch(next,'пусто'),{go:{disabled:true},status:{text:'Готово'}});
 const quoted=run(twoStates,stateRenameEdit(twoStates,'поиск',"не тронь 'это'"));
 assert.deepEqual(parse(quoted).states.map(s=>s.name),['не тронь \'это\'','пусто']);
 assert.deepEqual(patch(quoted,"не тронь 'это'"),{status:{text:'Ищем…'}});
 assert.throws(()=>stateRenameEdit(twoStates,'поиск','пусто'),/Состояние пусто уже объявлено/);
 assert.throws(()=>stateRenameEdit(twoStates,'нет','что'),/Состояние нет не объявлено/);
 assert.throws(()=>stateRenameEdit(twoStates,'поиск',' '),/Имя состояния должно быть непустым/);
});

test('deleting a state leaves the base and the other states alone',()=>{
 const next=run(twoStates,stateDeleteEdit(twoStates,'поиск'));
 assert.deepEqual(parse(next).states.map(s=>s.name),['пусто']);
 assert.deepEqual(patch(next),{status:{text:'Готово'}});
 assert.deepEqual(patch(next,'пусто'),{go:{disabled:true},status:{text:'Готово'}});
 assert.ok(!/поиск/.test(next));
 assert.ok(!/\n\n\n/.test(next));
 const one=run(twoStates,stateDeleteEdit(twoStates,'пусто'));
 const middle=run(one,stateDeleteEdit(one,'поиск'));
 assert.deepEqual(parse(middle).states,[]);
 assert.deepEqual(patch(middle),{status:{text:'Готово'}});
 assert.throws(()=>stateDeleteEdit(twoStates,'нет'),/Состояние нет не объявлено/);
});

test('an override the state already carries is edited in place',()=>{
 const before=findEntry(twoStates,'поиск','status');
 const next=run(twoStates,statePropertyEdit(twoStates,'поиск','Text','status','text',"'Печатаем…'"));
 assert.deepEqual(patch(next,'поиск'),{status:{text:'Печатаем…'}});
 assert.equal(occurrences(next,/Text \{ key: 'status'/g),occurrences(twoStates,/Text \{ key: 'status'/g));
 assert.equal(findEntry(next,'поиск','status').start,before.start);
});

test('a state gains a property its entry does not declare yet',()=>{
 const next=run(twoStates,statePropertyEdit(twoStates,'поиск','Text','status','opacity','0.6'));
 assert.deepEqual(patch(next,'поиск'),{status:{opacity:0.6,text:'Ищем…'}});
 assert.deepEqual(patch(next,'пусто'),{go:{disabled:true},status:{text:'Готово'}});
 const added=findEntry(next,'поиск','status');
 assert.equal(added.props.opacity,0.6);
 assert.ok(added.propertyRanges.opacity.from>added.propertyRanges.text.to);
});

test('a control the state never mentioned gets its own entry',()=>{
 const next=run(twoStates,statePropertyEdit(twoStates,'поиск','Button','go','disabled','true'));
 assert.deepEqual(parse(next).states.map(s=>s.nodes.map(n=>n.props.key)),[['status','go'],['go']]);
 assert.deepEqual(patch(next,'поиск'),{status:{text:'Ищем…'},go:{disabled:true}});
 assert.deepEqual(patch(next,'пусто'),{go:{disabled:true},status:{text:'Готово'}});
 assert.deepEqual(patch(next),{status:{text:'Готово'}});
 validateDesign(parse(component),parse(next));
});

test('a new entry is written on its own lines inside the state',()=>{
 const next=run(twoStates,statePropertyEdit(twoStates,'пусто','Text','status','text',"'Пусто'"));
 assert.ok(/state 'пусто' \{ Button \{ key: 'go'; disabled: true; \}\n {8}Text \{\n {12}key: 'status';\n {12}text: 'Пусто';\n {8}\}\n {4}\}/.test(next));
 assert.deepEqual(patch(next,'пусто'),{go:{disabled:true},status:{text:'Пусто'}});
});

test('dropping an entry removes only that control from the state',()=>{
 const next=run(twoStates,stateEntryRemoveEdit(twoStates,'поиск','status'));
 assert.deepEqual(parse(next).states.map(s=>s.name),['поиск','пусто']);
 assert.deepEqual(parse(next).states[0].nodes,[]);
 assert.deepEqual(patch(next,'поиск'),{status:{text:'Готово'}});
 assert.deepEqual(patch(next,'пусто'),{go:{disabled:true},status:{text:'Готово'}});
 assert.ok(!/\n\n\n/.test(next));
 assert.throws(()=>stateEntryRemoveEdit(twoStates,'поиск','go'),/Состояние поиск не переопределяет go/);
});

test('a property is removed from an entry with the markup primitive',()=>{
 const added=run(twoStates,statePropertyEdit(twoStates,'поиск','Text','status','color',"#8899aa"));
 const entry=findEntry(added,'поиск','status');
 const next=run(added,propertyRemoveEdit(added,entry.start,'color'));
 assert.deepEqual(parse(next).states[0].nodes[0].props,{key:'status',text:'Ищем…'});
 assert.deepEqual(patch(next,'поиск'),{status:{text:'Ищем…'}});
 assert.equal(next,twoStates);
});

test('a value written into a state stays the value the preview reads',()=>{
 const next=run(twoStates,statePropertyEdit(twoStates,'поиск','Text','status','text',"'Ищем…' /* после */"));
 assert.deepEqual(patch(next,'поиск'),{status:{text:'Ищем…'}});
 assert.ok(next.includes('/* после */'));
 assert.throws(()=>statePropertyEdit(twoStates,'поиск','Text','status','text','}'),/Значение не разобрано/);
 const edited=run(twoStates,propertyEdit(twoStates,findEntry(twoStates,'пусто','go').start,'disabled',false));
 assert.deepEqual(patch(edited,'пусто'),{go:{disabled:false},status:{text:'Готово'}});
 const added=run(edited,propertyAddEdit(edited,findEntry(edited,'пусто','go').start,'text',"'Найти'"));
 assert.deepEqual(patch(added,'пусто'),{go:{disabled:false,text:'Найти'},status:{text:'Готово'}});
});

test('the lookups answer what the panel has to mark',()=>{
 assert.deepEqual(findState(twoStates,'поиск').nodes.map(n=>n.props.key),['status']);
 assert.equal(findState(twoStates,'нет'),null);
 assert.equal(findEntry(twoStates,'поиск','go'),null);
 assert.equal(findEntry(twoStates,'нет','status'),null);
 assert.throws(()=>statePropertyEdit(twoStates,'нет','Text','status','text',"'x'"),/Состояние нет не объявлено/);
 assert.throws(()=>statePropertyEdit(twoStates,'поиск','Button','status','text',"'x'"),/Тип дизайн-key status: ожидался Text, получен Button/);
});
