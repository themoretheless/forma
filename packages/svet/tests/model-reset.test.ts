/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {bindProp,createRoot,effect,getGraphTelemetry,h,resetReactiveSystem,signal} from '../src/engine';
let stop:(()=>void)|undefined;
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{stop?.();stop=undefined;vi.restoreAllMocks();});
const tick=async()=>{await Promise.resolve();await Promise.resolve();};
function form(html:string){document.body.innerHTML='<form id="reset-form">'+html+'</form>';return document.querySelector('form')!;}
function bind(values:Record<string,ReturnType<typeof signal<any>>>){createRoot(dispose=>{stop=dispose;for(const [id,model] of Object.entries(values))bindProp(document.getElementById(id)!,'model',model);});}

it('publishes native defaults for text textarea numeric and checkbox controls after reset',async()=>{
 const f=form('<input id="text" value="default"><textarea id="area">original</textarea><input id="num" type="number" value="3"><input id="box" type="checkbox" checked>');
 const text=signal('edited'),area=signal('edited area'),num=signal(12),box=signal(false);bind({text,area,num,box});
 f.reset();expect(text()).toBe('edited');await tick();
 expect([text(),area(),num(),box()]).toEqual(['default','original',3,true]);
 stop!();for(const value of [text,area,num,box])value.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('radio reset publishes the checked default once even if unchecked peers follow it',async()=>{
 const f=form('<input id="a" type="radio" name="choice" value="a" checked><input id="b" type="radio" name="choice" value="b">');
 const choice=signal<string|null>('b');bind({a:choice,b:choice});let runs=0;const observed=effect(()=>{choice();runs++;});
 f.reset();await tick();expect(choice()).toBe('a');expect((document.getElementById('a') as HTMLInputElement).checked).toBe(true);expect(runs).toBe(2);
 observed.dispose();stop!();choice.dispose();
});

it('radio groups without a checked default clear their model',async()=>{
 const f=form('<input id="a" type="radio" name="choice" value="a"><input id="b" type="radio" name="choice" value="b">');
 const choice=signal<string|null>('b');bind({a:choice,b:choice});f.reset();await tick();expect(choice()).toBeNull();stop!();choice.dispose();
});

it('select resets preserve single values and multiple default selection arrays',async()=>{
 const f=form('<select id="one"><option value="a" selected>A</option><option value="b">B</option></select><select id="many" multiple><option value="a" selected>A</option><option value="b">B</option><option value="c" selected>C</option></select>');
 const one=signal('b'),many=signal(['b']);bind({one,many});f.reset();await tick();expect(one()).toBe('a');expect(many()).toEqual(['a','c']);stop!();one.dispose();many.dispose();
});

it('a canceled reset leaves signals and composition state unchanged',async()=>{
 const f=form('<input id="text" value="default">'),text=signal('edited');bind({text});const input=document.getElementById('text') as HTMLInputElement;
 input.dispatchEvent(new CompositionEvent('compositionstart'));input.value='composing';
 f.addEventListener('reset',event=>event.preventDefault());f.reset();await tick();expect(text()).toBe('edited');expect(input.value).toBe('composing');
 input.value='complete';input.dispatchEvent(new CompositionEvent('compositionend'));expect(text()).toBe('complete');stop!();text.dispose();
});

it('pending reset work cannot publish after owner teardown or graph reset',async()=>{
 const f=form('<input id="text" value="default">'),text=signal('edited');bind({text});f.reset();stop!();await tick();expect(text()).toBe('edited');text.dispose();
 const next=signal('next');bind({text:next});f.reset();resetReactiveSystem();const fresh=signal(17);await tick();expect(fresh()).toBe(17);expect(next()).toBe('next');fresh.dispose();
});

it('external form-associated inputs participate in reset',async()=>{
 const f=form('');const input=document.createElement('input');input.id='external';input.setAttribute('form',f.id);input.defaultValue='outside default';document.body.append(input);
 const external=signal('edited');bind({external});f.reset();await tick();expect(external()).toBe('outside default');stop!();external.dispose();
});

it('reset publishes one consistent batch of all changed fields',async()=>{
 const f=form('<input id="first" value="one"><input id="second" value="two">'),first=signal('old one'),second=signal('old two');bind({first,second});
 const values:string[][]=[],observer=effect(()=>{values.push([first(),second()]);});f.reset();await tick();expect(values).toEqual([['old one','old two'],['one','two']]);
 observer.dispose();stop!();first.dispose();second.dispose();
});

it('a successful reset ends composition before publishing native defaults',async()=>{
 const f=form('<input id="text" value="default">'),text=signal('edited');bind({text});const input=document.getElementById('text') as HTMLInputElement;
 input.dispatchEvent(new CompositionEvent('compositionstart'));input.value='composing';f.reset();await tick();expect(text()).toBe('default');
 text.set('after reset');expect(input.value).toBe('after reset');input.dispatchEvent(new CompositionEvent('compositionend'));expect(text()).toBe('after reset');stop!();text.dispose();
});

it('Svet-created detached forms reset model state without a document event',async()=>{
 const text=signal('edited');let f!:HTMLFormElement;
 createRoot(dispose=>{stop=dispose;f=h('form',null,h('input',{model:text})) as HTMLFormElement;});
 f.reset();await tick();expect(text()).toBe('');stop!();text.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('file reset clears a nullable FileList model',async()=>{
 const f=form('<input id="upload" type="file">'),empty=document.createElement('input');empty.type='file';const upload=signal<FileList|null>(empty.files);bind({upload});
 f.reset();await tick();expect(upload()).toBeNull();stop!();upload.dispose();
});

it('a queued option observer cannot overwrite native reset defaults before publication',async()=>{
 const f=form('<select id="one"><option value="a" selected>A</option><option value="b">B</option></select>'),one=signal('b');bind({one});
 const select=document.getElementById('one') as HTMLSelectElement;select.options[0].textContent='changed option';
 f.reset();await tick();expect(one()).toBe('a');expect(select.value).toBe('a');stop!();one.dispose();
});

it('a canceled reset still permits a pending option observer to synchronize its model',async()=>{
 const f=form('<select id="one"><option value="a">A</option></select>'),one=signal('b');bind({one});
 const select=document.getElementById('one') as HTMLSelectElement,option=document.createElement('option');option.value='b';select.append(option);
 f.addEventListener('reset',event=>event.preventDefault());f.reset();await tick();expect(one()).toBe('b');expect(select.value).toBe('b');stop!();one.dispose();
});

it('named form controls cannot hide the native elements collection from reset',async()=>{
 const f=form('<input id="text" name="elements" value="default">'),text=signal('edited');bind({text});f.reset();await tick();expect(text()).toBe('default');stop!();text.dispose();
});

it('repeated binding teardown releases the shared document reset listener',()=>{
 const add=vi.spyOn(document,'addEventListener'),remove=vi.spyOn(document,'removeEventListener');
 for(let i=0;i<20;i++){
   const model=signal('value');createRoot(dispose=>{bindProp(document.createElement('input'),'model',model);dispose();});model.dispose();
 }
 expect(add.mock.calls.filter(([name])=>name==='reset')).toHaveLength(20);
 expect(remove.mock.calls.filter(([name])=>name==='reset')).toHaveLength(20);expect(getGraphTelemetry().nodes).toBe(0);
});

it('a reset event detaches generation-invalidated model registrations',async()=>{
 const f=form('<input id="text" value="default">'),text=signal('edited');bind({text});
 const remove=vi.spyOn(document,'removeEventListener');resetReactiveSystem();const fresh=signal(23);
 f.reset();await tick();expect(fresh()).toBe(23);expect(remove.mock.calls.filter(([name])=>name==='reset')).toHaveLength(1);fresh.dispose();
});

it('owner disposal during document reset-listener registration leaves no late lease',()=>{
 const input=document.createElement('input'),model=signal('edited'),native=document.addEventListener.bind(document),remove=vi.spyOn(document,'removeEventListener');
 createRoot(dispose=>{
  stop=dispose;vi.spyOn(document,'addEventListener').mockImplementation((name,listener,options)=>{if(name==='reset')dispose();native(name,listener,options);});
  expect(()=>bindProp(input,'model',model)).toThrow(/disposed/);
 });
 expect(remove.mock.calls.filter(([name])=>name==='reset')).toHaveLength(1);model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
