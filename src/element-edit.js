import {parse} from './language.js';
export function locateElement(source,start){
 let found;const walk=(nodes,parent)=>{for(const node of nodes){if(node.start===start)found={node,parent};walk(node.children,node);}};walk(parse(source).nodes,null);
 if(!found)throw Error('Выберите элемент заново');return found;
}
const literal=v=>typeof v==='string'?JSON.stringify(v):Array.isArray(v)?'['+v.join(', ')+']':String(v);
function patch(source,node,props){
 const changes=[];let added='';for(const [key,value] of Object.entries(props)){
  const range=node.propertyRanges[key];if(range)changes.push({...range,insert:literal(value)});else added+=` ${key}: ${literal(value)};`;
 }
 if(added){const at=source.indexOf('{',node.start)+1;changes.push({from:at,to:at,insert:added});}
 for(const c of changes.sort((a,b)=>b.from-a.from))source=source.slice(0,c.from)+c.insert+source.slice(c.to);
 return source;
}
export function moveElement(source,start,dx,dy,cell,origin=[0,0]){
 const {node,parent}=locateElement(source,start);if(!parent||node.type==='Scroll')throw Error('Перемещайте дочерние контролы');
 let props;
 if(parent.props.columns!==undefined||parent.props.rows!==undefined){
  if(!cell)throw Error('Для Grid выберите целевую ячейку');
  props=node.props.cell?{cell:[cell.row,cell.column]}:{row:cell.row,column:cell.column};
 }else{
  for(const axis of ['x','y'])if(node.props[axis]!==undefined&&typeof node.props[axis]!=='number')throw Error('Координата задана выражением: измените её в коде');
  props={x:Math.max(0,Math.round(((node.props.x??origin[0])+dx)*10)/10),y:Math.max(0,Math.round(((node.props.y??origin[1])+dy)*10)/10)};
 }
 const next=patch(source,node,props);parse(next);return {from:node.start,to:node.end,insert:next.slice(node.start,node.end+next.length-source.length),start:node.start};
}
export function removeElement(source,start){const {node,parent}=locateElement(source,start);if(!parent||node.type==='Scroll')throw Error('Корневой контейнер нельзя удалить');return {from:node.start,to:node.end,insert:'',start:parent.start};}
export function copyElement(source,start){const {node,parent}=locateElement(source,start);if(!parent||node.type==='Scroll')throw Error('Выберите дочерний контрол');return source.slice(node.start,node.end);}
export function insertElement(source,start,text,multiple=false){
 const parsed=parse('component Clipboard { Frame { '+text+' } }').nodes[0].children;
 if(!parsed.length||(!multiple&&parsed.length!==1)||parsed.some(n=>['Frame','Scroll'].includes(n.type)))throw Error('Вставьте один контрол Forma');
 const {node,parent}=locateElement(source,start);const container=['Frame','Scroll'].includes(node.type)?node:parent;
 if(!container)throw Error('Выберите контейнер');
 const keys=new Set();const walk=nodes=>{for(const n of nodes){if(typeof n.props.key==='string')keys.add(n.props.key);walk(n.children);}};walk(parse(source).nodes);
 // Rename copied keys without changing labels, bindings or comments.
 const wrapper='component Clipboard { Frame { ';let wrapped=wrapper+text+' } }';
 const all=[];const collect=ns=>{for(const n of ns){all.push(n);collect(n.children);}};collect(parsed);
 for(const n of all.reverse()){if(typeof n.props.key!=='string')continue;let key=n.props.key;let i=2;while(keys.has(key))key=n.props.key+'_'+i++;keys.add(key);wrapped=patch(wrapped,n,{key});}
 text=wrapped.slice(wrapper.length,-4);
 const at=container===node?container.end-1:node.end;
 const insert='\n'+text+'\n';const next=source.slice(0,at)+insert+source.slice(at);parse(next);const inserted=parse(wrapper+text+' } }').nodes[0].children;return {from:at,to:at,insert,start:at+1,starts:inserted.map(n=>at+1+n.start-wrapper.length)};
}
export function gridCell(grid,x,y){
 const track=(sizes,value,gap)=>{let edge=0;for(let i=0;i<sizes.length;i++){edge+=sizes[i]+gap;if(value<edge)return i+1;}return sizes.length;};
 return {column:track(grid.columns,x-grid.bounds[0],grid.gap[1]),row:track(grid.rows,y-grid.bounds[1],grid.gap[0])};
}
export function reorderElement(source,start,direction){
 if(direction!==-1&&direction!==1)throw Error('Направление: выше или ниже');
 const {node,parent}=locateElement(source,start);
 if(!parent||node.type==='Scroll')throw Error('Выберите дочерний контрол');
 const index=parent.children.indexOf(node),other=parent.children[index+direction];
 if(!other)return null;
 const first=direction<0?other:node,last=direction<0?node:other;
 const gap=source.slice(first.end,last.start),a=source.slice(first.start,first.end),b=source.slice(last.start,last.end);
 const insert=b+gap+a;
 const next=source.slice(0,first.start)+insert+source.slice(last.end);parse(next);
 return {from:first.start,to:last.end,insert,start:direction<0?first.start:first.start+b.length+gap.length};
}
// Merge independent source edits into one editor transaction, preserving selection offsets.
export function editElements(source,starts,operation){
 const ordered=[...new Set(starts)].sort((a,b)=>a-b);
 const changes=ordered.map(start=>operation(start)).filter(Boolean).sort((a,b)=>a.from-b.from);
 if(!changes.length)return null;
 for(let i=1;i<changes.length;i++)if(changes[i].from<changes[i-1].to)throw Error('Выбирайте элементы одного уровня');
 let next=source;for(const c of [...changes].reverse())next=next.slice(0,c.from)+c.insert+next.slice(c.to);
 const from=changes[0].from,to=changes.at(-1).to,shift=changes.reduce((sum,c)=>sum+c.insert.length-(c.to-c.from),0);
 const selected=changes.filter(c=>c.insert.length).map(c=>c.start+changes.filter(p=>p.to<=c.from).reduce((sum,p)=>sum+p.insert.length-(p.to-p.from),0));
 parse(next);return {from,to,insert:next.slice(from,to+shift),starts:selected,start:selected[0]??changes[0].start};
}
export function copyElements(source,starts){return [...new Set(starts)].sort((a,b)=>a-b).map(start=>copyElement(source,start)).join('\n');}
export function insertElements(source,start,text){return insertElement(source,start,text,true);}
