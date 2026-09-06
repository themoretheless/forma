import {test} from 'node:test';
import assert from 'node:assert/strict';
import {parse} from '../src/language.js';
import {gridStyles} from '../src/grid.js';
test('Frame accepts mixed absolute and relative tracks',()=>{
 const n=parse('component Demo { Frame { columns: [200, 25%, *, 2*]; rows: [48px, auto, 1*]; } }').nodes[0];
 assert.deepEqual(gridStyles(n,{}),{display:'grid',alignContent:'start',gridTemplateColumns:'200px 25% 1fr 2fr',gridTemplateRows:'48px auto 1fr'});
});
test('one-based placement and spanning',()=>{
 const n=parse("component Demo { Text { column: 2; row: 1; column.span: 3; text: 'Hi'; } }").nodes[0];
 assert.deepEqual(gridStyles(n,{}),{gridColumnStart:'2',gridColumnEnd:'span 3',gridRowStart:'1'});
 assert.throws(()=>gridStyles({type:'Frame',props:{columns:-1}},{}));
 assert.throws(()=>gridStyles({type:'Text',props:{row:0}},{}));
});
test('unpositioned children fill the parent grid on unspecified axes',()=>{
 const parent={type:'Frame',props:{columns:[100,100],rows:[48,48]}};
 assert.deepEqual(gridStyles({type:'Text',props:{}},{},parent),{gridColumnStart:'1',gridColumnEnd:'-1',gridRowStart:'1',gridRowEnd:'-1'});
 assert.deepEqual(gridStyles({type:'Text',props:{row:2}},{},parent),{gridColumnStart:'1',gridColumnEnd:'-1',gridRowStart:'2'});
 assert.deepEqual(gridStyles({type:'Text',props:{}},{}),{});
});
test('cell shorthand uses row then column and supports spans',()=>{
 const n=parse("component Demo { Text { cell: 2 3; column.span: 2; text: 'Cell'; } }").nodes[0];
 assert.deepEqual(gridStyles(n,{}),{gridColumnStart:'3',gridColumnEnd:'span 2',gridRowStart:'2'});
 const parent={type:'Frame',props:{columns:[100,100],rows:[50,50]}};
 assert.deepEqual(gridStyles({type:'Text',props:{cell:[2,2]}},{},parent),{gridColumnStart:'2',gridRowStart:'2'});
 for(const props of [{cell:[2]},{cell:[0,2]},{cell:[1,1.5]},{cell:[1,2],row:1},{cell:[1,2],column:2}])assert.throws(()=>gridStyles({type:'Text',props},{}),/cell/);
});
