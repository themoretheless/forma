import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parser} from '../src/forma-parser.js';

// The external string tokenizer: names every String token the parser accepts in a property value.
const strings=source=>{const found=[];parser.parse(source).iterate({enter:n=>{if(n.name==='String')found.push(source.slice(n.from,n.to));}});return found;};
const wrap=value=>`component A { Text { text: ${value}; } }`;

test('an interpolation may hold a string in the other quote style without ending the string',()=>{
 assert.deepEqual(strings(wrap(`'a\${"b"}c'`)),[`'a\${"b"}c'`]);
 assert.deepEqual(strings(wrap(`"x\${'}'}y"`)),[`"x\${'}'}y"`],'a brace inside a nested string does not close the interpolation');
});
test('an escaped quote does not end the string',()=>{
 assert.deepEqual(strings(wrap(`'it\\'s'`)),[`'it\\'s'`]);
});
test('an unterminated string or interpolation yields no string token',()=>{
 assert.deepEqual(strings(wrap(`'abc\${`)),[]);
 assert.deepEqual(strings(wrap(`'abc`)),[]);
});
