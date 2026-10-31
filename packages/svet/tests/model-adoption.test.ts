/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createRoot,getGraphTelemetry,h,insertChild,Portal,resetReactiveSystem,signal,type Signal} from '../src/engine';
const tick=async()=>{await Promise.resolve();await Promise.resolve();};
let stops:Array<()=>void>=[],sources:Array<Signal<any>>=[];
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{for(const stop of stops.reverse())stop();for(const source of sources)source.dispose();stops=[];sources=[];vi.restoreAllMocks();});
function target(closed=false){const iframe=document.createElement('iframe');document.body.append(iframe);const doc=iframe.contentDocument!,form=doc.createElement('form');if(closed){const host=doc.createElement('div');doc.body.append(host);host.attachShadow({mode:'closed'}).append(form);}else doc.body.append(form);return{doc,form};}
function model(){const source=signal('edited');sources.push(source);return source;}
it.each([false,true])('Portal into a foreign native form resets its model (closed shadow: %s)',async closed=>{
 const {form}=target(closed),source=model();createRoot(stop=>{stops.push(stop);Portal({mount:form,children:()=>h('input',{model:source})});});form.reset();await tick();expect(source()).toBe('');
});
it('static subtree adoption resets a nested model through its new native form',async()=>{
 const {form}=target(),source=model();createRoot(stop=>{stops.push(stop);insertChild(form,h('div',null,h('input',{model:source})));});form.reset();await tick();expect(source()).toBe('');
});
it.each([false,true])('reset dispatched inside native insertion is observed before adoption completes (closed: %s)',async closed=>{
 const {form}=target(closed),source=model(),append=form.appendChild.bind(form);createRoot(stop=>{stops.push(stop);vi.spyOn(form,'appendChild').mockImplementation(<T extends Node>(node:T):T=>{append(node);form.reset();return node;});insertChild(form,h('input',{model:source}));});await tick();expect(source()).toBe('');
});
it('a canceled destination reset preserves model state',async()=>{
 const {form}=target(),source=model();createRoot(stop=>{stops.push(stop);insertChild(form,h('input',{model:source}));});form.addEventListener('reset',event=>event.preventDefault());form.reset();await tick();expect(source()).toBe('edited');
});
it('adopted reset callbacks retire with their owner before queued publication',async()=>{
 const {form}=target(),source=model();let dispose!:()=>void;createRoot(stop=>{dispose=stop;stops.push(stop);insertChild(form,h('input',{model:source}));});form.reset();dispose();await tick();expect(source()).toBe('edited');source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
it('destination reset registration disposal rejects insertion and releases late leases',()=>{
 const {doc,form}=target(),source=model(),add=doc.addEventListener.bind(doc),remove=vi.spyOn(doc,'removeEventListener');createRoot(stop=>{stops.push(stop);const input=h('input',{model:source});vi.spyOn(doc,'addEventListener').mockImplementation((type,listener,options)=>{if(type==='reset')stop();add(type,listener,options);});expect(()=>insertChild(form,input)).toThrow(/disposed/);expect(input.parentNode).toBeNull();});expect(remove.mock.calls.some(([type])=>type==='reset')).toBe(true);
});
it('removing an old reset root cannot erase a registration created by native removal reentry',async()=>{
 const {form}=target(),first=model(),replacement=model(),remove=document.removeEventListener.bind(document);let nativeForm!:HTMLFormElement,once=true;
 createRoot(stop=>{stops.push(stop);const input=h('input',{model:first});vi.spyOn(document,'removeEventListener').mockImplementation((type,listener,options)=>{if(type==='reset'&&once){once=false;nativeForm=document.createElement('form');nativeForm.append(h('input',{model:replacement}));document.body.append(nativeForm);}remove(type,listener,options);});insertChild(form,input);});nativeForm.reset();await tick();expect(replacement()).toBe('');
});
it('a node can migrate delegated event and model-reset routes together',async()=>{
 const {form}=target(),source=model();let calls=0;createRoot(stop=>{stops.push(stop);insertChild(form,h('input',{model:source,onClick(){calls++;}}));});form.querySelector('input')!.click();form.reset();await tick();expect(calls).toBe(1);expect(source()).toBe('');
});
