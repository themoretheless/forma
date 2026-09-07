import {expressionReferences} from './expressions.js';

// Keep every contributing source for expressions, rather than attributing a
// computed value to an arbitrary last dependency.
export function propertyOrigins(value,source,props={},sources={},traces={},seen=new Set()){
 const result=source?[{source,label:source.label??'Объявление'}]:[];
 for(const ref of expressionReferences(value)){
  if(ref.startsWith('props.')){
   const key=ref.slice(6);if(seen.has(key))continue;
   const next=new Set(seen);next.add(key);
   result.push(...(traces[key]??propertyOrigins(props[key],sources[key],props,sources,traces,next)));
  }else result.push({label:ref});
 }
 const unique=new Map();for(const origin of result)unique.set(JSON.stringify(origin),origin);
 return [...unique.values()];
}

export function displayProperty(value){
 if(value?.expr)return value.expr;
 if(typeof value==='string')return JSON.stringify(value);
 return JSON.stringify(value)??'—';
}
