import {designReference,hasDesignData,readDesignData} from './design-data.js';
import {colorValue,isColorProperty,pickerHex} from './color-values.js';
import {serializeValue} from './expressions.js';
import {sourceNode,propertyEdit,valueTextEdit,statementTextEdit,statementParts} from './property-edit.js';

function valueText(value){
  if(value===null||value===undefined)return 'пусто';
  return typeof value==='object'?serializeValue(value):String(value);
}
// The inspector edits the definition, so a field is described by what the source
// actually holds: its own text, its range, and how it may be written back. A property
// without a range in this file (computed geometry, an optional declaration) has no
// place to write to and stays read-only.
export function propertyFields(node,source){
  const origin=sourceNode(source,node?.start);
  const fields=[];
  for(const [key,value]of Object.entries(node?.props??{})){
    const range=origin?.propertyRanges?.[key];
    // A bound name still reaches the preview as a computed prop; the statement below is
    // where it is written, so the preview value would only add a second, frozen row.
    if(!range){if(origin?.statementRanges?.[key])continue;fields.push({key,kind:'computed',text:valueText(value),commit:null});continue;}
    const declared=origin.props[key],text=source.slice(range.from,range.to);
    const ref=designReference(declared);
    if(ref){fields.push({key,kind:'token',text,ref,commit:'value'});continue;}
    // A bare `#e8edf7` is a single-token expression to the parser, not a string, so a
    // color is recognised from the text it carries rather than from its JSON type.
    const literal=typeof declared==='string'?declared:typeof declared?.expr==='string'?declared.expr:null;
    const color=literal===null||!isColorProperty(key)?null:colorValue(literal);
    if(color){fields.push({key,kind:'color',value:literal,text:literal,css:color,commit:'literal'});continue;}
    if(typeof declared==='string'){fields.push({key,kind:'string',value:declared,text:declared,commit:'literal'});continue;}
    if(typeof declared==='number'||typeof declared==='boolean'){fields.push({key,kind:typeof declared,value:declared,text:String(declared),commit:'literal'});continue;}
    fields.push({key,kind:'expression',text,commit:'value'});
  }
  for(const [key,range]of Object.entries(origin?.statementRanges??{})){
    if(origin.propertyRanges[key])continue;
    const parts=statementParts(source,range);
    if(!parts||parts.op===':')continue;
    fields.push({key,kind:parts.op==='<->'?'binding':'handler',op:parts.op,text:parts.rhs,commit:'statement'});
  }
  return fields;
}
export function createPropertyInspector({container,commit,designTokens=()=>[],onError=()=>{}}){
  const tokens=document.createElement('datalist');tokens.id='forma-design-tokens';
  let current=null;
  function heading(text){const label=document.createElement('div');label.className='inspector-label';label.textContent=text;container.append(label);}
  function edit(field,text,input){
    try{
      let change;
      if(field.commit==='literal'){
        let value=text;
        if(typeof field.value==='number'){value=Number(text);if(!Number.isFinite(value))throw Error('Некорректное значение свойства');}
        else if(typeof field.value==='boolean'){if(!['true','false'].includes(text.trim()))throw Error('Ожидается true или false');value=text.trim()==='true';}
        change=propertyEdit(current.source,current.node.start,field.key,value);
      }else if(field.commit==='value'){
        change=valueTextEdit(current.source,current.node.start,field.key,text);
      }else change=statementTextEdit(current.source,current.node.start,field.key,text);
      input.setCustomValidity('');
      commit({file:current.path,source:current.source,...change});
    }catch(error){
      input.setCustomValidity(error.message);
      input.reportValidity();
      onError(error.message);
    }
  }
  function row(field){
    const label=document.createElement('label');
    label.className=`property ${field.kind}`;
    const name=document.createElement('span');name.textContent=field.key;label.append(name);
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
    return label;
  }
  function render(state){
    current=state;
    container.replaceChildren();
    const title=document.createElement('h3');title.textContent=current.node.type;container.append(title);
    if(current.note){const note=document.createElement('small');note.textContent=current.note;container.append(note);}
    tokens.replaceChildren();
    for(const name of designTokens()){const option=document.createElement('option');option.value=name;tokens.append(option);}
    container.append(tokens);
    const fields=propertyFields(current.node,current.source);
    const values=fields.filter(field=>field.commit!=='statement');
    const links=fields.filter(field=>field.commit==='statement');
    heading('СВОЙСТВА');
    for(const field of values)container.append(row(field));
    heading('ПРИВЯЗКИ И СОБЫТИЯ');
    if(!links.length){const none=document.createElement('small');none.textContent='Нет привязок';container.append(none);}
    for(const field of links)container.append(row(field));
  }
  return {render};
}
