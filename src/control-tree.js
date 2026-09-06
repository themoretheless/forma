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
  function filtered(n){const children=n.children.map(filtered).filter(Boolean);if(!needle||`${n.label} ${n.detail??''}`.toLocaleLowerCase().includes(needle))return n;return children.length?{...n,children}:null;}
  const out=[];
  function walk(items,level,parent){items.forEach((item,index)=>{const expanded=item.children.length>0&&(!collapsed.has(item.id)||!!needle);out.push({...item,level,parent,index:index+1,siblings:items.length,expanded});if(expanded)walk(item.children,level+1,item.id);});}
  walk(roots.map(filtered).filter(Boolean),1,null);return out;
}

export function createControlTree({explorer,toolbar,onSelect,onScopeChange,onOpenTemplate,storage=localStorage}){
  const panel=document.createElement('section');panel.className='control-tree-panel';panel.setAttribute('aria-label','Дерево контролов');
  panel.innerHTML=`<div class="control-tree-heading"><span>КОНТРОЛЫ</span><button data-action="expand" title="Развернуть дерево" aria-label="Развернуть дерево">⊞</button><button data-action="collapse" title="Свернуть дерево" aria-label="Свернуть дерево">⊟</button></div><div class="control-tree-scopes"><button data-scope="designer">Дизайнер</button><button data-scope="file">Файл</button></div><div class="control-tree-file"></div><input class="control-tree-search" type="search" placeholder="Найти контрол…" aria-label="Поиск в дереве контролов"><div class="control-tree-status" role="status"></div><div class="control-tree-items" role="tree" aria-label="Иерархия контролов"></div><button class="control-tree-template" hidden>Открыть шаблон компонента ↗</button>`;
  explorer.querySelector('.explorer-note').before(panel);
  const toggle=document.createElement('button');toggle.id='toggle-control-tree';toggle.textContent='☷ Дерево';toggle.setAttribute('aria-controls','control-tree-panel');panel.id='control-tree-panel';toolbar.prepend(toggle);
  const list=panel.querySelector('.control-tree-items'),search=panel.querySelector('input'),status=panel.querySelector('.control-tree-status'),template=panel.querySelector('.control-tree-template');
  let visible=true,scope='designer',roots=[],rows=[],selectedId=null,focusId=null,query='',disabled=false,templatePath=null;
  const collapsed=new Set();
  try{const saved=JSON.parse(storage.getItem('forma-control-tree'));visible=saved?.visible!==false;if(saved?.scope==='file')scope='file';}catch{}
  function save(){try{storage.setItem('forma-control-tree',JSON.stringify({visible,scope}));}catch{}}
  function updateView(){panel.hidden=!visible;explorer.classList.toggle('has-control-tree',visible);toggle.classList.toggle('chosen',visible);toggle.setAttribute('aria-expanded',String(visible));for(const b of panel.querySelectorAll('[data-scope]')){b.classList.toggle('chosen',b.dataset.scope===scope);b.setAttribute('aria-pressed',String(b.dataset.scope===scope));}save();}
  function rowElement(id){return [...list.children].find(el=>el.dataset.controlId===id);}
  function draw(restoreFocus=false){
    rows=treeRows(roots,collapsed,query);list.replaceChildren();
    if(!rows.some(n=>n.id===focusId))focusId=rows.some(n=>n.id===selectedId)?selectedId:rows[0]?.id;
    for(const n of rows){
      const row=document.createElement('div');row.className='control-tree-row';row.dataset.controlId=n.id;row.setAttribute('role','treeitem');row.tabIndex=n.id===focusId?0:-1;
      row.setAttribute('aria-level',String(n.level));row.setAttribute('aria-posinset',String(n.index));row.setAttribute('aria-setsize',String(n.siblings));row.setAttribute('aria-selected',String(n.id===selectedId));row.setAttribute('aria-disabled',String(disabled));
      if(n.children.length)row.setAttribute('aria-expanded',String(n.expanded));row.style.paddingLeft=(8+(n.level-1)*14)+'px';
      const arrow=document.createElement('span');arrow.className='control-tree-arrow';arrow.dataset.disclosure='';arrow.setAttribute('aria-hidden','true');arrow.textContent=n.children.length?(n.expanded?'⌄':'›'):'·';
      const label=document.createElement('span');label.className='control-tree-label';label.textContent=n.label;
      const detail=document.createElement('span');detail.className='control-tree-detail';detail.textContent=n.detail??'';
      row.title=[n.label,n.detail,n.path].filter(Boolean).join(' · ');row.append(arrow,label,detail);list.append(row);
    }
    if(!rows.length){const empty=document.createElement('p');empty.className='control-tree-empty';empty.textContent=query?'Контролы не найдены':'Нет контролов';list.append(empty);}
    if(restoreFocus)rowElement(focusId)?.focus({preventScroll:true});
  }
  function focus(id){focusId=id;for(const el of list.children)el.tabIndex=el.dataset.controlId===id?0:-1;rowElement(id)?.focus({preventScroll:true});rowElement(id)?.scrollIntoView({block:'nearest'});}
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
  function syncSelection(start,path){
    selectedId=null;templatePath=null;
    function walk(items,parents=[]){for(const n of items){if(n.node?.start===start&&n.path===path){selectedId=n.id;for(const id of parents)collapsed.delete(id);if(n.templatePath)templatePath=n.templatePath;}walk(n.children,[...parents,n.id]);}}
    walk(roots);template.hidden=!templatePath;const focused=list.contains(document.activeElement);if(!focused)focusId=selectedId??focusId;draw(focused);
    if(selectedId)rowElement(selectedId)?.scrollIntoView({block:'nearest'});
  }
  updateView();
  return {
    get scope(){return scope;},setView,
    update({document:doc,path,error='',stale=false,selectedStart=null,selectedPath=null,files={}}){
      roots=doc?buildControlTree(doc,path):[];disabled=!!error;
      function annotate(items){for(const n of items){const target=n.node&&`components/${n.node.type}.ui`;if(target&&files[target])n.templatePath=target;annotate(n.children);}}annotate(roots);
      panel.querySelector('.control-tree-file').textContent=path??'Нет UI';panel.querySelector('.control-tree-file').title=path??'';
      status.textContent=error?(stale?'Последнее корректное дерево · исправьте разметку':error):'';status.hidden=!error;
      syncSelection(selectedStart,selectedPath);
    },
    select:syncSelection,
    snapshot(){return {visible,scope,disabled,selectedId,rows:rows.map(n=>({id:n.id,path:n.path,label:n.label,detail:n.detail,level:n.level,expanded:n.expanded,start:n.node?.start??null,end:n.node?.end??null,selected:n.id===selectedId}))};},
    selectId(id){const n=rows.find(n=>n.id===id);if(!n||!n.node)throw Error('Контрол не найден в видимом дереве');if(disabled)throw Error('Исправьте разметку перед выбором');choose(n);},
    fold(id,collapsed){if(!rows.some(n=>n.id===id))throw Error('Узел дерева не найден');fold(id,collapsed);},
  };
}
