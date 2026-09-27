import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
const main=readFileSync(new URL('../src/main.js',import.meta.url),'utf8');
const source=main.slice(main.indexOf('  let designRequest,'),main.indexOf("  const runApp=document.createElement" )).replaceAll('import.meta.hot','hot');
test('design request timeout/disconnect clear busy state and ignore late replies',()=>{
 const handlers=new Map(),timers=new Map(),buttons=[],sent=[],logs=[],disposers=[];let next=0,applied=0;
 const hot={on:(k,f)=>handlers.set(k,f),off:k=>handlers.delete(k),dispose:f=>disposers.push(f),send:(_,data)=>sent.push(data)};
 runInNewContext(source,{hot,document:{createElement(){const b={};buttons.push(b);return b;}},$:()=>({before(){}}),
  compiled:{},designReferences:()=>['design.VALUE'],confirm:()=>true,files:{'main.ui':'source'},crypto:{randomUUID:()=>String(++next)},
  setTimeout:f=>{timers.set(next,f);return next;},clearTimeout:id=>timers.delete(id),log:s=>logs.push(s),
  setDesignData:()=>applied++,compile(){},output(){},JSON});
 const button=buttons[0];button.onclick();assert.equal(button.disabled,true);
 const old=sent[0];timers.values().next().value();assert.equal(button.disabled,false);assert.equal(timers.size,0);
 handlers.get('forma:design-data-result')({id:old.id,values:{}});assert.equal(applied,0);
 button.onclick();handlers.get('vite:ws:disconnect')();assert.equal(button.disabled,false);assert.equal(timers.size,0);
 button.onclick();handlers.get('forma:design-data-result')({id:sent.at(-1).id,values:{}});
 assert.equal(applied,1);assert.equal(timers.size,0);assert.equal(button.disabled,false);assert.equal(sent.length,3);
 button.onclick();disposers.forEach(dispose=>dispose());
 assert.equal(handlers.size,0);assert.equal(timers.size,0);assert.equal(button.disabled,false);
});
