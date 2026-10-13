import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mountDevBridge} from '../src/dev-bridge.js';

function harness(){
 const handlers=new Map(),timers=new Map(),buttons=[],sent=[],logs=[],disposers=[];let next=0;
 const counts={applied:0,errors:[],inspections:[],commands:[]};
 const hot={on:(k,f)=>handlers.set(k,f),off:k=>handlers.delete(k),dispose:f=>disposers.push(f),send:(type,data)=>sent.push({type,...data})};
 const env={document:{createElement(){const b={};buttons.push(b);return b;}},confirm:()=>true,crypto:{randomUUID:()=>String(++next)},
  setTimeout:f=>{timers.set(next,f);return next;},clearTimeout:id=>timers.delete(id)};
 const studio={anchor:{before(...nodes){studio.mounted=nodes;}},compiled:{},files:{'main.ui':'source'},designReferences:()=>['design.VALUE'],
  log:s=>logs.push(s),showEvents(){},setDesignData:()=>counts.applied++,compile(){},setError:e=>counts.errors.push(e),persist(){},
  ideCommand:(command,args)=>{counts.commands.push(command);if(command==='boom')throw Error('bad');return {command,args};},
  showNativeInspection:data=>counts.inspections.push(data)};
 mountDevBridge(hot,studio,env);
 return {handlers,timers,buttons,sent,logs,disposers,counts,studio};
}

test('design request timeout/disconnect clear busy state and ignore late replies',()=>{
 const {handlers,timers,buttons,sent,disposers,counts}=harness();
 const button=buttons[0],requests=()=>sent.filter(m=>m.type==='forma:design-data');
 button.onclick();assert.equal(button.disabled,true);
 const old=requests()[0];timers.values().next().value();assert.equal(button.disabled,false);assert.equal(timers.size,0);
 handlers.get('forma:design-data-result')({id:old.id,values:{}});assert.equal(counts.applied,0);
 button.onclick();handlers.get('vite:ws:disconnect')();assert.equal(button.disabled,false);assert.equal(timers.size,0);
 button.onclick();handlers.get('forma:design-data-result')({id:requests().at(-1).id,values:{}});
 assert.equal(counts.applied,1);assert.equal(timers.size,0);assert.equal(button.disabled,false);assert.equal(requests().length,3);
 button.onclick();disposers.forEach(dispose=>dispose());
 assert.equal(handlers.size,0);assert.equal(timers.size,0);assert.equal(button.disabled,false);
});

test('a changed project discards the reply and a Rust error becomes a diagnostic',()=>{
 const {handlers,buttons,sent,logs,counts,studio}=harness();
 buttons[0].onclick();studio.files={'main.ui':'edited'};
 handlers.get('forma:design-data-result')({id:sent.at(-1).id,values:{}});
 assert.equal(counts.applied,0);assert.match(logs.at(-1),/Проект изменился/);
 buttons[0].onclick();handlers.get('forma:design-data-result')({id:sent.at(-1).id,error:'cargo failed'});
 assert.deepEqual(counts.errors,['cargo failed']);
});

test('the transport answers commands and Rust output feeds the inspector line by line',()=>{
 const {handlers,buttons,sent,logs,counts,studio}=harness();
 assert.equal(studio.mounted.length,4,'design data, app, Rust run and stop sit before the run button');
 assert.equal(sent[0].type,'forma:hello');
 handlers.get('forma:command')({id:'a',command:'ide_status',args:{}});
 handlers.get('forma:command')({id:'b',command:'boom'});
 assert.deepEqual(sent.filter(m=>m.type==='forma:result').map(m=>[m.id,m.error??m.result.command]),[['a','ide_status'],['b','bad']]);
 const [,,runRust,stopRust]=buttons;
 runRust.onclick();assert.equal(runRust.disabled,true);assert.equal(stopRust.disabled,false);
 handlers.get('forma:rust-output')({text:'FORMA_INSPECT {"x"'});
 handlers.get('forma:rust-output')({text:':1}\nhello\n',kind:'finished'});
 assert.deepEqual(counts.inspections,[{x:1}]);
 assert.ok(logs.includes('[Rust] :1}\nhello\n'));
 assert.equal(runRust.disabled,false);assert.equal(stopRust.disabled,true);
});
