/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createRoot,disposeResumedContainer,getGraphTelemetry,resetReactiveSystem,resumeContainer,signal} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>vi.restoreAllMocks());
const snapshot=(value:unknown,type='text',attrName?:string)=>JSON.stringify({signals:{value},bindings:[{elementId:'field',signal:'value',type,attrName}],timestamp:0});
function host(markup='<span id="field">old</span>'){const element=document.createElement('div');element.innerHTML=markup;document.body.append(element);return element;}

it('rejects a disposed calling owner before replacing existing live bindings',()=>{
 const container=host(),state=resumeContainer(container,snapshot('live'));
 createRoot(dispose=>{dispose();expect(()=>resumeContainer(container,snapshot('stale'))).toThrow('disposed');});
 state.value.set('still live');expect(container.textContent).toBe('still live');disposeResumedContainer(container);expect(getGraphTelemetry().nodes).toBe(0);
});

it('graph reset during an attribute binding rejects stale publication and preserves fresh nodes',()=>{
 const container=host('<p id="field"></p>'),element=container.firstElementChild!,native=element.setAttribute.bind(element);let fresh!:ReturnType<typeof signal<number>>;
 const set=vi.spyOn(element,'setAttribute').mockImplementation((name,value)=>{resetReactiveSystem();fresh=signal(7);native(name,value);});
 expect(()=>resumeContainer(container,snapshot('title','attr','title'))).toThrow('reset');set.mockRestore();disposeResumedContainer(container);expect(fresh()).toBe(7);fresh.set(8);expect(fresh()).toBe(8);fresh.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('manual disposal during initial DOM writes cancels the pending resumption',()=>{
 const container=host('<p id="field"></p>'),element=container.firstElementChild!,native=element.setAttribute.bind(element);
 const set=vi.spyOn(element,'setAttribute').mockImplementation((name,value)=>{disposeResumedContainer(container);native(name,value);});
 expect(()=>resumeContainer(container,snapshot('value','attr','title'))).toThrow('disposed');set.mockRestore();expect(getGraphTelemetry().nodes).toBe(0);disposeResumedContainer(container);
});

it('caller owner teardown during model registration removes late event listeners',()=>{
 const container=host('<input id="field">'),input=container.querySelector('input')!,native=input.addEventListener.bind(input);let stop!:()=>void;
 const add=vi.spyOn(input,'addEventListener').mockImplementation((type,listener,options)=>{if(type==='input')stop();native(type,listener,options);});
 createRoot(dispose=>{stop=dispose;expect(()=>resumeContainer(container,snapshot('value','model'))).toThrow('disposed');});
 add.mockRestore();input.value='late';input.dispatchEvent(new Event('input'));expect(getGraphTelemetry().nodes).toBe(0);disposeResumedContainer(container);
});

it('newer independently owned reentrant resumption keeps its registry entry and bindings',()=>{
 const container=host('<p id="field"></p>'),element=container.firstElementChild!,native=element.setAttribute.bind(element);let nested=false,newer!:ReturnType<typeof resumeContainer>;
 const set=vi.spyOn(element,'setAttribute').mockImplementation((name,value)=>{native(name,value);if(!nested){nested=true;newer=createRoot(()=>resumeContainer(container,snapshot('newer','attr','title')));}});
 expect(()=>resumeContainer(container,snapshot('older','attr','title'))).toThrow('superseded');set.mockRestore();expect(element.getAttribute('title')).toBe('newer');
 newer.value.set('latest');expect(element.getAttribute('title')).toBe('latest');disposeResumedContainer(container);expect(getGraphTelemetry().nodes).toBe(0);
});

it('runtime attribute writes reject graph reset without corrupting new slots',()=>{
 const container=host('<p id="field"></p>'),element=container.firstElementChild!,state=resumeContainer(container,snapshot('initial','attr','title')),native=element.setAttribute.bind(element);let fresh!:ReturnType<typeof signal<number>>;
 const set=vi.spyOn(element,'setAttribute').mockImplementation((name,value)=>{resetReactiveSystem();fresh=signal(9);native(name,value);});
 expect(()=>state.value.set('next')).toThrow('reset');set.mockRestore();disposeResumedContainer(container);expect(fresh()).toBe(9);fresh.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a newer resumption from old listener cleanup supersedes the waiting replacement',()=>{
 const container=host('<input id="field">'),input=container.querySelector('input')!;resumeContainer(container,snapshot('old','model'));const native=input.removeEventListener.bind(input);let reentered=false,newer!:ReturnType<typeof resumeContainer>;
 const remove=vi.spyOn(input,'removeEventListener').mockImplementation((type,listener,options)=>{native(type,listener,options);if(type==='input'&&!reentered){reentered=true;newer=resumeContainer(container,snapshot('newer','attr','title'));}});
 expect(()=>resumeContainer(container,snapshot('superseded','attr','title'))).toThrow('superseded');remove.mockRestore();expect(input.getAttribute('title')).toBe('newer');newer.value.set('live');expect(input.getAttribute('title')).toBe('live');disposeResumedContainer(container);expect(getGraphTelemetry().nodes).toBe(0);
});

it('nested resumption cannot escape an owner disposed while replacing itself',()=>{
 const container=host('<p id="field"></p>'),element=container.firstElementChild!,native=element.setAttribute.bind(element);let nested=false;
 const set=vi.spyOn(element,'setAttribute').mockImplementation((name,value)=>{native(name,value);if(!nested){nested=true;resumeContainer(container,snapshot('nested','attr','title'));}});
 expect(()=>resumeContainer(container,snapshot('outer','attr','title'))).toThrow('disposed');set.mockRestore();disposeResumedContainer(container);expect(getGraphTelemetry().nodes).toBe(0);
});
