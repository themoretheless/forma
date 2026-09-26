import {linkComponentDefinitions} from './components.js';

// The designer adds controls to a page, so an entry has to be a type the page compiler takes
// as a top-level control. That is exactly what instantiation requires: the name links through
// `components/<Name>.ui` or a built-in fallback, and the linked definition is a single
// Rectangle. Bare primitives (Rectangle, Image) and internal parts rooted in a Text or a
// ContentPresenter (Icon, Label, Spinner) are written like controls but fail that rule, so
// they stay out of the palette. Containers are absent for the same reason: the node splicing
// primitive refuses them as inserted nodes, just as it refuses them on paste.
export function insertableControls(files){
  let definitions;
  try{definitions=linkComponentDefinitions(files).definitions;}catch{return [];}
  const items=[];
  for(const [name,definition]of Object.entries(definitions)){
    if(definition?.nodes?.length!==1||definition.nodes[0].type!=='Rectangle')continue;
    // A control whose own label default is empty would land on the canvas invisible.
    const props=definition.defaults?.text===''?["text: 'Текст';"]:[];
    items.push({type:name,markup:props.length?`${name} { ${props.join(' ')} }`:`${name} { }`});
  }
  return items.sort((a,b)=>a.type.localeCompare(b.type,'ru'));
}
