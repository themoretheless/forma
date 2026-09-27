// Undo and redo replace the whole file at once, so a canvas selection can only be carried across them
// by describing what each control *is* rather than where its text stood. Two descriptions survive an
// edit elsewhere in the tree: the design `key`, which is how the preview addresses a control already,
// and the slot the control occupies — its type plus the chain of child indexes from the page root,
// which holds however deeply inside a group the control sits.
export function selectionIdentities(root,starts){
 if(!root)return [];
 const found=new Map();
 const walk=(node,path)=>{
  found.set(node.start,{key:typeof node.props?.key==='string'?node.props.key:null,type:node.type,path:[...path]});
  (node.children??[]).forEach((child,index)=>walk(child,[...path,index]));
 };
 walk(root,[]);
 return [...new Set(starts)].map(start=>found.get(start)).filter(Boolean);
}
// A control with a key answers to it wherever it stands; one without a key has to be the same type in
// the same slot, so a look-alike that only happens to sit under a moved container is not mistaken for
// the node the designer had selected. Identities come back in the order they were given.
export function restoreSelection(root,identities){
 const nodes=[];
 let dropped=0;
 if(!root)return {nodes,starts:[],dropped:identities.length};
 const byKey=new Map(),byPath=new Map();
 const walk=(node,path)=>{
  const key=node.props?.key;
  if(typeof key==='string'&&!byKey.has(key))byKey.set(key,node);
  byPath.set(path.join('/'),node);
  (node.children??[]).forEach((child,index)=>walk(child,[...path,index]));
 };
 walk(root,[]);
 for(const step of identities){
  const node=step.key!==null?byKey.get(step.key):byPath.get(step.path.join('/'));
  if(node&&(step.key!==null||node.type===step.type)){if(!nodes.includes(node))nodes.push(node);}
  else dropped++;
 }
 return {nodes,starts:nodes.map(node=>node.start),dropped};
}
