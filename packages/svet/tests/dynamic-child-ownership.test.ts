/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {effect,getGraphTelemetry,h,mount,onCleanup,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>vi.restoreAllMocks());

it('failed preparation preserves the committed nested binding and disposes provisional resources',()=>{
 const mode=signal(false),text=signal('old');let discarded=0;
 const app=mount(()=>h('div',null,()=>{if(mode()){effect(()=>{});onCleanup(()=>{discarded++;});throw Error('prepare failed');}return h('span',null,text);}),document.body),span=document.querySelector('span')!,baseline=getGraphTelemetry().nodes;
 expect(()=>mode.set(true)).toThrow('prepare failed');expect(discarded).toBe(1);expect(getGraphTelemetry().nodes).toBe(baseline);
 text.set('live');expect(span.textContent).toBe('live');expect(document.querySelector('span')).toBe(span);
 app.unmount();mode.dispose();text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed insertion keeps old array accessors reactive until replacement succeeds',()=>{
 const mode=signal(false),text=signal('old'),retry=signal(0);let cleaned=0;
 const app=mount(()=>h('div',null,()=>{retry();if(mode())return ['new'];onCleanup(()=>{cleaned++;});return [text];}),document.body),host=document.querySelector('div')!,native=host.insertBefore.bind(host);let fail=true;
 const insert=vi.spyOn(host,'insertBefore').mockImplementation(function<T extends Node>(node:T,anchor:Node|null):T{if(fail){fail=false;throw Error('insert failed');}return native(node,anchor);});
 expect(()=>mode.set(true)).toThrow('insert failed');expect(cleaned).toBe(0);text.set('still live');expect(host.textContent).toBe('still live');
 insert.mockRestore();retry.set(1);expect(host.textContent).toBe('new');expect(cleaned).toBe(1);text.set('ignored');expect(host.textContent).toBe('new');
 app.unmount();mode.dispose();text.dispose();retry.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a retained consumed fragment keeps its original nested bindings on unrelated reruns',()=>{
 const tick=signal(0),text=signal('old');let fragment:DocumentFragment|undefined,cleaned=0;
 const app=mount(()=>h('div',null,()=>{tick();if(!fragment){onCleanup(()=>{cleaned++;});fragment=document.createDocumentFragment();fragment.append(h('span',null,text));}return fragment;}),document.body),span=document.querySelector('span')!;
 tick.set(1);expect(cleaned).toBe(0);text.set('live');expect(span.textContent).toBe('live');app.unmount();expect(cleaned).toBe(1);tick.dispose();text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('successful text replacement releases exactly the previous content scope',()=>{
 const value=signal('first'),cleaned:string[]=[];
 const app=mount(()=>h('div',null,()=>{const label=value();onCleanup(()=>{cleaned.push(label);});return label;}),document.body);
 value.set('second');expect(document.body.textContent).toBe('second');expect(cleaned).toEqual(['first']);app.unmount();expect(cleaned).toEqual(['first','second']);value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('unmount from committed-content cleanup disposes the new scope without revival',()=>{
 const mode=signal(false);let app!:ReturnType<typeof mount>,newCleanups=0;
 app=mount(()=>h('div',null,()=>{if(!mode()){onCleanup(()=>app.unmount());return 'old';}onCleanup(()=>{newCleanups++;});return h('b',null,'new');}),document.body);
 expect(()=>mode.set(true)).toThrow('disposed');expect(document.body.textContent).toBe('');expect(newCleanups).toBe(1);mode.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a retained single node preserves its committed nested binding scope',()=>{
 const tick=signal(0),text=signal('old');let node:Node|undefined,cleaned=0;
 const app=mount(()=>h('div',null,()=>{tick();if(!node){onCleanup(()=>{cleaned++;});node=h('span',null,text);}return node;}),document.body),span=document.querySelector('span')!;
 tick.set(1);text.set('live');expect(span.textContent).toBe('live');expect(cleaned).toBe(0);
 app.unmount();expect(cleaned).toBe(1);tick.dispose();text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
