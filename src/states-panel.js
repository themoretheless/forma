// A design file holds the base overrides plus the named states that patch them; this panel is the
// authoring UI for that list. It shows one file at a time and never splices text itself: every
// action hands the host a state name, and the host recomputes the edit against the source the
// editor holds at that moment, so a row left behind by an earlier change fails with a message
// instead of cutting at offsets that no longer describe the file.
const plural=(n,one,few,many)=>`${n} ${n%10===1&&n%100!==11?one:n%10>=2&&n%10<=4&&(n%100<10||n%100>=20)?few:many}`;
export function createStatesPanel({explorer,onActivate,onCreate,onRename,onDelete}){
  const panel=document.createElement('section');panel.className='states-panel';panel.setAttribute('aria-label','Состояния дизайна');
  panel.innerHTML='<div class="states-heading"><span>СОСТОЯНИЯ</span><button data-action="add" title="Добавить состояние" aria-label="Добавить состояние">+</button></div><div class="states-file"></div><div class="states-status" role="status" hidden></div><div class="states-items" role="list" aria-label="Состояния дизайн-файла"></div>';
  explorer.querySelector('.explorer-note').before(panel);
  const fileLine=panel.querySelector('.states-file'),status=panel.querySelector('.states-status'),items=panel.querySelector('.states-items'),add=panel.querySelector('[data-action=add]');
  // `active` names the state the preview switcher shows: null is the base block, undefined nothing.
  let path=null,states=[],baseCount=0,active,note='',editable=true,draft=null,focusDraft=false;
  function show(message){note=message??'';status.textContent=note;status.hidden=!note;}
  function act(action){try{action();}catch(error){show(error.message);}}
  function row(info){
    const item=document.createElement('div');item.className='states-item';item.setAttribute('role','listitem');
    const button=document.createElement('button');button.className='states-row'+(info.base?' base':'');
    button.textContent=info.label;
    if(info.base?active===null:active===info.name)button.setAttribute('aria-current','true');
    button.onclick=()=>act(()=>onActivate(info.base?null:info.name));
    const count=document.createElement('span');count.className='states-count';count.textContent=info.detail;
    item.append(button,count);
    if(!info.base&&editable){
      const rename=document.createElement('button');rename.className='states-rename';rename.textContent='✎';
      rename.title='Переименовать состояние';rename.onclick=()=>{draft={name:info.name};focusDraft=true;draw();};
      const remove=document.createElement('button');remove.className='states-delete';remove.textContent='×';
      remove.title='Удалить состояние вместе с его переопределениями';remove.onclick=()=>act(()=>onDelete(info.name));
      item.append(rename,remove);
    }
    return item;
  }
  function draftRow(name){
    const item=document.createElement('div');item.className='states-item states-draft';item.setAttribute('role','listitem');
    const input=document.createElement('input');input.className='states-input';input.value=name??'';
    input.placeholder='имя состояния';input.setAttribute('aria-label',name===undefined?'Имя нового состояния':'Новое имя состояния');
    const done=keep=>{
      const text=input.value;
      if(!keep){draft=null;draw();return;}
      try{if(name===undefined)onCreate(text);else onRename(name,text);draft=null;}
      // The name the file refused is still the one being typed, so the row keeps it on screen.
      catch(error){show(error.message);return;}
      // The host redraws from the written file once the edit compiles; this clears the input now.
      draw();
    };
    input.onkeydown=event=>{
      if(event.key==='Enter'){event.preventDefault();done(true);}
      else if(event.key==='Escape'){event.preventDefault();done(false);}
    };
    input.onchange=()=>done(true);
    item.append(input);
    return item;
  }
  function draw(){
    fileLine.textContent=path??'Дизайн-файл не выбран';fileLine.title=path??'';
    add.disabled=!path||!editable;
    items.replaceChildren();
    status.textContent=note;status.hidden=!note;
    if(!path){
      const none=document.createElement('p');none.className='states-empty';
      none.textContent='Откройте .design.ui, чтобы править его состояния';items.append(none);return;
    }
    const keys=n=>plural(n,'переопределённый ключ','переопределённых ключа','переопределённых ключей');
    // The input takes focus only once its row is in the list: a detached element cannot hold it.
    let pending=null;
    const draftItem=name=>{const item=draftRow(name);if(focusDraft)pending=item.children[0];return item;};
    items.append(row({base:true,name:null,label:'Базовые значения',detail:keys(baseCount)}));
    states.forEach(state=>items.append(draft&&draft.name===state.name?draftItem(state.name):row({name:state.name,label:state.name,detail:keys(state.count)})));
    if(draft&&draft.name===undefined)items.append(draftItem(undefined));
    if(pending){pending.focus();pending.select();}
    focusDraft=false;
  }
  add.onclick=()=>{if(!add.disabled){draft={name:undefined};focusDraft=true;draw();}};
  draw();
  return {
    update({path:nextPath=null,states:nextStates=[],baseCount:nextBase=0,active:nextActive,editable:nextEditable=true,error=''}){
      path=nextPath;states=nextStates;baseCount=nextBase;editable=nextEditable;active=nextActive;
      // A state that left the file while its name was being typed ends the draft: the text on
      // screen belongs to nothing now, and the host's own message says why the list changed.
      if(draft&&draft.name!==undefined&&!states.some(state=>state.name===draft.name))draft=null;
      show(error);draw();
    },
    snapshot(){
      return {path,editable,active:active??null,status:note,
        rows:[{name:null,base:true,count:baseCount,current:active===null},
          ...states.map((state,index)=>({name:state.name,base:false,count:state.count,current:active===state.name,index}))]};
    },
  };
}
