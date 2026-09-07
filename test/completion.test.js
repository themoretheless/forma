import {test} from 'node:test';
import assert from 'node:assert/strict';
import {completeCode,createCompletionSource,indexProject} from '../src/completion.js';
import {EditorState} from '@codemirror/state';
const files={
 'src/lib.rs':`pub struct AddressVm { pub city: Property<String> }
 pub struct CustomerVm { pub name: Property<String>, pub busy: Property<bool>, pub address: Context<AddressVm> }
 impl CustomerVm { pub fn save(&self) {} fn with_arg(&self, value: String) {} fn mutate(&mut self) {} }
 // struct Fake { field: Property<String> }
 `,
 'components/Base.ui':`component Base { width:200; disabled:false; Rectangle {} }`,
 'components/TextField.ui':`component TextField : Base { value:''; placeholder:''; }`,
};
function hints(body,extra={}){const source=`component Form { contextType:'crate::CustomerVm'; events:['saved']; Frame { ${body}`;return completeCode({source:source.replace('|',''),pos:source.indexOf('|'),path:'ui/Form.ui',files:{...files,...extra},explicit:true});}
const labels=result=>result?.options.map(o=>o.label)??[];
const overrideFiles={'components/Button.ui':`component Button {fontSize:14;Rectangle {ContentPresenter {key:'content';Row {Text {key:'header-caption';text:'Save';fontSize:props.fontSize;}}}}}`};
function overrideHints(body,extra={}){
 const source=`component LargeButton : Button { ${body}`;
 return completeCode({source:source.replace('|',''),pos:source.indexOf('|'),path:'components/LargeButton.ui',files:{...overrideFiles,...extra},explicit:true});
}
test('override hints resolve keys and the actual content root in incomplete code',()=>{
 assert.deepEqual(labels(overrideHints('override |')),['content','header-caption']);
 const content=overrideHints('override con|').options.find(o=>o.label==='content');assert.match(content.detail,/Row/);
 assert.match(overrideHints("override 'header-|'").options.find(o=>o.label==='header-caption').apply,/^'header-caption'/);
 const row=labels(overrideHints('override content { ga|'));assert.ok(row.includes('gap'));assert.ok(!row.includes('fontSize'));assert.ok(!row.includes('Text'));
 const text=labels(overrideHints("override 'header-caption' { fon|"));assert.ok(text.includes('fontSize'));assert.ok(!text.includes('key'));assert.ok(!text.includes('clicked'));
 assert.ok(!labels(overrideHints("override 'header-caption' { fontSize:18; col|")).includes('fontSize'));
 assert.equal(overrideHints('override content { // ga|'),null);
});
test('incomplete UI offers inherited properties, bindings and components',()=>{
 const result=hints('TextField { pla|');assert.ok(labels(result).includes('placeholder'));assert.ok(labels(result).includes('disabled'));assert.ok(labels(result).includes('value <->'));
 assert.equal(result.from,'component Form { contextType:\'crate::CustomerVm\'; events:[\'saved\']; Frame { TextField { '.length);
 assert.ok(labels(hints('Tex|')).includes('TextField'));
 assert.ok(!labels(hints('TextField { width:200; dis|')).includes('width'));
});
test('typed fields, nested contexts and context assignments use the right model',()=>{
 assert.deepEqual(labels(hints('TextField { value <-> state.|')),['state.name','state.busy','state.address']);
 assert.deepEqual(labels(hints('TextField { value <-> state.address.|')),['state.address.city']);
 assert.deepEqual(labels(hints("TextField { contextType:'crate::AddressVm'; context:state.address; value <-> state.|")),['state.city']);
 assert.deepEqual(labels(hints("TextField { contextType:'crate::AddressVm'; context:state.|")),['state.address']);
 assert.deepEqual(labels(hints('TextField { value <-> state.unknown.|')),[]);
});
test('events and commands are separated from model fields',()=>{
 assert.deepEqual(labels(hints('TextField { clicked -> events.|')),['events.saved']);
 const commands=hints('TextField { clicked -> state.|');assert.deepEqual(labels(commands),['state.save']);assert.equal(commands.options[0].apply,'state.save()');
 assert.equal(hints('TextField { clicked -> state.sa|(); }').options[0].apply,'state.save');
});
test('props refer to component defaults, not the current visual primitive',()=>{
 const source='component TextField : Base { value:\'\'; Rectangle { Text { text:props.|';
 const result=completeCode({source:source.replace('|',''),pos:source.indexOf('|'),path:'components/TextField.ui',files,explicit:true});
 assert.ok(labels(result).includes('props.width'));assert.ok(labels(result).includes('props.value'));
});
test('comments and strings suppress hints, but contextType strings offer models',()=>{
 for(const s of ['TextField { // state.|','TextField { /* state.|',"TextField { value:'state.|",'TextField { value:"state.|'])assert.equal(hints(s),null);
 assert.ok(labels(hints("TextField { contextType:'crate::|" )).includes('crate::CustomerVm'));
 assert.ok(!labels(hints("TextField { contextType:'crate::|" )).includes('crate::Fake'));
});
test('completion refreshes after an in-place project edit',()=>{
 const project={...files};const source=createCompletionSource(()=>({files:project,path:'ui/Form.ui'}));
 const doc="component Form { contextType:'crate::CustomerVm'; Frame { TextField { value:state.";
 const ctx={state:EditorState.create({doc}),pos:doc.length,explicit:true};assert.ok(labels(source(ctx)).includes('state.name'));
 project['src/lib.rs']=project['src/lib.rs'].replace('pub name:', 'pub renamed:');assert.ok(labels(source(ctx)).includes('state.renamed'));assert.ok(!labels(source(ctx)).includes('state.name'));
});
test('ambiguous Rust types are not guessed and non-code files get no hints',()=>{
 const index=indexProject({...files,'src/other.rs':'struct CustomerVm {other:Property<bool>}'});assert.equal(index.models.get('CustomerVm'),null);
 assert.equal(completeCode({source:'state.',pos:6,path:'README.md',files,explicit:true}),null);
 assert.ok(labels(completeCode({source:'Prop',pos:4,path:'src/lib.rs',files,explicit:true})).includes('Property::new'));
});

test('typing a dot invalidates cached suggestions for the previous path',()=>{
 const result=hints('TextField { value:state.|');assert.ok(result.validFor.test('state.na'));assert.ok(!result.validFor.test('state.address.'));
 assert.ok(!hints('TextField { val|').validFor.test('value:'));
});
test('models in Rust modules keep qualified names and local nested contexts',()=>{
 const extra={'src/models.rs':'pub struct Account {pub address:Context<AddressVm>} pub struct AddressVm {pub street:Property<String>}'};
 const source="component Form {contextType:'crate::models::Account';Frame {TextField {value:state.address.";
 const result=completeCode({source,pos:source.length,path:'ui/Form.ui',files:{...files,...extra},explicit:true});assert.deepEqual(labels(result),['state.address.street']);
 assert.ok(labels(hints("TextField { contextType:'crate::|",extra)).includes('crate::models::Account'));
});
