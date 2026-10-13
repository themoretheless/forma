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
// A literal row shows its value as text but writes it back as the value the source holds, so the
// text is checked against the shape the row was read in. Batch rows ask for the same rule.
export function literalValue(field,text){
  if(typeof field.value==='number'){
    const value=Number(text);
    if(!Number.isFinite(value))throw Error('Некорректное значение свойства');
    return value;
  }
  if(typeof field.value==='boolean'){
    if(!['true','false'].includes(text.trim()))throw Error('Ожидается true или false');
    return text.trim()==='true';
  }
  return text;
}
// What a property holds is decided by the text around its own range: a design token, a colour,
// a literal, an expression. That shape is what the field validates against and how it quotes a
// write back; where the write goes is the row's own business. A design entry is a node like a
// markup control is, so the same reading works in the base block and in a state block.
export function classify(source,origin,key){
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
// Puts exactly `nodes` in `parent`, in order. What leaves goes first, so the nodes that stay keep
// their places and only a node that really changed position is moved.
function place(parent,nodes){
  const keep=new Set(nodes);
  for(const child of [...parent.children])if(!keep.has(child))child.remove();
  nodes.forEach((node,i)=>{const at=parent.children[i];if(at!==node)parent.insertBefore(node,at??null);});
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
  const element=(tag,className,text)=>{const el=document.createElement(tag);if(className)el.className=className;el.textContent=text;return el;};
  function edit(field,text,input){
    try{
      freshen();
      const at=target(field);
      const value=field.shape==='literal'?literalValue(field,text):text;
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
  // What a row is made of, as opposed to what it shows: rows of the same shape differ only in text,
  // values and the field their handlers write, so one is refilled for another.
  const rowShape=field=>[field.kind,field.layer&&field.layer!=='markup'?field.layer:'',
    field.commit===null||!current.editable?'view':field.kind==='color'?pickerHex(field.value)?'picker':'hint':''].join('|');
  function row(field){
    const label=document.createElement('label'),name=document.createElement('span');label.append(name);
    const rec={label,shape:rowShape(field),field,name};
    // A design file puts its values in blocks the markup never holds, so the panel says which one a
    // row reads from and writes back to; a plain markup row needs no badge.
    if(field.layer&&field.layer!=='markup'){rec.badge=label.appendChild(document.createElement('span'));rec.badge.className='property-layer';}
    if(field.commit===null||!current.editable){
      rec.out=label.appendChild(document.createElement('output'));rec.hint=label.appendChild(document.createElement('small'));
      return rec;
    }
    const input=rec.input=document.createElement('input');
    if(field.kind==='color'){
      const chip=document.createElement('span');chip.className='color-chip';
      rec.fill=chip.appendChild(document.createElement('span'));label.append(chip);
      if(pickerHex(field.value)){
        const picker=rec.picker=document.createElement('input');picker.type='color';
        picker.onchange=()=>{input.value=picker.value;edit(rec.field,picker.value,input);};
        label.append(picker);
      }else label.append(element('small','','Формат цвета сохраняется как написан'));
    }
    if(field.kind==='token'){input.setAttribute('list','forma-design-tokens');rec.value=label.appendChild(document.createElement('small'));rec.value.className='token-value';}
    input.onchange=()=>edit(rec.field,input.value,input);
    label.append(input);
    const remove=rec.remove=element('button','property-remove','−');remove.type='button';
    remove.onclick=()=>declare(rec.field.commit==='state',source=>removeEdit(rec.field,source));
    label.append(remove);
    return rec;
  }
  // Writes only what differs: a text or attribute set to the value it has would still reach the
  // shell's observer and repaint a control that did not change.
  const text=(el,value)=>{if(el.textContent!==value)el.textContent=value;};
  const attr=(el,key,value)=>{if(el.getAttribute(key)!==value)el.setAttribute(key,value);};
  const prop=(el,key,value)=>{if(el[key]!==value)el[key]=value;};
  function fill(rec,field){
    rec.field=field;
    // The layer rides on the class as well, so a design row is recognisable without its badge.
    prop(rec.label,'className',`property ${field.kind}${field.layer&&field.layer!=='markup'?` ${field.layer}`:''}`);
    text(rec.name,field.key);
    if(rec.badge){text(rec.badge,layerLabel(field.layer));prop(rec.badge,'title',`Значение переопределено в ${layerLabel(field.layer)} дизайн-файла и правится там же`);}
    if(rec.out){
      text(rec.out,field.text);
      text(rec.hint,field.commit===null?'Значение задаётся раскладкой или объявлением, не в этом исходнике':'Только просмотр: правка недоступна для этого сценария');
      return rec.label;
    }
    const input=rec.input;
    prop(input,'value',field.text);input.setCustomValidity('');
    attr(input,'aria-label',field.key);
    if(field.kind==='binding'||field.kind==='handler')prop(input.dataset,'link',field.key);
    else prop(input.dataset,'prop',field.key);
    if(rec.fill)prop(rec.fill.style,'backgroundColor',field.css);
    if(rec.picker){prop(rec.picker,'value',pickerHex(field.value));attr(rec.picker,'aria-label',`Палитра ${field.key}`);}
    if(rec.value){
      if(hasDesignData(field.ref)){
        const resolved=String(readDesignData(field.ref));
        text(rec.value,`= ${resolved.length>80?`${resolved.slice(0,80)}…`:resolved}`);prop(rec.value,'title',resolved);
      }else{text(rec.value,'значения нет: нажмите «↻ Данные дизайна»');prop(rec.value,'title','');}
    }
    prop(rec.remove,'title',`Убрать ${field.key} из ${field.layer==='markup'?'разметки этого элемента':layerLabel(field.layer)}`);
    attr(rec.remove,'aria-label',`Убрать свойство ${field.key}`);
    return rec.label;
  }
  // Only names the compiler accepts for this type that the block does not declare yet, each paired
  // with the markup text it starts from. With a state on screen the designer is building that state,
  // so a new declaration goes into it; without one the form writes the component's own markup.
  function addableNames(declared){
    const state=current.state;
    // A state entry is created under the selected control's own key, so `key` is never a thing to
    // add here; every other name the state does not patch yet is an override waiting to be made.
    return addableProperties(current.node.type,state?[...Object.keys(state.entry?.props??{}),'key']:declared);
  }
  // One form, refilled for every selection and reset to its first name as a new one would be.
  const form=(()=>{
    const wrap=element('div','property-add',''),none=document.createElement('small');
    const pick=document.createElement('select');pick.setAttribute('aria-label','Свойство для добавления');
    const value=document.createElement('input');value.setAttribute('aria-label','Начальное значение');
    const button=document.createElement('button');button.type='button';
    let names=null;
    const sync=()=>{prop(value,'value',propertyStart[pick.value]);
      text(button,`Добавить ${pick.value}${current.state?` в ${layerLabel('state')}`:''}`);};
    pick.onchange=()=>{sync();notice.textContent='';value.focus();};
    button.onclick=()=>{const state=current.state;declare(!!state,source=>state
      ?statePropertyEdit(source,state.name,current.node.type,current.node.props.key,pick.value,value.value)
      :propertyAddEdit(source,current.node.start,pick.value,value.value));};
    value.onkeydown=event=>{if(event.key==='Enter'){event.preventDefault();button.onclick();}};
    return {el:wrap,fill(next){
      if(names?.join('\n')!==next.join('\n')){names=next;pick.replaceChildren(...names.map(name=>new Option(name,name)));}
      prop(pick,'disabled',!names.length);prop(value,'disabled',!names.length);
      if(!names.length){
        text(none,current.state?'Состояние переопределяет все свойства этого типа':'Все свойства этого типа уже объявлены');
        prop(value,'value','');place(wrap,[none,pick,value]);
      }else{prop(pick,'value',names[0]);sync();place(wrap,[pick,value,button]);}
      return wrap;
    }};
  })();
  // Pieces that every selection shows again are kept rather than rebuilt: an element that stays in
  // the document keeps its layout, its painted shell surface and its observers.
  const empty=element('small','inspector-empty','Контрол не выбран'),title=document.createElement('h3'),note=document.createElement('small');
  const propertiesLabel=element('div','inspector-label','СВОЙСТВА'),linksLabel=element('div','inspector-label','ПРИВЯЗКИ И СОБЫТИЯ'),noLinks=element('small','','Нет привязок');
  let rows=[],tokenNames=null;
  // The last render's rows, by shape; a field takes one with its own name first, so a focused or
  // half-typed field stays with the property it belongs to.
  function rowsFor(fields){
    const spare=new Map();for(const rec of rows){const list=spare.get(rec.shape);if(list)list.push(rec);else spare.set(rec.shape,[rec]);}
    rows=fields.map(field=>{
      const list=spare.get(rowShape(field)),rec=list?.find(r=>r.field.key===field.key)??list?.[0];
      if(rec)list.splice(list.indexOf(rec),1);
      return rec??row(field);
    });
    return rows.map((rec,i)=>fill(rec,fields[i]));
  }
  function render(state){
    current=state;
    // A panel with no control to describe says so instead of keeping the rows it last drew: after an
    // undo took the selected control out of the file, those rows would write into markup that is gone.
    if(!current.node){place(container,[empty]);tokens.replaceChildren();tokenNames=null;return;}
    text(title,current.node.type);
    if(current.note)text(note,current.note);
    const names=designTokens();
    if(names.join('\n')!==tokenNames){tokenNames=names.join('\n');tokens.replaceChildren(...names.map(name=>{const option=document.createElement('option');option.value=name;return option;}));}
    const fields=propertyFields(current.node,current.source,current.state);
    const values=fields.filter(field=>field.commit!=='statement');
    const links=fields.filter(field=>field.commit==='statement');
    notice.textContent='';
    const [valueRows,linkRows]=(all=>[all.slice(0,values.length),all.slice(values.length)])(rowsFor([...values,...links]));
    place(container,[title,...(current.note?[note]:[]),tokens,propertiesLabel,...valueRows,
      // A binding or handler already occupies its name in the block, so it is not a candidate for
      // declaring twice, and neither is a design row: the markup the form writes to has no statement
      // under that name yet. A row with no place to write to is the only one not counted at all.
      ...(current.editable?[form.fill(addableNames(fields.filter(field=>field.layer==='markup'&&field.commit!==null).map(field=>field.key))),notice]:[]),
      linksLabel,...(links.length?linkRows:[noLinks])]);
  }
  return {render};
}
