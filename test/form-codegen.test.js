import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync,writeFileSync,mkdtempSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join,resolve,dirname} from 'node:path';
import {spawnSync} from 'node:child_process';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
import {sections,sectionSource} from '../vector-ui/controls/catalog.js';
import {prepareFormProject,mapFormDiagnostic} from '../form-project.js';
import {generateForm} from '../src/form-codegen.js';

const project=resolve('vector-ui/examples/binding-app');
const entry='ui/CustomerForm.ui';
const files={[entry]:readFileSync(join(project,entry),'utf8')};
for(const name of readdirSync(join(project,'components')))files['components/'+name]=readFileSync(join(project,'components',name),'utf8');
const generate=source=>generateForm({...files,[entry]:source??files[entry]},entry);

test('generator rejects unsupported or misspelled declarations with source locations',()=>{
 for(const [from,to,expected]of [
   ["contextType: 'crate::CustomerVm'","contextType: 'crate::CustomerVm; bad'",/ui\/CustomerForm.ui:2:.*contextType/],
   ['events.saveRequested()','events.saveRequsted()',/ui\/CustomerForm.ui:26:.*не объявлено/],
   ['value <-> state.name','value <-> state.na-me',/ui\/CustomerForm.ui:.*(?:14:.*Rust-имя|Строка 14:)/],
   ['value <-> state.name','widht: state.name',/ui\/CustomerForm.ui:14:.*Неизвестное свойство/],
   ["key: 'nameInput'","key: 'runtime'",/Конфликт Rust-метода runtime/],
   ['value <-> state.name','value <-> state.name; value: \'other\'',/Повторное свойство/],
   ['disabled: state.busy','disabled <-> state.busy',/привязка disabled/],
 ])assert.throws(()=>generate(files[entry].replace(from,to)),expected);
});

test('generator accepts a Row as the form root',()=>{
 const result=generateForm({...files,'ui/Horizontal.ui':`component Horizontal {contextType:'crate::CustomerVm';Row {width:500;height:100;gap:8;Button {text:'First';} Button {text:'Second';}}}`},'ui/Horizontal.ui');
 assert.match(result.rust,/root:N \{kind:r#"Row"#/);
 const spanning=generateForm({...files,'ui/Spanning.ui':`component Spanning {contextType:'crate::CustomerVm';Grid {width:400;height:200;columns:[200,200];Button {column.span:2;row.span:2;text:'Wide';}}}`},'ui/Spanning.ui');
 assert.match(spanning.rust,/r#"column.span"#/);assert.match(spanning.rust,/r#"row.span"#/);
});

test('generated Rust compiles, exercises the renderer, and rejects wrong fields and types',()=>{
 const build=spawnSync('cargo',['test','--offline','--locked','--manifest-path',join(project,'Cargo.toml'),'--message-format=json'],{encoding:'utf8',timeout:180000,maxBuffer:8*1024*1024,env:{...process.env,FORMA_NODE:process.execPath}});
 assert.equal(build.status,0,build.stderr+'\n'+build.stdout.split('\n').filter(line=>line.startsWith('{')).map(line=>{try{return JSON.parse(line).message?.rendered??'';}catch{return '';}}).join('\n'));
 const artifacts=build.stdout.split('\n').filter(line=>line.startsWith('{')).map(line=>JSON.parse(line));
 const library=artifacts.find(item=>item.reason==='compiler-artifact'&&item.target.name==='forma'&&item.filenames.some(name=>name.endsWith('.rlib')));
 assert.ok(library,'Cargo must report the actual forma rlib');
 const rlib=library.filenames.find(name=>name.endsWith('.rlib'));
 const directory=mkdtempSync(join(tmpdir(),'forma-codegen-'));
 const models=`use forma::binding::{Property, Context};
 pub struct AddressVm { pub city: Property<String> }
 pub struct CustomerVm { pub name: Property<String>, pub busy: Property<bool>, pub status: Property<String>, pub address: Context<AddressVm> }
 `;
 try{
   const compile=(source,prefix=models)=>{
     writeFileSync(join(directory,'generated.rs'),prefix+generate(source).rust);
     return spawnSync('rustc',['--edition=2021','--crate-type=lib','--crate-name=generated_form','--emit=metadata','--extern',`forma=${rlib}`,'-L',`dependency=${dirname(rlib)}`,join(directory,'generated.rs'),'-o',join(directory,'output.rmeta')],{encoding:'utf8',timeout:30000});
   };
   assert.equal(compile(files[entry]).status,0);
   const catalog={};for(const dir of ['components','assets'])for(const name of readdirSync(resolve('vector-ui/controls',dir)))catalog[dir+'/'+name]=readFileSync(resolve('vector-ui/controls',dir,name),'utf8');
   const themed=materializeTheme(catalog);
   let program='pub struct Vm; impl Vm {fn noop(&self){}}\n';let checks='';
   for(const [i,section]of sections.entries()){
     const source=sectionSource(section.id,'light','regular').replace(/component \w+ \{/,`component Generated${i} { contextType:'crate::Vm';`).replace(/clicked -> [\w.]+\(\)/g,'clicked -> state.noop()');
     program+=generateForm({...themed,'ui/Catalog.ui':source},'ui/Catalog.ui').rust;
     checks+=`let mut f=Generated${i}::new(std::rc::Rc::new(Vm)).expect("${section.id}");assert!(f.runtime().unwrap().control_count()>0);assert!(!f.runtime().unwrap().pixels(320,${section.height},1.).is_empty());\n`;
   }
   writeFileSync(join(directory,'catalog.rs'),program+'fn main(){'+checks+'}');
   let catalogBuild=spawnSync('rustc',['--edition=2021','--extern',`forma=${rlib}`,'-L',`dependency=${dirname(rlib)}`,join(directory,'catalog.rs'),'-o',join(directory,'catalog')],{encoding:'utf8',timeout:120000,maxBuffer:8*1024*1024});
   assert.equal(catalogBuild.status,0,catalogBuild.stderr);
   const catalogRun=spawnSync(join(directory,'catalog'),[],{encoding:'utf8',timeout:30000});assert.equal(catalogRun.status,0,catalogRun.stderr);
   const dynamic=`component Dynamic { contextType:'crate::Live'; events:['edited']; Frame {key:'panel';width:500;height:600;padding:20;gap:10;
     Checkbox {key:'check';checked <-> state.checked;}
     Slider {key:'slider';value <-> state.amount;}
     TextArea {key:'input';value <-> state.text;changed -> events.edited();}
     Button {key:'button';width:state.width;text:state.text;color:state.color;clicked -> state.save(); Label {text:state.caption;color:props.color;} }
   } }`;
   const live=`use forma::binding::Property;use std::rc::Rc;
   pub struct Live {checked:Property<bool>,amount:Property<f64>,width:Property<f64>,text:Property<String>,color:Property<String>,caption:Property<String>}
   impl Live {fn save(&self){self.width.set(400.);}}
   `+generateForm({...themed,'ui/Live.ui':dynamic},'ui/Live.ui').rust+`
   fn main(){
     let vm=Rc::new(Live{checked:Property::new(false),amount:Property::new(0.5),width:Property::new(200.),text:Property::new("Hello".into()),color:Property::new("#ff0000".into()),caption:Property::new("Caption".into())});
     let mut f=Dynamic::new(vm.clone()).unwrap();
     assert_eq!(f.runtime().unwrap().control_bounds(3)[2],200.);
     f.update(|r|r.activate_control(0)).unwrap();assert!(vm.checked.get());
     f.update(|r|r.activate_control(3)).unwrap();assert_eq!(f.runtime().unwrap().control_bounds(3)[2],400.);
     vm.text.set("Hello\\nWorld".into());f.sync().unwrap();
     let calls=Rc::new(std::cell::Cell::new(0));let captured=calls.clone();
     f.on_edited(move |event|{assert_eq!(event.context.text.get(),"Edited");captured.set(captured.get()+1);});
     f.update(|r|{r.focus_control(2);r.text_key("a",false,true);r.text_insert("Edited");}).unwrap();assert_eq!(calls.get(),1);
     let bounds=f.runtime().unwrap().control_bounds(1);let y=bounds[1]+bounds[3]/2.;
     f.update(|r|r.pointer(bounds[0]+30.,y,1)).unwrap();let before=vm.amount.get();
     f.update(|r|r.pointer(bounds[0]+200.,y,0)).unwrap();assert!(vm.amount.get()>before);
     f.update(|r|r.pointer(bounds[0]+220.,y,2)).unwrap();
     let pixels=f.runtime().unwrap().pixels(500,600,1.);vm.color.set("#00ff00".into());assert!(pixels != f.runtime().unwrap().pixels(500,600,1.),"Changing a bound color must redraw supplied content");
     f.button().bind_property("width",Property::new(123.0_f64));assert_eq!(f.runtime().unwrap().control_bounds(3)[2],123.);
     vm.caption.set("New caption".into());assert_eq!(f.runtime().unwrap().control_label(3),"New caption");
     f.panel().clear();f.sync().unwrap();assert!(f.runtime().unwrap().control_disabled(3));
   }`;
   writeFileSync(join(directory,'live.rs'),live);
   const liveBuild=spawnSync('rustc',['--edition=2021','--extern',`forma=${rlib}`,'-L',`dependency=${dirname(rlib)}`,join(directory,'live.rs'),'-o',join(directory,'live')],{encoding:'utf8',timeout:120000,maxBuffer:8*1024*1024});assert.equal(liveBuild.status,0,liveBuild.stderr);
   const liveRun=spawnSync(join(directory,'live'),[],{encoding:'utf8',timeout:30000});assert.equal(liveRun.status,0,liveRun.stderr);
   const repeated=`component Repeated { contextType:'crate::RowsVm'; event edited(id:String,value:String); event chosen(id:String); Frame {width:420;height:600;padding:20;gap:8;
     if state.show { Button {key:'intro';text:'Intro';} }
     for row in state.rows key row.id {
       TextField {key:'editor';value <-> row.name;changed -> events.edited(row.id,row.name);}
       Button {key:'choose';width:max(100,state.width - 20);text:'\${row.name}: \${state.suffix}';clicked -> events.chosen(row.id);}
     } empty {Button {key:'empty';text:'Empty';}}
   } }`;
   const rows=`use forma::binding::Property;use std::rc::Rc;use std::cell::RefCell;
   #[derive(Clone,PartialEq)] pub struct Row {id:String,name:String}
   pub struct RowsVm {rows:Property<Vec<Row>>,show:Property<bool>,width:Property<f64>,suffix:Property<String>}
   fn row(id:&str,name:&str)->Row {Row{id:id.into(),name:name.into()}}
   `+generateForm({...files,'ui/Rows.ui':repeated},'ui/Rows.ui').rust+`
   fn main(){
     let vm=Rc::new(RowsVm{rows:Property::new(vec![row("a","Alpha"),row("b","Beta")]),show:Property::new(true),width:Property::new(320.),suffix:Property::new("ready".into())});
     let mut form=Repeated::new(vm.clone()).unwrap();
     assert_eq!(form.runtime().unwrap().control_count(),5);assert_eq!(form.runtime().unwrap().control_label(2),"Alpha: ready");
     let calls=Rc::new(RefCell::new(Vec::new()));let captured=calls.clone();
     form.on_edited(move |event|{captured.borrow_mut().push(format!("{}={}",event.id,event.value));});
     let chosen=Rc::new(RefCell::new(String::new()));let captured=chosen.clone();form.on_chosen(move |event|{*captured.borrow_mut()=event.id;});
     form.update(|r|{r.focus_control(1);r.text_key("a",false,true);r.text_insert("Edited");}).unwrap();
     assert_eq!(vm.rows.get()[0].name,"Edited");assert_eq!(&*calls.borrow(),&["a=Edited"]);
     let key=form.runtime().unwrap().control_key(1);
     let mut items=vm.rows.get();items.reverse();vm.rows.set(items);form.sync().unwrap();
     assert_eq!(form.runtime().unwrap().control_key(3),key);assert_eq!(form.runtime().unwrap().focused_index(),3);
     form.update(|r|r.activate_control(2)).unwrap();assert_eq!(&*chosen.borrow(),"b");
     vm.show.set(false);form.sync().unwrap();assert_eq!(form.runtime().unwrap().control_count(),4);assert_eq!(form.runtime().unwrap().focused_index(),2);
     vm.suffix.set("updated".into());vm.width.set(220.);form.sync().unwrap();assert_eq!(form.runtime().unwrap().control_label(1),"Beta: updated");assert_eq!(form.runtime().unwrap().control_bounds(1)[2],200.);
     vm.rows.set(vec![]);form.sync().unwrap();assert_eq!(form.runtime().unwrap().control_count(),1);assert_eq!(form.runtime().unwrap().control_label(0),"Empty");assert_eq!(form.runtime().unwrap().focused_index(),-1);
     vm.rows.set(vec![row("c","Gamma")]);form.sync().unwrap();form.update(|r|r.activate_control(1)).unwrap();assert_eq!(&*chosen.borrow(),"c");
     let next=Rc::new(RowsVm{rows:Property::new(vec![row("z","Zed")]),show:Property::new(false),width:Property::new(220.),suffix:Property::new("new".into())});form.set_context(next.clone()).unwrap();
     vm.rows.set(vec![row("old","Wrong")]);form.update(|r|r.activate_control(1)).unwrap();assert_eq!(&*chosen.borrow(),"z");assert_eq!(form.runtime().unwrap().control_label(1),"Zed: new");
     let current=next.clone();form.update(|r|{r.focus_control(0);r.text_key("a",false,true);r.text_insert("stale edit");current.rows.set(vec![row("y","Replacement")]);}).unwrap();
     assert_eq!(next.rows.get()[0].name,"Replacement");assert_eq!(calls.borrow().len(),1);
     next.rows.set(vec![row("dup","First"),row("dup","Second")]);assert!(form.sync().unwrap_err().contains("key"));
     next.rows.set(vec![row("fixed","Fixed")]);form.sync().unwrap();
   }`;
   writeFileSync(join(directory,'rows.rs'),rows);
   const rowsBuild=spawnSync('rustc',['--edition=2021','--extern',`forma=${rlib}`,'-L',`dependency=${dirname(rlib)}`,join(directory,'rows.rs'),'-o',join(directory,'rows')],{encoding:'utf8',timeout:120000,maxBuffer:8*1024*1024});assert.equal(rowsBuild.status,0,rowsBuild.stderr);
   const rowsRun=spawnSync(join(directory,'rows'),[],{encoding:'utf8',timeout:30000});assert.equal(rowsRun.status,0,rowsRun.stderr);
   const fixtures=[
     {
       name:'builtin',
       source:`component Builtin {contextType:'crate::Vm';Column {width:360;height:180;gap:8;Text {key:'label';text:'Name: \${state.name}';width:320;height:40;} TextInput {key:'input';value <-> state.name;width:320;height:40;}}}`,
       model:`use forma::binding::Property;use std::rc::Rc;pub struct Vm{name:Property<String>}`,
       body:`let vm=Rc::new(Vm{name:Property::new("Ada".into())});let mut form=Builtin::new(vm.clone()).unwrap();
         assert_eq!(form.runtime().unwrap().control_count(),2);assert_eq!(form.runtime().unwrap().control_label(0),"Name: Ada");
         form.update(|r|{r.focus_control(1);r.text_key("a",false,true);r.text_insert("Grace");}).unwrap();assert_eq!(vm.name.get(),"Grace");assert_eq!(form.runtime().unwrap().control_label(0),"Name: Grace");`,
       definitions:{},
     },
     {
       name:'nested',
       source:`component Nested {contextType:'crate::Vm';event edited(value:String);Frame {width:400;height:500;
         for group in state.groups key group.id {for row in group.rows key row.id {TextField {key:'edit';value <-> row.name;changed -> events.edited(row.name);}}}
         for label in state.labels key label {Button {key:'label';text:label;}}
       }}`,
       model:`use forma::binding::Property;use std::rc::Rc;
         #[derive(Clone,PartialEq)] pub struct Row{id:String,name:String}
         #[derive(Clone,PartialEq)] pub struct Group{id:String,rows:Vec<Row>}
         pub struct Vm {groups:Property<Vec<Group>>,labels:Property<Vec<String>>}`,
       body:`let vm=Rc::new(Vm{groups:Property::new(vec![Group{id:"g".into(),rows:vec![Row{id:"a".into(),name:"Alpha".into()}]}]),labels:Property::new(vec!["Tag".into()])});
         let mut form=Nested::new(vm.clone()).unwrap();assert_eq!(form.runtime().unwrap().control_count(),2);assert_eq!(form.runtime().unwrap().control_label(1),"Tag");
         form.update(|r|{r.focus_control(0);r.text_key("a",false,true);r.text_insert("Nested edit");}).unwrap();assert_eq!(vm.groups.get()[0].rows[0].name,"Nested edit");
         vm.groups.set(vec![]);assert_eq!(form.runtime().unwrap().control_count(),1);`,
       definitions:files,
     },
     {
       name:'typed',
       source:`component Typed {contextType:'crate::Vm';required prop title:String;event got(value:String?);Frame {width:400;height:300;TypedButton {key:'button';title:props.title;clicked -> events.got(state.maybe);} Button {key:'status';text:state.maybe ?? 'none';}}}`,
       model:`use forma::binding::Property;use std::rc::Rc;pub struct Vm{maybe:Property<Option<String>>}`,
       body:`let vm=Rc::new(Vm{maybe:Property::new(None)});assert!(Typed::new(vm.clone()).is_err());
         let mut form=Typed::with_properties(vm.clone(),forma::form_document::props(vec![("title",forma::form_document::Value::Text("Hello".into()))])).unwrap();
         assert_eq!(form.runtime().unwrap().control_label(0),"Hello!");assert_eq!(form.runtime().unwrap().control_label(1),"none");
         let received=Rc::new(std::cell::RefCell::new(Some("initial".to_string())));let captured=received.clone();form.on_got(move|event|{*captured.borrow_mut()=event.value;});
         form.update(|r|r.activate_control(0)).unwrap();assert!(received.borrow().is_none());vm.maybe.set(Some("Ready".into()));form.update(|r|r.activate_control(0)).unwrap();assert_eq!(received.borrow().as_deref(),Some("Ready"));
         assert!(Typed::with_properties(vm,forma::form_document::props(vec![("title",forma::form_document::Value::Number(1.))])).is_err());`,
       definitions:{...files,'components/TypedButton.ui':`component TypedButton : Button {required prop title:String;prop suffix:String='!';text:'\${props.title}\${props.suffix}';}`},
     },
     {
       name:'scalars',
       source:`component Scalars {contextType:'crate::Vm';Frame {width:400;height:300;TabButton {key:'tab';selected <-> state.selected;} Checkbox {key:'check';checked <-> state.checked;} Slider {key:'slider';value <-> state.amount;} for item in state.items key item.id {Checkbox {key:'rowCheck';checked <-> item.checked;} Slider {key:'rowRange';value <-> item.amount;} TabButton {key:'rowTab';selected <-> item.selected;}}}}`,
       model:`use forma::binding::Property;use std::rc::Rc;#[derive(Clone,PartialEq)] pub struct Entry{id:String,checked:bool,selected:bool,amount:f32} pub struct Vm{selected:Property<bool>,checked:Property<bool>,amount:Property<f32>,items:Property<Vec<Entry>>}`,
       body:`let vm=Rc::new(Vm{selected:Property::new(false),checked:Property::new(false),amount:Property::new(0.25),items:Property::new(vec![Entry{id:"row".into(),checked:false,selected:false,amount:0.5}])});let mut form=Scalars::new(vm.clone()).unwrap();
         form.update(|r|r.activate_control(0)).unwrap();assert!(vm.selected.get());form.update(|r|r.activate_control(0)).unwrap();assert!(vm.selected.get());
         form.update(|r|r.activate_control(1)).unwrap();assert!(vm.checked.get());form.update(|r|r.activate_control(1)).unwrap();assert!(!vm.checked.get());
         let b=form.runtime().unwrap().control_bounds(2);form.update(|r|r.pointer(b[0]+100.,b[1]+b[3]/2.,1)).unwrap();form.update(|r|r.pointer(b[0]+180.,b[1]+b[3]/2.,2)).unwrap();assert!(vm.amount.get()>0.25);
         assert_eq!(form.runtime().unwrap().range_value(4),0.5);form.update(|r|r.activate_control(3)).unwrap();assert!(vm.items.get()[0].checked);form.update(|r|r.activate_control(3)).unwrap();assert!(!vm.items.get()[0].checked);
         form.update(|r|r.activate_control(5)).unwrap();assert!(vm.items.get()[0].selected);
         let b=form.runtime().unwrap().control_bounds(4);form.update(|r|r.pointer(b[0]+100.,b[1]+b[3]/2.,1)).unwrap();form.update(|r|r.pointer(b[0]+220.,b[1]+b[3]/2.,0)).unwrap();form.update(|r|r.pointer(b[0]+220.,b[1]+b[3]/2.,2)).unwrap();assert!(vm.items.get()[0].amount>0.5);`,
       definitions:themed,
     },
   ];
   for(const fixture of fixtures){
     const code=fixture.model+generateForm({...fixture.definitions,'ui/Fixture.ui':fixture.source},'ui/Fixture.ui').rust+'fn main(){'+fixture.body+'}';
     const input=join(directory,fixture.name+'.rs'),output=join(directory,fixture.name);writeFileSync(input,code);
     const compiled=spawnSync('rustc',['--edition=2021','--extern',`forma=${rlib}`,'-L',`dependency=${dirname(rlib)}`,input,'-o',output],{encoding:'utf8',timeout:120000,maxBuffer:8*1024*1024});assert.equal(compiled.status,0,fixture.name+': '+compiled.stderr);
     const executed=spawnSync(output,[],{encoding:'utf8',timeout:30000});assert.equal(executed.status,0,fixture.name+': '+executed.stderr);
   }
   const typo=compile(files[entry].replace('state.name','state.naem'));
   assert.notEqual(typo.status,0);assert.match(typo.stderr,/no field `naem`/);
   const wrongType=compile(files[entry].replace('state.busy','state.name'));
   assert.notEqual(wrongType.status,0);assert.match(wrongType.stderr,/mismatched types/);
   const wrongNumber=compile(files[entry].replace('value <-> state.name','width: state.name'));
   assert.notEqual(wrongNumber.status,0);assert.match(wrongNumber.stderr,/ModelNumber/);
   const nested=compile(files[entry].replace("contextType: 'crate::AddressVm';\n            context: state.address;\n            value <-> state.city;",'value <-> state.address.city;'));
   assert.equal(nested.status,0,nested.stderr);
   const empty=compile("component Empty { contextType:'crate::CustomerVm'; Frame { width:100; height:100; } }");
   assert.equal(empty.status,0,empty.stderr);
   const wrongContext=compile(files[entry].replace("contextType: 'crate::AddressVm'","contextType: 'crate::CustomerVm'"));
   assert.notEqual(wrongContext.status,0);assert.match(wrongContext.stderr,/mismatched types|no field `city`/);
 }finally{rmSync(directory,{recursive:true,force:true});}
});


test('project staging generates all forms and maps Rust field errors to UI locations',()=>{
 const input={...files,'Cargo.toml':'[package]\nname="demo"\nversion="0.1.0"\n[dependencies]\nforma = { path = "../.." }','src/main.rs':'include!("forma_generated.rs"); fn main() {}'};
 const result=prepareFormProject(input);assert.equal(result.count,1);assert.ok(result.files['src/forma_generated.rs']);assert.ok(!input['src/forma_generated.rs']);
 const [line,location]=Object.entries(result.sourceMap)[0];assert.ok(location.file.endsWith('.ui'));
 assert.match(mapFormDiagnostic(`--> /tmp/project/src/forma_generated.rs:${line}:10`,result.sourceMap),/ui\/CustomerForm.ui:/);
});
