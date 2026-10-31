/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createRoot,For,getGraphTelemetry,h,mount,Portal,resetReactiveSystem,Show,signal} from '../src/engine';
let stop:(()=>void)|undefined;
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>{stop?.();stop=undefined;vi.restoreAllMocks();});
function tree(mode:ShadowRootMode='closed'){const host=document.createElement('div');document.body.append(host);return {host,root:host.attachShadow({mode})};}

it('delegates clicks and logical target/currentTarget within a closed ShadowRoot mount',()=>{
 const {root}=tree(),calls:EventTarget[]=[],targets:EventTarget[]=[];
 const app=mount(()=>h('section',{onClick:(event:Event)=>{calls.push(event.currentTarget!);targets.push(event.target!);}},h('button',{onClick:(event:Event)=>{calls.push(event.currentTarget!);targets.push(event.target!);}},'click')),root);stop=app.unmount;
 const button=root.querySelector('button')!;button.click();expect(calls).toEqual([button,root.firstChild]);expect(targets).toEqual([button,button]);app.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});

it('handles noncomposed input events within a shadow-mounted element container',()=>{
 const {root}=tree(),container=document.createElement('div');root.append(container);const handler=vi.fn();const app=mount(()=>h('input',{onInput:handler}),container);stop=app.unmount;
 container.querySelector('input')!.dispatchEvent(new Event('input',{bubbles:true,composed:false}));expect(handler).toHaveBeenCalledOnce();
});

it('open shadow delegation reaches outer handlers exactly once with native retargeting',()=>{
 const {root,host}=tree('open'),calls:string[]=[],targets:EventTarget[]=[];
 let stopHost!:()=>void;createRoot(dispose=>{stopHost=dispose;bindProp(host,'onClick',(event:Event)=>{calls.push('host');targets.push(event.target!);});});
 const app=mount(()=>h('button',{onClick:(event:Event)=>{calls.push('button');targets.push(event.target!);}},'click'),root);stop=()=>{app.unmount();stopHost();};
 const button=root.firstChild as HTMLButtonElement;button.click();expect(calls).toEqual(['button','host']);expect(targets).toEqual([button,host]);
});

it('reactive Show and For child handlers inherit the mount delegation area',()=>{
 const {root}=tree(),show=signal(false),rows=signal([1]),calls:number[]=[];
 const app=mount(()=>h('div',null,Show({when:show,children:()=>For({each:rows,keyFn:item=>item,children:item=>h('button',{onClick:()=>calls.push(item)},String(item))})})),root);stop=app.unmount;
 show.set(true);(root.querySelector('button') as HTMLButtonElement).click();rows.set([2]);(root.querySelector('button') as HTMLButtonElement).click();expect(calls).toEqual([1,2]);
 app.unmount();show.dispose();rows.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a Portal into the document retains document event fallback from a shadow mount',()=>{
 const {root}=tree(),portal=document.createElement('div');document.body.append(portal);const handler=vi.fn();
 const app=mount(()=>h('div',null,Portal({mount:portal,children:h('button',{onClick:handler},'portal')})),root);stop=app.unmount;
 (portal.querySelector('button') as HTMLButtonElement).click();expect(handler).toHaveBeenCalledOnce();
});

it('synchronous reuse of the same Event dispatches each binding once per dispatch',()=>{
 const {root}=tree('open'),handler=vi.fn();const app=mount(()=>h('button',{onClick:handler},'click'),root);stop=app.unmount;
 const button=root.firstChild!,event=new MouseEvent('click',{bubbles:true,composed:true});button.dispatchEvent(event);button.dispatchEvent(event);expect(handler).toHaveBeenCalledTimes(2);
});

it('a mounted element moved into another closed tree keeps its event delegation',()=>{
 const first=tree(),second=tree(),container=document.createElement('div');first.root.append(container);const handler=vi.fn();
 const app=mount(()=>h('button',{onClick:handler},'click'),container);stop=app.unmount;second.root.append(container);container.querySelector('button')!.click();expect(handler).toHaveBeenCalledOnce();
});

it('retained shadow nodes cannot invoke callbacks after unmount',()=>{
 const {root}=tree(),handler=vi.fn();const app=mount(()=>h('button',{onClick:handler},'click'),root);stop=app.unmount;const button=root.firstChild as HTMLButtonElement;
 button.click();app.unmount();root.append(button);button.click();expect(handler).toHaveBeenCalledOnce();
});

it('slotted light-DOM handlers run once through document fallback',()=>{
 const {root,host}=tree('open'),handler=vi.fn();let button!:HTMLButtonElement;
 const app=mount(()=>{button=h('button',{onClick:handler},'slotted') as HTMLButtonElement;return h('slot',null);},root);stop=app.unmount;host.append(button);button.click();expect(handler).toHaveBeenCalledOnce();
});

it('an event can be synchronously redispatched after its node moves into the document',()=>{
 const {root}=tree('open'),handler=vi.fn();const app=mount(()=>h('button',{onClick:handler},'click'),root);stop=app.unmount;
 const button=root.firstChild!,event=new MouseEvent('click',{bubbles:true,composed:true});button.dispatchEvent(event);document.body.append(button);button.dispatchEvent(event);expect(handler).toHaveBeenCalledTimes(2);
});

it('stopPropagation inside the closed area prevents native host bubbling',()=>{
 const {root,host}=tree(),nativeHost=vi.fn();host.addEventListener('click',nativeHost);
 const app=mount(()=>h('button',{onClick:(event:Event)=>event.stopPropagation()},'click'),root);stop=app.unmount;(root.firstChild as HTMLButtonElement).click();expect(nativeHost).not.toHaveBeenCalled();
});

it('owner disposal during local event registration releases the late shadow listener',()=>{
 const {root}=tree(),native=root.addEventListener.bind(root),remove=vi.spyOn(root,'removeEventListener');
 createRoot(dispose=>{
  stop=dispose;vi.spyOn(root,'addEventListener').mockImplementation((name,listener,options)=>{if(name==='click')dispose();native(name,listener,options);});
  expect(()=>mount(()=>h('button',{onClick:()=>{}},'never'),root)).toThrow(/disposed/);
 });
 expect(root.childNodes.length).toBe(0);expect(remove.mock.calls.filter(([name])=>name==='click')).toHaveLength(1);expect(getGraphTelemetry().nodes).toBe(0);
});

it('a failed local event registration unlinks the provisional node and document binding',()=>{
 const {root}=tree(),button=document.createElement('button'),handler=vi.fn();root.append(button);
 vi.spyOn(root,'addEventListener').mockImplementation(()=>{throw Error('local listener failed');});
 createRoot(dispose=>{stop=dispose;expect(()=>bindProp(button,'onClick',handler)).toThrow('local listener failed');});
 button.click();expect(handler).not.toHaveBeenCalled();expect(getGraphTelemetry().nodes).toBe(0);
});
