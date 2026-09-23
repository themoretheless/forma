import {expressionReferences} from './expressions.js';

const noOrigins=Object.freeze([]);
// A property with one contributing origin repeats for every instance of a
// definition, so its snapshot is shared: keyed by source for literals and by the
// value object for reference-free values.
const originsBySource=new WeakMap();
const originsByValue=new WeakMap();
function singleOrigin(source){
 const known=originsBySource.get(source);
 if(known!==undefined)return known;
 const result=Object.freeze([{source,label:source.label??'Объявление'}]);
 if(typeof source==='object'||typeof source==='function')originsBySource.set(source,result);
 return result;
}
function singleValueOrigin(value,source){
 const known=originsByValue.get(value);
 if(known!==undefined&&known[0]===source)return known[1];
 const result=Object.freeze([{source,label:source.label??'Объявление'}]);
 originsByValue.set(value,[source,result]);
 return result;
}
const soleOrigin=(value,source)=>source!==null&&typeof source==='object'?singleValueOrigin(value,source):singleOrigin(source);

// Keep every contributing source for expressions, rather than attributing a
// computed value to an arbitrary last dependency.
export function propertyOrigins(value,source,props={},sources={},traces={},seen){
 if(value===null||typeof value!=='object')return source?singleOrigin(source):noOrigins;
 const references=expressionReferences(value);
 if(references.length===0)return source?soleOrigin(value,source):noOrigins;
 const result=source?[{source,label:source.label??'Объявление'}]:[];
 for(const ref of references){
  if(ref.startsWith('props.')){
   const key=ref.slice(6);if(seen?.has(key))continue;
   if(traces[key]){result.push(...traces[key]);continue;}
   seen??=new Set();seen.add(key);
   try{result.push(...propertyOrigins(props[key],sources[key],props,sources,traces,seen));}
   finally{seen.delete(key);}
  }else result.push({label:ref});
 }
 if(result.length<2)return result;
 const unique=new Map();for(const origin of result)unique.set(JSON.stringify(origin),origin);
 return [...unique.values()];
}

export function displayProperty(value){
 if(value?.expr)return value.expr;
 if(typeof value==='string')return JSON.stringify(value);
 return JSON.stringify(value)??'—';
}
