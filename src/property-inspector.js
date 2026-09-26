import {designReference,hasDesignData,readDesignData} from './design-data.js';
import {colorValue,isColorProperty,pickerHex} from './color-values.js';
import {serializeValue} from './expressions.js';
import {sourceNode,propertyEdit,valueTextEdit,statementTextEdit,statementParts,propertyAddEdit,propertyRemoveEdit,stringLiteral} from './property-edit.js';
import {findEntry,statePropertyEdit,stateEntryRemoveEdit} from './design-states.js';
import {addableProperties,propertyStart} from './property-catalog.js';

function valueText(value){
  if(value===null||value===undefined)return 'пусто';
  return typeof value==='object'?serializeValue(value):String(value);
}
// What a property holds is decided by the text around its own range: a design token, a colour,
// a literal, an expression. That shape is what the field validates against and how it quotes a
// write back; where the write goes is the row's own business. A design entry is a node like a
// markup control is, so the same reading works in the base block and in a state block.
function classify(source,origin,key){
  const range=origin?.propertyRanges?.[key];
  if(!range)return null;
  const declared=origin.props[key],text=source.slice(range.from,range.to);
  const ref=designReference(declared);
  if(ref)return {kind:'token',text,ref,shape:'value'};
  // A bare `#e8edf7` is a single-token expression to the parser, not a string, so a
  // color is recognised from the text it carries rather than from its JSON type.
  const literal=typeof declared==='string'?declared:typeof declared?.expr==='string'?declared.expr:null;
  const color=literal===null||!isColorProperty(key)?null:colorValue(literal);
  if(color)return {kind:'color',value:literal,text:literal,css:color,shape:'literal'};
  if(typeof declared==='string')return {kind:'string',value:declared,text:declared,shape:'literal'};
  if(typeof declared==='number'||typeof declared==='boolean')return {kind:typeof declared,value:declared,text:String(declared),shape:'literal'};
  return {kind:'expression',text,shape:'value'};
}
// The inspector edits the definition, so a field is described by what the source
// actually holds: its own text, its range, and how it may be written back. A property
// without a range in this file (computed geometry, an optional declaration) has no
// place to write to and stays read-only.
// `state` names the design layer the preview shows: `{name,path,source,entry,base}` for the
// active state and the base entry of the same design key. Then a property the state already
// patches reads from the state, one the base patches reads from the base, and the rest still
// read from the markup — but every one of them writes as a state override, which is what a
// designer expects while a state is on screen.
export function propertyFields(node,source,state=null){
  const origin=sourceNode(source,node?.start);
  const fields=[];
  const overrides=[];
  if(state&&state.entry)overrides.push({layer:'state',from:state.entry});
  if(state&&state.base)overrides.push({layer:'base',from:state.base});
  const names=new Set([...Object.keys(node?.props??{}),...overrides.flatMap(o=>Object.keys(o.from.props))]);
  for(const key of names){
    // The design key is the address the override hangs on: renaming it here would detach the
    // component control from the state row that shows it.
    const override=key==='key'?null:overrides.find(other=>other.from.propertyRanges?.[key]);
    // `commit` says which document the row writes and `shape` how the value there is held, since a
    // design row needs both: the block to splice is its layer's, the quoting rule is the block's.
    if(override){fields.push({key,layer:override.layer,...classify(state.source,override.from,key),commit:'state'});continue;}
    const value=node.props[key];
    const range=origin?.propertyRanges?.[key];
    // A bound name still reaches the preview as a computed prop; the statement below is
    // where it is written, so the preview value would only add a second, frozen row.
    if(!range){if(origin?.statementRanges?.[key])continue;fields.push({key,layer:'markup',kind:'computed',text:valueText(value),commit:null});continue;}
    const read=classify(source,origin,key);
    fields.push({key,layer:'markup',...read,commit:read.shape});
  }
  for(const [key,range]of Object.entries(origin?.statementRanges??{})){
    if(origin.propertyRanges[key])continue;
    const parts=statementParts(source,range);
    if(!parts||parts.op===':')continue;
    // A design file holds values, never bindings or handlers, so these rows always write markup.
    fields.push({key,layer:'markup',kind:parts.op==='<->'?'binding':'handler',op:parts.op,text:parts.rhs,shape:'statement',commit:'statement'});
  }
  return fields;
}
export function createPropertyInspector({container,commit,designTokens=()=>[],onError=()=>{},liveSource=null}){
  const tokens=document.createElement('datalist');tokens.id='forma-design-tokens';
  // Declaring and undeclaring a property changes the shape of the panel, so its failures are
  // reported inside the panel rather than through a field that is about to disappear.
  const notice=document.createElement('small');notice.className='property-notice';notice.setAttribute('role','status');
  let current=null;
  // A commit rewrites the editor but leaves the row on screen, so the snapshot the panel was
  // rendered from is one change behind the file. Each edit re-reads the live document and
  // resolves the node again in it; if that stops working the commit guard says so instead of
  // splicing offsets that no longer describe the file.
  function freshen(){
    if(!current)return;
    const markup=liveSource?.(current.path);
    if(typeof markup==='string'&&markup!==current.source){
      try{const node=sourceNode(markup,current.node.start);if(node)current={...current,node,source:markup};}
      catch{/* the snapshot stays, and the host refuses the commit */}
    }
    // A state row splices the design file, which is a second document with its own live text: the
    // markup offsets say nothing about it. Its entries are found again by the design key, the one
    // address a state override hangs on, so a row drawn before an earlier edit stays writable.
    const state=current.state;
    if(!state)return;
    const design=liveSource?.(state.path);
    if(typeof design!=='string'||design===state.source)return;
    try{
      const key=current.node.props.key;
      current={...current,state:{...state,source:design,entry:findEntry(design,state.name,key),base:findEntry(design,null,key)}};
    }catch{/* as above: the host refuses rather than cutting at offsets the file no longer has */}
  }
  // Which document and which block a row writes: the state and base blocks of the design file, or
  // the component's markup. A row reads from its layer and writes back to the same one, so the
  // badge on it is also the place its value lives.
  function target(field){
    if(field.commit!=='state')return current;
    const {path,source,name}=current.state;
    // The design file's default block is the one a splice selects with a null state name.
    return {path,source,state:field.layer==='state'?name:null};
  }
  function heading(text){const label=document.createElement('div');label.className='inspector-label';label.textContent=text;container.append(label);}
  function edit(field,text,input){
    try{
      freshen();
      const at=target(field);
      let value=text;
      if(field.shape==='literal'){
        if(typeof field.value==='number'){value=Number(text);if(!Number.isFinite(value))throw Error('Некорректное значение свойства');}
        else if(typeof field.value==='boolean'){if(!['true','false'].includes(text.trim()))throw Error('Ожидается true или false');value=text.trim()==='true';}
      }
      // A design block holds no node of the markup file to cut into: the entry it writes is located
      // by the control's type and design key, and a string the field shows is quoted back as markup.
      const change=field.commit==='state'
        ?statePropertyEdit(at.source,at.state,current.node.type,current.node.props.key,field.key,
          field.kind==='string'?stringLiteral(value):text)
        :field.shape==='literal'?propertyEdit(current.source,current.node.start,field.key,value)
        :field.shape==='value'?valueTextEdit(current.source,current.node.start,field.key,text)
        :statementTextEdit(current.source,current.node.start,field.key,text);
      input.setCustomValidity('');
      commit({file:at.path,source:at.source,...change,...(field.commit==='state'?{reselect:current.node.start}:{})});
    }catch(error){
      input.setCustomValidity(error.message);
      input.reportValidity();
      onError(error.message);
    }
  }
  // Declaring and undeclaring changes the shape of the panel, so the failure is reported here
  // rather than through a field that is about to disappear. `reselect` asks the host to rebuild the
  // rows from the AST the editor is about to compile. The design file is chosen only after the
  // refresh, since an earlier commit from this same panel moved the snapshot it was taken from.
  function declare(inDesign,build){
    notice.textContent='';
    try{
      freshen();
      const at=inDesign?current.state:current;
      commit({file:at.path,source:at.source,...build(at.source),reselect:current.node.start});
    }catch(error){notice.textContent=error.message;}
  }
  // `−` hands the layer below back: dropping a state override shows the base value again, dropping
  // a base one shows the markup. An entry that patches only this property exists just for it, so it
  // goes away entirely instead of staying behind as an empty block.
  function removeEdit(field,source){
    if(field.commit!=='state')return propertyRemoveEdit(source,current.node.start,field.key);
    const {name,entry,base}=current.state;
    const [block,node]=field.layer==='state'?[name,entry]:[null,base];
    // An entry the file dropped while its row stayed on screen: asking the block to remove it is what
    // names the loss, rather than counting properties that are no longer there.
    if(!node)return stateEntryRemoveEdit(source,block,current.node.props.key);
    // The key is how the entry is found, not a value it overrides, so an entry left with only it
    // would patch nothing while still shadowing the layer below.
    return Object.keys(node.propertyRanges).filter(key=>key!=='key').length>1
      ?propertyRemoveEdit(source,node.start,field.key)
      :stateEntryRemoveEdit(source,block,current.node.props.key);
  }
  // What the layer is called in the panel's own words.
  const layerLabel=layer=>layer==='state'?`состояние «${current.state.name}»`:'базовые значения';
  function row(field){
    const label=document.createElement('label');
    // The layer rides on the class as well, so a design row is recognisable without its badge.
    label.className=`property ${field.kind}${field.layer&&field.layer!=='markup'?` ${field.layer}`:''}`;
    const name=document.createElement('span');name.textContent=field.key;label.append(name);
    // A design file puts its values in blocks the markup never holds, so the panel says which one a
    // row reads from and writes back to; a plain markup row needs no badge.
    if(field.layer&&field.layer!=='markup'){
      const badge=document.createElement('span');badge.className='property-layer';
      badge.textContent=layerLabel(field.layer);
      badge.title=`Значение переопределено в ${layerLabel(field.layer)} дизайн-файла и правится там же`;
      label.append(badge);
    }
    if(field.commit===null||!current.editable){
      const out=document.createElement('output');out.textContent=field.text;label.append(out);
      const hint=document.createElement('small');
      hint.textContent=field.commit===null?'Значение задаётся раскладкой или объявлением, не в этом исходнике':'Только просмотр: правка недоступна для этого сценария';
      label.append(hint);
      return label;
    }
    const input=document.createElement('input');
    input.value=field.text;
    input.setAttribute('aria-label',field.key);
    if(field.kind==='color'){
      const chip=document.createElement('span');chip.className='color-chip';
      const fill=document.createElement('span');fill.style.backgroundColor=field.css;chip.append(fill);label.append(chip);
      const hex=pickerHex(field.value);
      if(hex){
        const picker=document.createElement('input');
        picker.type='color';picker.value=hex;picker.setAttribute('aria-label',`Палитра ${field.key}`);
        picker.onchange=()=>{input.value=picker.value;edit(field,picker.value,input);};
        label.append(picker);
      }else{
        const hint=document.createElement('small');hint.textContent='Формат цвета сохраняется как написан';label.append(hint);
      }
    }
    if(field.kind==='token'){
      input.setAttribute('list','forma-design-tokens');
      const value=document.createElement('small');value.className='token-value';
      if(hasDesignData(field.ref)){
        const resolved=String(readDesignData(field.ref));
        value.textContent=`= ${resolved.length>80?`${resolved.slice(0,80)}…`:resolved}`;
        value.title=resolved;
      }else value.textContent='значения нет: нажмите «↻ Данные дизайна»';
      label.append(value);
    }
    if(field.kind==='binding'||field.kind==='handler')input.dataset.link=field.key;
    else input.dataset.prop=field.key;
    input.onchange=()=>edit(field,input.value,input);
    label.append(input);
    const remove=document.createElement('button');
    remove.type='button';remove.className='property-remove';remove.textContent='−';
    remove.title=`Убрать ${field.key} из ${field.layer==='markup'?'разметки этого элемента':layerLabel(field.layer)}`;
    remove.setAttribute('aria-label',`Убрать свойство ${field.key}`);
    remove.onclick=()=>declare(field.commit==='state',source=>removeEdit(field,source));
    label.append(remove);
    return label;
  }
  // Only names the compiler accepts for this type that the block does not declare yet, each paired
  // with the markup text it starts from. With a state on screen the designer is building that state,
  // so a new declaration goes into it; without one the form writes the component's own markup.
  function addForm(declared){
    const state=current.state;
    // A state entry is created under the selected control's own key, so `key` is never a thing to
    // add here; every other name the state does not patch yet is an override waiting to be made.
    const names=addableProperties(current.node.type,state?[...Object.keys(state.entry?.props??{}),'key']:declared);
    const wrap=document.createElement('div');wrap.className='property-add';
    const pick=document.createElement('select');pick.setAttribute('aria-label','Свойство для добавления');
    const value=document.createElement('input');value.setAttribute('aria-label','Начальное значение');
    const button=document.createElement('button');button.type='button';
    if(!names.length){
      pick.disabled=true;value.disabled=true;
      const none=document.createElement('small');
      none.textContent=state?'Состояние переопределяет все свойства этого типа':'Все свойства этого типа уже объявлены';
      wrap.append(none,pick,value);
      return wrap;
    }
    for(const name of names)pick.append(new Option(name,name));
    const sync=()=>{value.value=propertyStart[pick.value];
      button.textContent=`Добавить ${pick.value}${state?` в ${layerLabel('state')}`:''}`;};
    pick.onchange=()=>{sync();notice.textContent='';value.focus();};
    button.onclick=()=>declare(!!state,source=>state
      ?statePropertyEdit(source,state.name,current.node.type,current.node.props.key,pick.value,value.value)
      :propertyAddEdit(source,current.node.start,pick.value,value.value));
    value.onkeydown=event=>{if(event.key==='Enter'){event.preventDefault();button.onclick();}};
    sync();
    wrap.append(pick,value,button);
    return wrap;
  }
  function render(state){
    current=state;
    container.replaceChildren();
    const title=document.createElement('h3');title.textContent=current.node.type;container.append(title);
    if(current.note){const note=document.createElement('small');note.textContent=current.note;container.append(note);}
    tokens.replaceChildren();
    for(const name of designTokens()){const option=document.createElement('option');option.value=name;tokens.append(option);}
    container.append(tokens);
    const fields=propertyFields(current.node,current.source,current.state);
    const values=fields.filter(field=>field.commit!=='statement');
    const links=fields.filter(field=>field.commit==='statement');
    heading('СВОЙСТВА');
    notice.textContent='';
    for(const field of values)container.append(row(field));
    // A binding or handler already occupies its name in the block, so it is not a candidate for
    // declaring twice, and neither is a design row: the markup the form writes to has no statement
    // under that name yet. A row with no place to write to is the only one not counted at all.
    if(current.editable)container.append(addForm(fields.filter(field=>field.layer==='markup'&&field.commit!==null).map(field=>field.key)),notice);
    heading('ПРИВЯЗКИ И СОБЫТИЯ');
    if(!links.length){const none=document.createElement('small');none.textContent='Нет привязок';container.append(none);}
    for(const field of links)container.append(row(field));
  }
  return {render};
}
