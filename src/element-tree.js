// Source locations are UTF-16 offsets, matching the editor (not Rust byte offsets).
// Attach before linking so inherited nodes keep their defining file.
export function attachSources(value,file){
 if(!value||typeof value!=='object')return value;
 for(const child of Object.values(value))attachSources(child,file);
 if(value.type&&Number.isInteger(value.start)&&Number.isInteger(value.end)){
  value.source={file,from:value.start,to:value.end};
  value.propertySources=Object.fromEntries(Object.entries(value.propertyRanges??{}).map(([key,range])=>[key,{file,...range}]));
 }
 return value;
}

function freeze(value){
 if(value&&typeof value==='object'&&!Object.isFrozen(value)){
  for(const child of Object.values(value))freeze(child);
  Object.freeze(value);
 }
 return value;
}

// IDs are local to this immutable compilation snapshot. A user key is metadata,
// never an array index or a globally unique ID. Reconciliation is a separate job.
export function createElementTree(roots){
 const nodes=[];
 function add(element,parent){
  const id=nodes.length;
  const node={id,parent,type:element.type,key:element.props?.key??null,
   source:element.source??null,
   propertySources:element.propertySources??{},
   props:element.props??{},events:element.events??{},
   bindings:element.bindings??{},children:[]};
  nodes.push(node);
  node.children=(element.children??[]).map(child=>add(child,id));
  return id;
 }
 const rootIds=roots.map(root=>add(root,null));
 // Project first, then detach once. Cloning each field separately repeats the
 // structured-clone setup five times per node and copies shared origins again.
 return freeze(structuredClone({roots:rootIds,nodes}));
}
