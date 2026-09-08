const shorten=v=>String(v).replace(/\s+/g,' ').slice(0,70);
function valueLabel(v){if(Array.isArray(v))return '('+v.map(valueLabel).join(', ')+')';return v?.expr??shorten(v);}

// This is a source hierarchy, not an invented runtime expansion. A template's
// conditional branches are all shown and labelled; source offsets stay intact.
export function buildControlTree(document,path){
  function values(v,id,label){
    if(v?.type&&v.type!=='Brush')return [node(v,id,label)];
    if(v&&Object.hasOwn(v,'match'))return [{id,path,label:`${label} · match ${valueLabel(v.match)}`,node:null,children:v.branches.flatMap((b,i)=>values(b.value,`${id}/branch/${i}`,`${valueLabel(b.pattern)} ⇒`))}];
    if(v?.expr?.startsWith('base.'))return [{id,path,label:`${label} ${v.expr}`,node:null,children:[]}];
    return [];
  }
  function node(n,id,prefix=''){
    const detail=n.props.key?`#${n.props.key}`:typeof n.props.text==='string'?shorten(n.props.text):typeof n.props.source==='string'?shorten(n.props.source):'';
    return {id,path,label:prefix?`${prefix} ${n.type}`:n.type,detail,node:n,children:[
      ...(n.children??[]).map((c,i)=>node(c,`${id}/child/${i}`)),
      ...Object.entries(n.props).flatMap(([k,v])=>values(v,`${id}/prop/${k}`,`${k}:`)),
    ]};
  }
  return [{id:path,path,label:document.name+(document.base?` : ${document.base}`:''),node:null,children:[
    ...document.nodes.map((n,i)=>node(n,`${path}/root/${i}`)),
    ...Object.entries(document.slots??{}).flatMap(([k,v])=>values(v,`${path}/override/${k}`,`override ${k}:`)),
  ]}];
}

export function treeRows(roots,collapsed=new Set(),query=''){
  const needle=query.trim().toLocaleLowerCase();
  function filtered(n){if(`${n.label} ${n.detail??''}`.toLocaleLowerCase().includes(needle))return n;const children=n.children.map(filtered).filter(Boolean);return children.length?{...n,children}:null;}
  const out=[];
  function walk(items,level,parent){items.forEach((item,index)=>{const expanded=item.children.length>0&&(!collapsed.has(item.id)||!!needle);out.push({...item,level,parent,index:index+1,siblings:items.length,expanded});if(expanded)walk(item.children,level+1,item.id);});}
  walk(needle?roots.map(filtered).filter(Boolean):roots,1,null);return out;
}

export function createControlTree({explorer,toolbar,onSelect,onScopeChange,onOpenTemplate,storage=localStorage}){
  const panel=document.createElement('section');panel.className='control-tree-panel';panel.setAttribute('aria-label','Дерево контролов');
  panel.innerHTML=`<div class="control-tree-heading"><span>КОНТРОЛЫ</span><button data-action="expand" title="Развернуть дерево" aria-label="Развернуть дерево">⊞</button><button data-action="collapse" title="Свернуть дерево" aria-label="Свернуть дерево">⊟</button></div><div class="control-tree-scopes"><button data-scope="designer">Дизайнер</button><button data-scope="file">Файл</button></div><div class="control-tree-file"></div><input class="control-tree-search" type="search" placeholder="Найти контрол…" aria-label="Поиск в дереве контролов"><div class="control-tree-status" role="status"></div><div class="control-tree-items" role="tree" aria-label="Иерархия контролов"></div><button class="control-tree-template" hidden>Открыть шаблон компонента ↗</button>`;
  explorer.querySelector('.explorer-note').before(panel);
  const toggle=document.createElement('button');toggle.id='toggle-control-tree';toggle.textContent='☷ Дерево';toggle.setAttribute('aria-controls','control-tree-panel');panel.id='control-tree-panel';toolbar.prepend(toggle);
  const list=panel.querySelector('.control-tree-items'),search=panel.querySelector('input'),status=panel.querySelector('.control-tree-status'),template=panel.querySelector('.control-tree-template');
  let visible=true,scope='designer',roots=[],rows=[],selectedId=null,focusId=null,query='',disabled=false,templatePath=null;
  const collapsed=new Set();
  const elements=new Map(),parents=new Map(),sources=new Map();
  let currentDocument,currentPath;
  try{const saved=JSON.parse(storage.getItem('forma-control-tree'));visible=saved?.visible!==false;if(saved?.scope==='file')scope='file';}catch{}
  function save(){try{storage.setItem('forma-control-tree',JSON.stringify({visible,scope}));}catch{}}
  function updateView(){panel.hidden=!visible;explorer.classList.toggle('has-control-tree',visible);toggle.classList.toggle('chosen',visible);toggle.setAttribute('aria-expanded',String(visible));for(const b of panel.querySelectorAll('[data-scope]')){b.classList.toggle('chosen',b.dataset.scope===scope);b.setAttribute('aria-pressed',String(b.dataset.scope===scope));}save();}
  function rowElement(id){return elements.get(id);}
  function draw(restoreFocus=false){
    rows=treeRows(roots,collapsed,query);
    const keep=new Set(rows.map(n=>n.id));
    for(const [id,row] of elements)if(!keep.has(id)){row.remove();elements.delete(id);}
    list.querySelector('.control-tree-empty')?.remove();
    if(!rows.some(n=>n.id===focusId))focusId=rows.some(n=>n.id===selectedId)?selectedId:rows[0]?.id;
    for(const [index,n] of rows.entries()){
      let row=elements.get(n.id);
      if(!row){row=document.createElement('div');row.className='control-tree-row';
        for(const name of ['arrow','label','detail']){const span=document.createElement('span');span.className='control-tree-'+name;row.append(span);}
        elements.set(n.id,row);
      }row.dataset.controlId=n.id;row.setAttribute('role','treeitem');row.tabIndex=n.id===focusId?0:-1;
      row.setAttribute('aria-level',String(n.level));row.setAttribute('aria-posinset',String(n.index));row.setAttribute('aria-setsize',String(n.siblings));row.setAttribute('aria-selected',String(n.id===selectedId));row.setAttribute('aria-disabled',String(disabled));
      if(n.children.length)row.setAttribute('aria-expanded',String(n.expanded));else row.removeAttribute('aria-expanded');row.style.paddingLeft=(8+(n.level-1)*14)+'px';
      const [arrow,label,detail]=row.children;arrow.dataset.disclosure='';arrow.setAttribute('aria-hidden','true');arrow.textContent=n.children.length?(n.expanded?'⌄':'›'):'·';
      if(label.textContent!==n.label)label.textContent=n.label;
      if(detail.textContent!==(n.detail??''))detail.textContent=n.detail??'';
      row.title=[n.label,n.detail,n.path].filter(Boolean).join(' · ');const next=list.children[index];if(next!==row)list.insertBefore(row,next??null);
    }
    if(!rows.length){const empty=document.createElement('p');empty.className='control-tree-empty';empty.textContent=query?'Контролы не найдены':'Нет контролов';list.append(empty);}
    if(restoreFocus)rowElement(focusId)?.focus({preventScroll:true});
  }
  function focus(id){const previous=rowElement(focusId);if(previous)previous.tabIndex=-1;focusId=id;const next=rowElement(id);if(next){next.tabIndex=0;next.focus({preventScroll:true});next.scrollIntoView({block:'nearest'});}}
  function fold(id,collapse){const item=rows.find(n=>n.id===id);if(!item?.children.length)return;if(collapse)collapsed.add(id);else collapsed.delete(id);draw(true);}
  function choose(item){if(disabled)return;focus(item.id);if(item.node)onSelect(item);else fold(item.id,item.expanded);}
  list.addEventListener('click',e=>{const row=e.target.closest('[data-control-id]');const item=rows.find(n=>n.id===row?.dataset.controlId);if(!item||disabled)return;if(e.target.closest('[data-disclosure]')&&item.children.length){focusId=item.id;fold(item.id,item.expanded);}else choose(item);});
  list.addEventListener('keydown',e=>{
    const current=rows.find(n=>n.id===e.target.closest('[data-control-id]')?.dataset.controlId);if(!current||disabled)return;
    const index=rows.indexOf(current);
    if(!['ArrowDown','ArrowUp','ArrowLeft','ArrowRight','Home','End','Enter',' '].includes(e.key))return;e.preventDefault();
    if(e.key==='ArrowDown')focus(rows[Math.min(index+1,rows.length-1)].id);
    else if(e.key==='ArrowUp')focus(rows[Math.max(index-1,0)].id);
    else if(e.key==='Home')focus(rows[0].id);
    else if(e.key==='End')focus(rows.at(-1).id);
    else if(e.key==='ArrowRight'){if(current.children.length&&!current.expanded)fold(current.id,false);else if(current.expanded)focus(rows[index+1].id);}
    else if(e.key==='ArrowLeft'){if(current.expanded)fold(current.id,true);else if(current.parent)focus(current.parent);}
    else choose(current);
  });
  search.oninput=()=>{query=search.value;draw();};
  toggle.onclick=()=>setView({visible:!visible});
  for(const b of panel.querySelectorAll('[data-scope]'))b.onclick=()=>setView({scope:b.dataset.scope});
  panel.querySelector('[data-action=expand]').onclick=()=>{collapsed.clear();draw();};
  panel.querySelector('[data-action=collapse]').onclick=()=>{function walk(items){for(const n of items){if(n.children.length)collapsed.add(n.id);walk(n.children);}}walk(roots);draw();};
  template.onclick=()=>{if(templatePath)onOpenTemplate(templatePath);};
  function setView(next){if(next.visible!==undefined)visible=next.visible;if(next.scope!==undefined)scope=next.scope;updateView();onScopeChange();}
  function syncSelection(start,path,redraw=false){
    const previousSelected=selectedId,previousFocus=focusId;
    selectedId=null;templatePath=null;
    const item=sources.get(path)?.get(start);
    if(item){selectedId=item.id;templatePath=item.templatePath??null;for(let id=parents.get(item.id);id!=null;id=parents.get(id))if(collapsed.delete(id))redraw=true;}
    template.hidden=!templatePath;const focused=list.contains(document.activeElement);if(!focused)focusId=selectedId??focusId;
    if(redraw)draw(focused);
    else {
      if(!elements.has(focusId))focusId=elements.has(selectedId)?selectedId:rows[0]?.id;
      if(previousSelected!==selectedId){rowElement(previousSelected)?.setAttribute('aria-selected','false');rowElement(selectedId)?.setAttribute('aria-selected','true');}
      if(previousFocus!==focusId){const old=rowElement(previousFocus),next=rowElement(focusId);if(old)old.tabIndex=-1;if(next)next.tabIndex=0;}
    }
    if(selectedId)rowElement(selectedId)?.scrollIntoView({block:'nearest'});
  }
  updateView();
  return {
    get scope(){return scope;},setView,
    update({document:doc,path,error='',stale=false,selectedStart=null,selectedPath=null,files={}}){
      const changed=doc!==currentDocument||path!==currentPath,redraw=changed||disabled!==!!error;
      if(changed){roots=doc?buildControlTree(doc,path):[];currentDocument=doc;currentPath=path;parents.clear();sources.clear();}
      disabled=!!error;
      function annotate(items,parent=null){for(const n of items){if(changed){parents.set(n.id,parent);if(n.node){if(!sources.has(n.path))sources.set(n.path,new Map());sources.get(n.path).set(n.node.start,n);}}const target=n.node&&`components/${n.node.type}.ui`;n.templatePath=target&&files[target]?target:null;annotate(n.children,n.id);}}annotate(roots);
      panel.querySelector('.control-tree-file').textContent=path??'Нет UI';panel.querySelector('.control-tree-file').title=path??'';
      status.textContent=error?(stale?'Последнее корректное дерево · исправьте разметку':error):'';status.hidden=!error;
      syncSelection(selectedStart,selectedPath,redraw);
    },
    select:syncSelection,
    snapshot(){return {visible,scope,disabled,selectedId,rows:rows.map(n=>({id:n.id,path:n.path,label:n.label,detail:n.detail,level:n.level,expanded:n.expanded,start:n.node?.start??null,end:n.node?.end??null,selected:n.id===selectedId}))};},
    selectId(id){const n=rows.find(n=>n.id===id);if(!n||!n.node)throw Error('Контрол не найден в видимом дереве');if(disabled)throw Error('Исправьте разметку перед выбором');choose(n);},
    fold(id,collapsed){if(!rows.some(n=>n.id===id))throw Error('Узел дерева не найден');fold(id,collapsed);},
  };
}
