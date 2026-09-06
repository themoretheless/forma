let values={};
export function setDesignData(next){values=next;}
export function designReference(value){const ref=value?.expr??value;return typeof ref==='string'&&ref.startsWith('design.')?ref:null;}
export function readDesignData(ref){if(!Object.hasOwn(values,ref))throw Error(`Нет дизайн-данных ${ref}: нажмите «↻ Данные дизайна»`);return values[ref];}
export function designReferences(compiled){const refs=new Set();for(const props of Object.values(compiled.overrides))for(const value of Object.values(props)){const ref=designReference(value);if(ref)refs.add(ref);}return [...refs];}
