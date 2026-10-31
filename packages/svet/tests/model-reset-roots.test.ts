/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createRoot,effect,getCurrentOwner,getGraphTelemetry,h,mount,resetReactiveSystem,signal} from '../src/engine';
let stop:(()=>void)|undefined;
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{stop?.();stop=undefined;vi.restoreAllMocks();});
const tick=async()=>{await Promise.resolve();await Promise.resolve();};
function shadow(mode:ShadowRootMode='open'){const host=document.createElement('div');document.body.append(host);return host.attachShadow({mode});}

it('late native append into a detached Svet form participates in immediate reset',async()=>{
 const text=signal('edited');let form!:HTMLFormElement;
 createRoot(dispose=>{stop=dispose;form=h('form',null) as HTMLFormElement;form.append(h('input',{model:text}));});
 form.reset();await tick();expect(text()).toBe('');stop!();text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('moving a model into another live detached Svet form resets through its new owner form',async()=>{
 const text=signal('edited');let first!:HTMLFormElement,second!:HTMLFormElement;
 createRoot(dispose=>{stop=dispose;first=h('form',null,h('input',{model:text})) as HTMLFormElement;second=h('form',null) as HTMLFormElement;second.append(first.firstChild!);});
 second.reset();await tick();expect(text()).toBe('');stop!();text.dispose();
});

it('late model fields reset within a closed shadow tree without document propagation',async()=>{
 const root=shadow('closed'),text=signal('edited'),documentReset=vi.fn();document.addEventListener('reset',documentReset);
 let form!:HTMLFormElement;
 try{
  createRoot(dispose=>{stop=dispose;form=h('form',null) as HTMLFormElement;root.append(form);form.append(h('input',{model:text}));});
  form.reset();await tick();expect(text()).toBe('');expect(documentReset).not.toHaveBeenCalled();
 }finally{document.removeEventListener('reset',documentReset);stop!();text.dispose();}
});

it('mount inside a native closed-shadow form receives reset outside its container',async()=>{
 const root=shadow('closed'),form=document.createElement('form'),container=document.createElement('div');form.append(container);root.append(form);
 const text=signal('edited'),app=mount(()=>h('input',{model:text}),container);stop=app.unmount;
 form.reset();await tick();expect(text()).toBe('');app.unmount();text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('mount observes forms natively inserted later into its shadow subtree',async()=>{
 const root=shadow(),container=document.createElement('div');root.append(container);const text=signal('edited');let input!:Node;
 const app=mount(()=>{input=h('input',{model:text});return input;},container);stop=app.unmount;
 const form=document.createElement('form');form.append(input);container.append(form);
 form.reset();await tick();expect(text()).toBe('');app.unmount();text.dispose();
});

it('a mounted subtree moved between shadow trees retains its local reset coordinator',async()=>{
 const first=shadow(),second=shadow('closed'),container=document.createElement('div');first.append(container);const text=signal('edited');let input!:Node;
 const app=mount(()=>{input=h('input',{model:text});return input;},container);stop=app.unmount;
 const form=document.createElement('form');form.append(input);container.append(form);second.append(container);
 form.reset();await tick();expect(text()).toBe('');app.unmount();text.dispose();
});

it('shadow-root reset cancellation leaves the current model intact',async()=>{
 const root=shadow(),container=document.createElement('div');root.append(container);const text=signal('edited');
 const app=mount(()=>h('form',{onReset:(event:Event)=>event.preventDefault()},h('input',{model:text})),container);stop=app.unmount;
 (container.firstChild as HTMLFormElement).reset();await tick();expect(text()).toBe('edited');app.unmount();text.dispose();
});

it('unmount releases both the shadow-root and local container reset listeners',()=>{
 const root=shadow(),container=document.createElement('div');root.append(container);
 const rootAdd=vi.spyOn(root,'addEventListener'),rootRemove=vi.spyOn(root,'removeEventListener'),localAdd=vi.spyOn(container,'addEventListener'),localRemove=vi.spyOn(container,'removeEventListener');
 const app=mount(()=>document.createTextNode('no model yet'),container);stop=app.unmount;app.unmount();app.unmount();
 expect(rootAdd.mock.calls.filter(([name])=>name==='reset')).toHaveLength(1);expect(rootRemove.mock.calls.filter(([name])=>name==='reset')).toHaveLength(1);
 expect(localAdd.mock.calls.filter(([name])=>name==='reset')).toHaveLength(1);expect(localRemove.mock.calls.filter(([name])=>name==='reset')).toHaveLength(1);expect(getGraphTelemetry().nodes).toBe(0);
});

it('owner teardown during shadow reset-root registration prevents view execution and releases a late listener',()=>{
 const root=shadow(),container=document.createElement('div');root.append(container);const native=root.addEventListener.bind(root),remove=vi.spyOn(root,'removeEventListener'),view=vi.fn(()=>document.createTextNode('never'));
 createRoot(dispose=>{
  stop=dispose;vi.spyOn(root,'addEventListener').mockImplementation((name,listener,options)=>{if(name==='reset')dispose();native(name,listener,options);});
  expect(()=>mount(view,container)).toThrow(/disposed/);
 });
 expect(view).not.toHaveBeenCalled();expect(remove.mock.calls.filter(([name])=>name==='reset')).toHaveLength(1);expect(getGraphTelemetry().nodes).toBe(0);
});

it('mount accepts a closed ShadowRoot directly and resets late native forms',async()=>{
 const root=shadow('closed'),text=signal('edited');let input!:Node;
 const app=mount(()=>{input=h('input',{model:text});return input;},root);stop=app.unmount;
 const form=document.createElement('form');form.append(input);root.append(form);form.reset();await tick();expect(text()).toBe('');
 app.unmount();text.dispose();expect(root.childNodes.length).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it('replacing a ShadowRoot mount retires old reset routing without clearing the new app',async()=>{
 const root=shadow('closed'),oldText=signal('old'),nextText=signal('next');
 const first=mount(()=>h('form',null,h('input',{model:oldText})),root);
 const second=mount(()=>h('form',null,h('input',{model:nextText})),root);stop=second.unmount;first.unmount();
 const form=root.firstChild as HTMLFormElement;form.reset();await tick();expect(oldText()).toBe('old');expect(nextText()).toBe('');expect(root.firstChild).toBe(form);
 second.unmount();oldText.dispose();nextText.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('document form and mount capture listeners queue only one array-model publication',async()=>{
 const root=shadow(),many=signal(['b']);let form!:HTMLFormElement;
 const app=mount(()=>{form=h('form',null,h('select',{multiple:true,model:many},h('option',{value:'a',selected:true},'A'),h('option',{value:'b'},'B'))) as HTMLFormElement;return form;},root);stop=app.unmount;
 let runs=0;const observed=effect(()=>{many();runs++;});form.reset();await tick();expect(many()).toEqual(['a']);expect(runs).toBe(2);
 observed.dispose();app.unmount();many.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('failed structural form reset registration does not accumulate caller cleanup hooks',()=>{
 vi.spyOn(HTMLFormElement.prototype,'addEventListener').mockImplementation(()=>{throw Error('listener registration failed');});
 createRoot(dispose=>{
  stop=dispose;const owner=getCurrentOwner()!,baseline=owner.disposables.length;
  for(let i=0;i<20;i++){expect(()=>h('form',null)).toThrow('listener registration failed');expect(owner.disposables).toHaveLength(baseline);}
 });
 expect(getGraphTelemetry().nodes).toBe(0);
});
