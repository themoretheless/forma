export {parse,validateDesign,designStatePatch} from '@themoretheless/tokenizer-forma/language';
import {parse,resolve as resolveValue} from '@themoretheless/tokenizer-forma/language';
import {readDesignData} from './design-data.js';
export const resolve=(value,state)=>resolveValue(value,state,readDesignData);
const recent=new Map();
export function parseShared(source){
  let doc=recent.get(source);
  if(doc){recent.delete(source);recent.set(source,doc);return doc;}
  doc=parse(source);recent.set(source,doc);
  if(recent.size>4)recent.delete(recent.keys().next().value);
  return doc;
}

