// Source locations are UTF-16 offsets, matching the editor (not Rust byte offsets).
// Attach before linking so inherited nodes keep their defining file.
export function attachSources(value,file){
 if(!value||typeof value!=='object')return value;
 for(const child of Object.values(value))attachSources(child,file);
 if(value.propertyRanges)value.propertySources=Object.fromEntries(Object.entries(value.propertyRanges).map(([key,range])=>[key,{file,...range}]));
 if(value.type&&Number.isInteger(value.start)&&Number.isInteger(value.end)){
  value.source={file,from:value.start,to:value.end};
  value.propertySources=Object.fromEntries(Object.entries(value.propertyRanges??{}).map(([key,range])=>[key,{file,...range}]));
 }
 return value;
}

// IDs are local to this immutable compilation snapshot. A user key is metadata,
// never an array index or a globally unique ID. Reconciliation is a separate job.
export function createElementTree(roots){
 const nodes=[];
 // Nodes of repeated instances share their definition values, so each distinct
 // value is copied once and reused. Copying and freezing in one pass keeps the
 // snapshot detached without the per-node structured-clone setup.
 const copies=new WeakMap();
 function copy(value){
  if(value===null||typeof value!=='object')return value;
  const known=copies.get(value);
  if(known!==undefined)return known;
  if(Array.isArray(value)){
   const out=[];copies.set(value,out);
   for(let index=0;index<value.length;index++)out[index]=copy(value[index]);
   return Object.freeze(out);
  }
  const out={};copies.set(value,out);
  for(const key of Object.keys(value))out[key]=copy(value[key]);
  return Object.freeze(out);
 }
 function add(element,parent){
  const id=nodes.length;
  const node={id,parent,type:element.type,key:element.props?.key??null,
   source:element.source?copy(element.source):null,
   propertySources:copy(element.propertySources??{}),
   props:copy(element.props??{}),events:copy(element.events??{}),
   bindings:copy(element.bindings??{}),children:[]};
  nodes.push(node);
  node.children=Object.freeze((element.children??[]).map(child=>add(child,id)));
  Object.freeze(node);
  return id;
 }
 const rootIds=roots.map(root=>add(root,null));
 return Object.freeze({roots:Object.freeze(rootIds),nodes:Object.freeze(nodes)});
}
