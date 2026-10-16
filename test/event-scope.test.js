import {test} from 'node:test';
import assert from 'node:assert/strict';
import {createEventScope} from '../src/event-scope.js';
import {controlLabel,collapseText} from '../src/control-label.js';

class Target{
  constructor(){this.listeners=[];}
  addEventListener(type,listener,options){this.listeners.push({type,listener,options});options.signal.addEventListener('abort',()=>{this.listeners=this.listeners.filter(l=>l.listener!==listener);});}
}
class Observer{
  static created=[];
  constructor(callback){this.callback=callback;this.disconnected=false;Observer.created.push(this);}
  observe(target,options){this.target=target;this.options=options;}
  disconnect(){this.disconnected=true;}
}

test('one dispose releases listeners and every observer the scope created',t=>{
  const previous={m:globalThis.MutationObserver,r:globalThis.ResizeObserver,i:globalThis.IntersectionObserver};
  globalThis.MutationObserver=globalThis.ResizeObserver=globalThis.IntersectionObserver=Observer;
  t.after(()=>{globalThis.MutationObserver=previous.m;globalThis.ResizeObserver=previous.r;globalThis.IntersectionObserver=previous.i;Observer.created=[];});
  const scope=createEventScope(),target=new Target();
  scope.listen(target,'click',()=>{});
  scope.listen(target,'scroll',()=>{},true);
  assert.equal(target.listeners.length,2);
  assert.equal(target.listeners[1].options.capture,true,'a boolean third argument still means capture');
  let calls=0;
  const mutations=scope.observe({},()=>{calls++;},{childList:true});
  assert.equal(calls,1,'observe mirrors the initial state once');
  assert.deepEqual(mutations.options,{childList:true});
  scope.observe({},()=>{calls++;},{},{immediate:false});
  assert.equal(calls,1);
  scope.resize(()=>{});scope.intersect(()=>{});
  assert.equal(Observer.created.length,4);
  scope.dispose();
  assert.equal(target.listeners.length,0);
  assert.ok(Observer.created.every(o=>o.disconnected));
});

test('control labels drop the decorative glyph and collapse whitespace',()=>{
  const el=(text,attrs={})=>({textContent:text,title:'',getAttribute:name=>attrs[name]??null});
  assert.equal(controlLabel(el('▶  Приложение')),'Приложение');
  assert.equal(controlLabel(el('↻ Данные\n дизайна')),'Данные дизайна');
  assert.equal(controlLabel(el('▾',{'aria-label':'Меню проекта'})),'Меню проекта');
  assert.equal(controlLabel(el('Экспорт')),'Экспорт');
  assert.equal(collapseText('  a \n b  '),'a b');
});
