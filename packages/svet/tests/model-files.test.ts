/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {disposeResumedContainer,getGraphTelemetry,h,mount,resetReactiveSystem,resumeContainer,serializeDomNodeToHtml,signal} from '../src/engine';

beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{disposeResumedContainer(document.body);vi.restoreAllMocks();});
function emptyFiles():FileList{const input=document.createElement('input');input.type='file';return input.files!;}

it('constructs a file model with a native FileList without stringifying it',()=>{
 const files=emptyFiles(),model=signal<FileList|null>(files);
 const app=mount(()=>h('input',{type:'file',model}),document.body),input=document.querySelector('input')!;
 expect(input.files).toBe(files);app.unmount();model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('publishes the native FileList once on change and ignores input and cancel',()=>{
 const model=signal<FileList|null>(null),app=mount(()=>h('input',{type:'file',model}),document.body),input=document.querySelector('input')!;
 const selected=emptyFiles();input.files=selected;
 input.dispatchEvent(new Event('input'));expect(model()).toBeNull();
 input.dispatchEvent(new Event('change'));expect(model()).toBe(selected);
 input.dispatchEvent(new Event('cancel'));expect(model()).toBe(selected);
 app.unmount();model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('accepts another FileList through the native files setter',()=>{
 const model=signal<FileList|null>(null),app=mount(()=>h('input',{type:'file',model}),document.body),input=document.querySelector('input')!;
 const next=emptyFiles();model.set(next);expect(input.files).toBe(next);
 app.unmount();model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('clears a selected native field with null using only an empty value',()=>{
 const model=signal<FileList|null>(null),app=mount(()=>h('input',{type:'file',model}),document.body),input=document.querySelector('input')!;
 const native=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!;
 let value='C:\\fakepath\\report.txt';const writes:string[]=[];
 Object.defineProperty(input,'value',{configurable:true,get:()=>value,set:next=>{writes.push(next);value=next;native.set!.call(input,next);}});
 input.files=emptyFiles();input.dispatchEvent(new Event('change'));expect(model()).toBe(input.files);
 model.set(null);expect(writes).toEqual(['']);expect(value).toBe('');
 app.unmount();model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('reactive type changes activate file semantics without writing a path',()=>{
 const type=signal('text'),model=signal<FileList|null>(null),app=mount(()=>h('input',{type,model}),document.body),input=document.querySelector('input')!;
 type.set('file');const files=emptyFiles();input.files=files;input.dispatchEvent(new Event('change'));
 expect(model()).toBe(files);model.set(null);expect(input.value).toBe('');
 app.unmount();type.dispose();model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('unmount removes file change publication and source-to-DOM writes',()=>{
 const model=signal<FileList|null>(null),app=mount(()=>h('input',{type:'file',model}),document.body),input=document.querySelector('input')!;
 app.unmount();const files=emptyFiles();input.files=files;input.dispatchEvent(new Event('change'));expect(model()).toBeNull();
 model.set(emptyFiles());expect(input.files).toBe(files);model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('resumes a nullable file model through the common native binding',()=>{
 document.body.innerHTML='<input type="file" data-svet-model="upload">';
 const state=resumeContainer(document.body,JSON.stringify({signals:{upload:null}})),input=document.querySelector('input')!;
 const files=emptyFiles();input.files=files;input.dispatchEvent(new Event('change'));expect(state.upload()).toBe(files);
 state.upload.set(null);expect(input.value).toBe('');disposeResumedContainer(document.body);expect(getGraphTelemetry().nodes).toBe(0);
});

it('SSR omits file paths and value attributes without reading live value',()=>{
 const input=document.createElement('input');input.type='file';input.setAttribute('value','unrestorable');
 Object.defineProperty(input,'value',{get:()=>{throw Error('file value must not be serialized');}});
 expect(serializeDomNodeToHtml(input)).toBe('<input type="file" />');
});

it('invalid file model construction cleans its provisional binding scope',()=>{
 const model=signal<unknown>('a path is not a FileList'),baseline=getGraphTelemetry().nodes;
 expect(()=>mount(()=>h('input',{type:'file',model}),document.body)).toThrow();
 expect(getGraphTelemetry().nodes).toBe(baseline);expect(document.body.childNodes.length).toBe(0);
 model.set(null);model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a file getter that tears down the owner cannot publish a late selection',()=>{
 const model=signal<FileList|null>(null),app=mount(()=>h('input',{type:'file',model}),document.body),input=document.querySelector('input')!;
 const selected=emptyFiles();Object.defineProperty(input,'files',{configurable:true,get:()=>{app.unmount();return selected;}});
 input.dispatchEvent(new Event('change'));expect(model()).toBeNull();
 model.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
