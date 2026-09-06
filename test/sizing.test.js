import {test} from 'node:test';
import assert from 'node:assert/strict';
import {sizeValue} from '../src/sizing.js';
import {trackList} from '../src/grid.js';
import {parse,resolve} from '../src/language.js';
test('dash is content sizing without conflicting with arrows or negative numbers',()=>{
 const n=parse("component Demo { Frame { columns: [-, *]; rows: [48, -]; width: -; margin: -8; clicked -> actions.search(); value <-> state.query; } }").nodes[0];
 assert.equal(trackList(n.props.columns,{}),'max-content 1fr');
 assert.equal(trackList(n.props.rows,{}),'48px max-content');
 assert.equal(sizeValue(resolve(n.props.width,{})),'fit-content');
 assert.equal(n.props.margin,-8);
 assert.equal(n.events.clicked,'actions.search');
 assert.equal(n.bindings.value,'state.query');
});
test('content sizing for elements and tracks',()=>{
 assert.equal(sizeValue('content'),'fit-content');
 assert.equal(sizeValue(240),'240px');
 assert.equal(sizeValue('100%'),'100%');
 assert.equal(trackList([{expr:'content'},{expr:'*'}],{}),'max-content 1fr');
 assert.throws(()=>sizeValue(-10));
});
