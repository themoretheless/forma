import {expressionReferences} from './expressions.js';

// Keep every contributing source for expressions, rather than attributing a
// computed value to an arbitrary last dependency.
export function propertyOrigins(value,source,props={},sources={},traces={},seen){
 const result=source?[{source,label:source.label??'Объявление'}]:[];
 if(value===null||typeof value!=='object')return result;
 for(const ref of expressionReferences(value)){
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
