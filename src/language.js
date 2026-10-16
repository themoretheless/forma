export {parse,validateDesign,designStatePatch} from '@themoretheless/tokenizer-forma/language';
import {resolve as resolveValue} from '@themoretheless/tokenizer-forma/language';
import {readDesignData} from './design-data.js';
export const resolve=(value,state)=>resolveValue(value,state,readDesignData);
