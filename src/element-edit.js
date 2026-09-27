import {parse} from './language.js';
import {containerTypes,intrinsicLength,weight} from './component-layout.js';
import {flowsCoordinates,hasTracks} from './positioning.js';
export function locateElement(source,start){
 let found;const walk=(nodes,parent)=>{for(const node of nodes){if(node.start===start)found={node,parent};walk(node.children,node);}};walk(parse(source).nodes,null);
 if(!found)throw Error('Выберите элемент заново');return found;
}
const literal=v=>typeof v==='string'?JSON.stringify(v):Array.isArray(v)?'['+v.join(', ')+']':String(v);
// The smallest document that holds a bare control, so a fragment of markup can be parsed, patched
// and measured the same way whether it came from the clipboard or from another container.
const WRAPPER='component Clipboard { Frame { ';
// The node types that own a child list in markup. Scroll and Modal are containers although they are not
// one of the layout containers, and a container is the only thing that can take a new child.
export const holdsChildren=node=>containerTypes.has(node.type)||['Scroll','Modal'].includes(node.type);
function patch(source,node,props){
 const changes=[];let added='';for(const [key,value] of Object.entries(props)){
  const range=node.propertyRanges[key];if(range)changes.push({...range,insert:literal(value)});else added+=` ${key}: ${literal(value)};`;
 }
 if(added){const at=source.indexOf('{',node.start)+1;changes.push({from:at,to:at,insert:added});}
 for(const c of changes.sort((a,b)=>b.from-a.from))source=source.slice(0,c.from)+c.insert+source.slice(c.to);
 return source;
}
// A move writes a coordinate, so it is possible only where the container reads one. A container
// with tracks places its children in cells instead, and a bare Grid or a Stack gives a written `x`
// nothing to move: the layout puts those children where its own rules say.
export function moveBlock(node,parent,cell){
 if(!parent||['Scroll','Modal'].includes(node.type))return 'Перемещайте дочерние контролы';
 if(hasTracks(parent))return cell?null:'Для Grid выберите целевую ячейку';
 if(!flowsCoordinates(parent))return `В ${parent.type} положение задаёт раскладка: перенос недоступен`;
 return null;
}
export function moveElement(source,start,dx,dy,cell,origin=[0,0]){
 const {node,parent}=locateElement(source,start);
 const block=moveBlock(node,parent,cell);if(block)throw Error(block);
 let props;
 if(hasTracks(parent)){
  props=node.props.cell?{cell:[cell.row,cell.column]}:{row:cell.row,column:cell.column};
 }else{
  for(const axis of ['x','y'])if(node.props[axis]!==undefined&&typeof node.props[axis]!=='number')throw Error('Координата задана выражением: измените её в коде');
  props={x:Math.max(0,Math.round(((node.props.x??origin[0])+dx)*10)/10),y:Math.max(0,Math.round(((node.props.y??origin[1])+dy)*10)/10)};
 }
 const next=patch(source,node,props);parse(next);return {from:node.start,to:node.end,insert:next.slice(node.start,node.end+next.length-source.length),start:node.start};
}
// A size is editable only where the layout takes it from the control itself: an expression or
// a flex weight is code the designer wrote on purpose, and a Grid child is sized by its tracks.
export function resizeBlock(node,parent){
 if(!parent||['Scroll','Modal'].includes(node.type))return 'Изменяйте размер дочернего контрола';
 if(parent.props.columns!==undefined||parent.props.rows!==undefined)return 'В Grid размер задают треки: перетащите границу сетки';
 for(const key of ['width','height']){const value=node.props[key];if(intrinsicLength(value))continue;
  if(weight(value))return `Размер задан весом: измените ${key} в коде`;
  if(value?.expr!==undefined)return `Размер задан выражением: измените ${key} в коде`;}
 return null;
}
// `edges` carries how far each side moved in scene units and `bounds` is the rendered box the
// designer grabbed, so a control that sized itself gets its real size written down instead of a
// guess. The opposite edge anchors the drag, which is why a leading edge moves the coordinate too.
export function resizeElement(source,start,edges,bounds,origin=[0,0]){
 const {node,parent}=locateElement(source,start);
 const block=resizeBlock(node,parent);if(block)throw Error(block);
 const number=value=>typeof value==='number'?value:undefined,rounded=value=>Math.round(value*10)/10;
 const props={};
 for(const [key,min,max,size,leading,trailing,coordinate,at] of [['width','minWidth','maxWidth',2,'left','right','x',0],['height','minHeight','maxHeight',3,'top','bottom','y',1]]){
  const delta=edges[trailing]-edges[leading];
  if(delta)props[key]=Math.min(number(node.props[max])??Infinity,Math.max(1,number(node.props[min])??1,rounded(bounds[size]+delta)));
  if(edges[leading])props[coordinate]=Math.max(0,rounded((node.props[coordinate]??origin[at])+edges[leading]));
 }
 const next=patch(source,node,props);parse(next);
 return {from:node.start,to:node.end,insert:next.slice(node.start,node.end+next.length-source.length),start:node.start};
}
export function removeElement(source,start){const {node,parent}=locateElement(source,start);if(!parent||['Scroll','Modal'].includes(node.type))throw Error('Корневой контейнер нельзя удалить');return {from:node.start,to:node.end,insert:'',start:parent.start};}
export function copyElement(source,start){const {node,parent}=locateElement(source,start);if(!parent||['Scroll','Modal'].includes(node.type))throw Error('Выберите дочерний контрол');return source.slice(node.start,node.end);}
export function insertElement(source,start,text,multiple=false,side='auto'){
 const parsed=parse(WRAPPER+text+' } }').nodes[0].children;
 if(!parsed.length||(!multiple&&parsed.length!==1)||parsed.some(n=>['Frame','Scroll','Modal'].includes(n.type)))throw Error('Вставьте один контрол Forma');
 const {node,parent}=locateElement(source,start);
 if(side==='inside'&&!holdsChildren(node))throw Error('Внутрь можно добавить только контейнер');
 if(side==='after'&&!parent)throw Error('У корневого контейнера нет соседа');
 // `auto` stays the paste rule, where only a real parent can take the node: duplicating a Row
 // must not put the copy inside it. The palette states the intent instead, so adding into a
 // Row never turns into adding next to the Row.
 const container=side==='after'?null:side==='inside'?node:['Frame','Scroll','Modal'].includes(node.type)?node:parent;
 if(!container&&side!=='after')throw Error('Выберите контейнер');
 const keys=new Set();const walk=nodes=>{for(const n of nodes){if(typeof n.props.key==='string')keys.add(n.props.key);walk(n.children);}};walk(parse(source).nodes);
 // Rename copied keys without changing labels, bindings or comments.
 let wrapped=WRAPPER+text+' } }';
 const all=[];const collect=ns=>{for(const n of ns){all.push(n);collect(n.children);}};collect(parsed);
 for(const n of all.reverse()){if(typeof n.props.key!=='string')continue;let key=n.props.key;let i=2;while(keys.has(key))key=n.props.key+'_'+i++;keys.add(key);wrapped=patch(wrapped,n,{key});}
 return spliceInto(source,node,parent,container,wrapped.slice(WRAPPER.length,-4));
}
// Splice a finished node text into the block the caller picked: `container` is the node that gains
// the child, and a null one means the marker node's own next sibling. The text arrives complete, so
// this only decides where it goes and how it is indented — which is also all a move between two
// containers needs once its own text has been cut out of the block it left.
function spliceInto(source,node,parent,container,text){
 const at=container===node?container.end-1:node.end;
 // A new node takes the indentation of the block it joins instead of landing at column 0.
 // A copy keeps the column its own line had, so its closing brace shows what to strip.
 const column=index=>source.slice(source.lastIndexOf('\n',index-1)+1,index).match(/^ */)[0];
 const siblings=container===node?node.children:parent?.children??[];
 const indent=siblings.length?column(siblings.at(-1).start):column((container??node).start)+'    ';
 const lines=text.split('\n'),base=lines.length>1?lines.at(-1).match(/^ */)[0]:'';
 const body=lines.map((line,index)=>line.trim()?(index?indent+line.slice(base.length):indent+line):'').join('\n');
 const pad=container===node?column(at):'';
 let from=at,insert,put;
 if(container!==node){insert='\n'+body+(/\S/.test(source[at])?'\n'+indent:'');}
 else if(pad===source.slice(source.lastIndexOf('\n',at-1)+1,at)){
  // The closing brace has a line of its own, so the new node takes the line above it.
  from=at-pad.length;insert=body+'\n'+pad;
 }
 else if(lines.length>1){insert='\n'+body+'\n'+column(node.start);}
 // A container written on one line keeps that shape for a control written on one line.
 else{const head=/^\s/.test(source[at-1])?'':' ';insert=head+body.trim()+' ';}
 put=from+insert.match(/^\s*/)[0].length;
 const next=source.slice(0,from)+insert+source.slice(at);parse(next);
 const inserted=parse(WRAPPER+text+' } }').nodes[0].children;
 return {from,to:at,insert,start:put,starts:inserted.map(n=>put+n.start-WRAPPER.length)};
}
export function gridCell(grid,x,y){
 const track=(sizes,value,gap)=>{let edge=0;for(let i=0;i<sizes.length;i++){edge+=sizes[i]+gap;if(value<edge)return i+1;}return sizes.length;};
 return {column:track(grid.columns,x-grid.bounds[0],grid.gap[1]),row:track(grid.rows,y-grid.bounds[1],grid.gap[0])};
}
export function reorderElement(source,start,direction){
 if(direction!==-1&&direction!==1)throw Error('Направление: выше или ниже');
 const {node,parent}=locateElement(source,start);
 if(!parent||['Scroll','Modal'].includes(node.type))throw Error('Выберите дочерний контрол');
 const index=parent.children.indexOf(node),other=parent.children[index+direction];
 if(!other)return null;
 const first=direction<0?other:node,last=direction<0?node:other;
 const gap=source.slice(first.end,last.start),a=source.slice(first.start,first.end),b=source.slice(last.start,last.end);
 const insert=b+gap+a;
 const next=source.slice(0,first.start)+insert+source.slice(last.end);parse(next);
 return {from:first.start,to:last.end,insert,start:direction<0?first.start:first.start+b.length+gap.length};
}
// A drag across the tree can land several slots away, so this moves a node to an arbitrary
// position among its own siblings instead of trading places with a neighbour. Every sibling keeps
// its own text and takes the whitespace of the slot it lands in, which is what an ordering change
// must do: a control's inner lines are indented for its parent, and all siblings share that level.
export function moveAmongSiblings(source,start,targetStart,side){
 if(side!=='before'&&side!=='after')throw Error('Опустите контрол до или после соседа');
 const {node,parent}=locateElement(source,start);
 if(!parent)throw Error('Перемещайте дочерний контрол');
 // One parse for both ends, so a target from another parent simply is not in this list.
 const siblings=parent.children,from=siblings.indexOf(node),to=siblings.findIndex(n=>n.start===targetStart);
 if(to<0)throw Error('Переместите контрол среди его соседей');
 if(from===to)return null;
 const texts=siblings.map(n=>source.slice(n.start,n.end));
 const gaps=siblings.slice(1).map((n,i)=>source.slice(siblings[i].end,n.start));
 const order=siblings.map((_,index)=>index).filter(index=>index!==from);
 order.splice(order.indexOf(to)+(side==='after'?1:0),0,from);
 const block=order.map((index,slot)=>texts[index]+(gaps[slot]??'')).join(''),first=siblings[0].start,last=siblings.at(-1).end;
 if(block===source.slice(first,last))return null;
 const next=source.slice(0,first)+block+source.slice(last);parse(next);
 // The moved block starts where its own text begins inside the rewritten sibling run.
 let offset=0;for(let slot=0;slot<order.indexOf(from);slot++)offset+=texts[order[slot]].length+(gaps[slot]??'').length;
 return {from:first,to:last,insert:block,start:first+offset};
}
// The point a container reads its child by, written into the text that is about to join it. A
// coordinate the designer bound to an expression stays theirs: a drop must not overwrite code with
// the number the canvas happened to measure.
function rewriteCoords(text,coords){
 const boxed=WRAPPER+text+' } }',node=parse(boxed).nodes[0].children[0],props={};
 for(const [key,axis] of [['x',0],['y',1]]){const value=node.props[key];
  if(value===undefined||typeof value==='number')props[key]=Math.max(0,Math.round(coords[axis]*10)/10);}
 return patch(boxed,node,props).slice(WRAPPER.length,-4);
}
// A control that leaves one container for another travels as its own text: cut out of the sibling
// run it stood in, then spliced into the block it joins, where it takes that block's indentation.
// `coords` is the point the drop aimed at, in the receiving container's own space; a container that
// places its children itself gets none, because it reads no coordinate.
export function moveIntoContainer(source,start,targetStart,coords=null){
 const {node,parent}=locateElement(source,start);
 if(!parent||['Scroll','Modal'].includes(node.type))throw Error('Перемещайте дочерний контрол');
 const target=locateElement(source,targetStart).node;
 if(!holdsChildren(target))throw Error('Внутрь можно добавить только контейнер');
 if(start===targetStart)throw Error('Контрол не может принять самого себя');
 for(const stack=[...node.children??[]];stack.length;){const child=stack.pop();if(child.start===targetStart)throw Error('Нельзя перенести контрол в его собственный потомок');stack.push(...(child.children??[]));}
 // The cut takes the whitespace that held the node off its block too, so the run it leaves has no
 // gap where the control used to stand — and it stops at the parent's own brace.
 const moved=coords?rewriteCoords(source.slice(node.start,node.end),coords):source.slice(node.start,node.end);
 let cut=node.start;const open=source.indexOf('{',parent.start)+1;
 while(cut>open&&/\s/.test(source[cut-1]))cut--;
 const before=source.slice(0,cut)+source.slice(node.end),delta=node.end-cut;
 const at=targetStart>node.end?targetStart-delta:targetStart;
 const holder=locateElement(before,at);
 const change=spliceInto(before,holder.node,holder.parent,holder.node,moved);
 const next=before.slice(0,change.from)+change.insert+before.slice(change.to);
 if(next===source)return null;
 parse(next);
 // Both edits are reported as one transaction: the smallest span whose replacement turns the source
 // into the result, which is what keeps the editor's own selection offsets meaningful.
 let from=0;while(from<next.length&&source[from]===next[from])from++;
 let trim=0;while(trim<Math.min(source.length,next.length)-from&&source[source.length-1-trim]===next[next.length-1-trim])trim++;
 return {from,to:source.length-trim,insert:next.slice(from,next.length-trim),start:change.start,starts:[change.start]};
}
// A group is a container of its own, so a designer can move, align and resize a set of controls as one
// thing. Grouping freezes what the canvas shows: the new `Frame` takes the box the members cover
// together, told in the corner of the container they stand in, and each member keeps the point it was
// drawn at by carrying it from the group's own corner. Both boxes come from the scene because a member
// the *layout* placed has no coordinate in the markup to inherit — a group that wrote none would
// restack its content into the new panel's own flow.
export function groupElements(source,starts,box=null,coords=null){
 const ordered=[...new Set(starts)].sort((a,b)=>a-b);
 if(ordered.length<2)throw Error('Выберите хотя бы два контрола');
 if(!box||!coords)throw Error('Группировка берёт положение с холста: выберите контролы на нём');
 const picked=ordered.map(start=>locateElement(source,start));
 const [{parent}]=picked;
 if(!parent)throw Error('Группируйте дочерние контролы');
 // Each call parses the document again, so it is the parent's place in the text that says whether the
 // selection is one sibling run rather than a look-alike container elsewhere on the page.
 if(picked.some(p=>p.parent?.start!==parent.start))throw Error('Группируйте контролы одного контейнера');
 if(picked.some(p=>['Scroll','Modal'].includes(p.node.type)))throw Error('Скроллящийся лист не группуют');
 if(hasTracks(parent))throw Error('В Grid группировка недоступна: положение задают ячейки');
 if(!flowsCoordinates(parent))throw Error(`В ${parent.type} положение задаёт раскладка: группировка недоступна`);
 for(const {node} of picked){
  if(!coords.has(node.start))throw Error('Группируйте контролы, которые рисует холст');
  for(const axis of ['x','y']){const value=node.props[axis];
   if(value!==undefined&&typeof value!=='number')throw Error('Координата задана выражением: соберите группу в коде');}
 }
 const first=picked[0].node;
 const column=index=>source.slice(source.lastIndexOf('\n',index-1)+1,index).match(/^ */)[0];
 const pad=column(first.start),inner=pad+'    ';
 const rounded=value=>Math.max(0,Math.round(value*10)/10);
 // A member takes the point the canvas drew it at, counted from the group's own corner; an axis the
 // designer bound to an expression is refused above, so none of their code is overwritten here.
 const children=picked.map(({node})=>rewriteCoords(source.slice(node.start,node.end),
   coords.get(node.start).map((value,axis)=>value-box[axis])))
  // Every member is indented one level deeper than it stood, which is the shape the designer would
  // have written by hand for the same panel.
  .map(text=>text.split('\n').map((line,index)=>index&&line?`    ${line}`:line).join('\n'));
 const head=[`x: ${rounded(box[0])}; y: ${rounded(box[1])};`];
 // A box the scene could not measure is left out rather than written as a collapsed panel.
 if(rounded(box[2])>=1&&rounded(box[3])>=1)head.push(`width: ${rounded(box[2])}; height: ${rounded(box[3])};`);
 const text=['Frame {',...head.map(prop=>inner+prop),inner+children.join('\n'+inner),pad+'}'].join('\n');
 // The first member's own span becomes the container and the rest leave the block they stood in,
 // taking the whitespace that held them off it, which is what keeps the siblings' lines intact.
 const change=editElements(source,ordered,start=>{
  if(start===first.start)return {from:first.start,to:first.end,insert:text,start:first.start};
  const {node}=locateElement(source,start);
  let cut=node.start;const open=source.indexOf('{',parent.start)+1;
  while(cut>open&&/\s/.test(source[cut-1]))cut--;
  return {from:cut,to:node.end,insert:'',start:cut};
 });
 if(!change)return null;
 // The new container is what the designer now holds: one selection, so the panels and the undo
 // identity both describe the group rather than the members it swallowed.
 return {...change,start:first.start,starts:[first.start]};
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
