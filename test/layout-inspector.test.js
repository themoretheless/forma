import {test} from 'node:test';
import assert from 'node:assert/strict';
import {resizeTracks,trackSource} from '../src/layout-inspector.js';
import {designPreset} from '../src/design-presets.js';
test('moving a Grid boundary fixes only its adjacent tracks and preserves other expressions',()=>{
 const values=[{expr:'*'},120,{expr:'2*'},{expr:'auto'}];
 assert.deepEqual(resizeTracks(values,[200,120,400,30],0,500),[320,0,'2*','auto']);
 assert.equal(trackSource(resizeTracks(values,[200,120,400,30],0,-50)),'[150, 170, 2*, auto]');
 assert.equal(values[0].expr,'*');
 assert.equal(trackSource([{expr:'120px'},{expr:'props.width'},'*']),'[120px, props.width, *]');
 assert.throws(()=>trackSource(['malformed; width: 10']),/нельзя/);
});
test('design presets never modify source nodes',()=>{
 const n={type:'TextField',props:{text:'Label',value:'Hello'}};
 assert.equal(designPreset('empty',n).value,'');
 assert.equal(n.props.value,'Hello');
 assert.equal(designPreset('disabled',n).disabled,true);
});
