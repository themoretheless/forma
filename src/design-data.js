let values={};
// The runner answers design.NAME and design.method() only, so the pattern matches that shape
// and refuses dotted paths: './SearchWindow.design.ui' is a file name, not a token.
const designToken=/(?<![\w.])design\.[A-Za-z_]\w*(?:\(\))?(?![\w.(])/g;
export function setDesignData(next){values=next;}
export function designReference(value){const ref=value?.expr??value;return typeof ref==='string'&&ref.startsWith('design.')?ref:null;}
export function hasDesignData(ref){return Object.hasOwn(values,ref);}
export function readDesignData(ref){if(!Object.hasOwn(values,ref))throw Error(`Нет дизайн-данных ${ref}: нажмите «↻ Данные дизайна»`);return values[ref];}
export function designReferences(compiled){const refs=new Set();for(const props of Object.values(compiled.overrides))for(const value of Object.values(props)){const ref=designReference(value);if(ref)refs.add(ref);}return [...refs];}
// Rust computes a token's value and there is no catalogue without running it, so the
// list a designer can pick from is the set of tokens this project already references.
export function designReferencesInFiles(files){
  const refs=new Set();
  for(const [path,text]of Object.entries(files)){
    if(!path.endsWith('.ui')||typeof text!=='string')continue;
    for(const match of text.match(designToken)??[])refs.add(match);
  }
  return [...refs].sort();
}
