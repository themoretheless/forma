import {classify,literalValue} from './property-inspector.js';
import {sourceNode,propertyEdit} from './property-edit.js';
import {editElements} from './element-edit.js';

// The batch is addressed by the offsets the selection holds, so a row has to describe the same thing
// in every one of the controls: a name only some of them declare, or one they declare in different
// shapes (a design token here, a plain number there, an expression bound to state), has no single
// value to write, so it gets no row.
export function sharedFields(source,starts){
  if(starts.length<2)return [];
  const origins=starts.map(start=>sourceNode(source,start));
  if(origins.some(origin=>!origin))return [];
  const fields=[];
  for(const key of new Set(origins.flatMap(origin=>Object.keys(origin.props)))){
    // The design key is how the panel, the states and the preview address a control. Writing one value
    // into all of them at once would collide with itself, so it is never a batch row.
    if(key==='key')continue;
    const reads=origins.map(origin=>classify(source,origin,key));
    if(reads.some(read=>!read||read.shape!=='literal'||read.kind!==reads[0].kind))continue;
    const [first]=reads;
    fields.push({key,kind:first.kind,value:first.value,text:reads.every(read=>read.text===first.text)?first.text:null});
  }
  return fields;
}
// One write to every control of the batch, merged into the single transaction the editor undoes as one
// step. A property write never takes a control out of the file, so each of them only moves by the text
// the earlier controls grew or shrank by: the batch reports those offsets in the order the selection
// handed them over, which is what lets the host point at the same controls again.
export function sharedEdit(source,starts,key,value){
  const edits=starts.map(start=>({start,...propertyEdit(source,start,key,value)}));
  const change=editElements(source,starts,start=>edits.find(edit=>edit.start===start));
  if(!change)return null;
  const shift=start=>edits.filter(edit=>edit.from<start).reduce((sum,edit)=>sum+edit.insert.length-(edit.to-edit.from),0);
  return {...change,starts:starts.map(start=>start+shift(start))};
}
export function createBulkEditor({container,commit,onError=()=>{}}){
  let current=null;
  function apply(field,text,input){
    try{
      const change=sharedEdit(current.source,current.starts,field.key,literalValue(field,text));
      input.setCustomValidity('');
      // The row also carries the control the panel above belongs to: the merge knows how far each of
      // the edited controls moved, so the host can keep the whole batch selected instead of losing it.
      if(change)commit({file:current.path,source:current.source,...change,primary:change.starts[Math.max(0,current.starts.indexOf(current.primary))]});
    }catch(error){
      input.setCustomValidity(error.message);
      input.reportValidity();
      onError(error.message);
    }
  }
  // The rows are rebuilt from the same snapshot the panel above them reads, and they disappear with
  // the batch: a selection of one control is what that panel already edits.
  function render({path,source,starts,primary,editable}){
    container.querySelector('.bulk-edit')?.remove();
    current=null;
    if(!editable)return;
    const fields=sharedFields(source,starts);
    if(!fields.length)return;
    current={path,source,primary,starts:[...starts],fields};
    const section=document.createElement('section');section.className='bulk-edit';
    const heading=document.createElement('div');heading.className='inspector-label';heading.textContent='ПАКЕТНАЯ ПРАВКА';
    const note=document.createElement('small');note.className='bulk-note';
    note.textContent=`Значение записывается в разметку всех ${starts.length} выбранных контролов.`;
    section.append(heading,note);
    for(const field of fields){
      const label=document.createElement('label');label.className=`property ${field.kind}`;
      const name=document.createElement('span');name.textContent=field.key;
      label.append(name);
      const input=document.createElement('input');
      input.value=field.text??'';
      // A batch has no value of its own to show while the controls disagree, so the field starts empty
      // and says why; whatever is typed goes to every one of them.
      if(field.text===null)input.placeholder='значения различаются';
      input.setAttribute('aria-label',`${field.key} для ${starts.length} контролов`);
      // A batch row is not the panel's own field: the bridge finds those by `data-prop`.
      input.dataset.bulk=field.key;
      input.onchange=()=>apply(field,input.value,input);
      label.append(input);
      section.append(label);
    }
    container.append(section);
  }
  return {render};
}
