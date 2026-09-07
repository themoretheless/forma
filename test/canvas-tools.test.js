import {test} from 'node:test';
import assert from 'node:assert/strict';
import {fitScale} from '../src/canvas-tools.js';
test('fit keeps a logical frame inside narrow and short viewports with room around it',()=>{
 for(const viewport of [{width:450,height:500},{width:1200,height:250}]){
  const scale=fitScale(viewport,720,780);
  assert.ok(720*scale<=viewport.width-48+.001);
  assert.ok(780*scale<=viewport.height-48+.001);
 }
 assert.equal(fitScale({width:2000,height:2000},100,100),2);
 assert.equal(fitScale({width:0,height:0},720,780),.1);
});

test('measurements choose nearest aligned siblings and otherwise the parent',async()=>{
 const {layoutDistances}=await import('../src/layout-distances.js');
 const d=layoutDistances([50,50,20,20],[[10,50,25,20],[100,0,20,20]],[200,100]);
 assert.equal(d.find(x=>x.side==='left').value,15);
 assert.equal(d.find(x=>x.side==='left').target,'сосед');
 assert.equal(d.find(x=>x.side==='right').value,130);
 assert.equal(d.find(x=>x.side==='bottom').value,30);
});
