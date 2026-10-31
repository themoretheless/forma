/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createRoot,effect,getGraphTelemetry,h,mount,onCleanup,resetReactiveSystem,signal} from '../src/engine';

beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>vi.restoreAllMocks());

it('releases the root when removing mounted DOM throws',()=>{
  const source=signal(0);let runs=0;
  const app=mount(()=>{effect(()=>{source();runs++;});return h('p',null,'mounted');},document.body);
  const failure=new Error('remove failed');
  const replace=vi.spyOn(document.body,'replaceChildren').mockImplementation(()=>{throw failure;});
  expect(()=>app.unmount()).toThrow(failure);
  expect(getGraphTelemetry().nodes).toBe(1);
  source.set(1);expect(runs).toBe(1);
  expect(()=>app.unmount()).not.toThrow();
  replace.mockRestore();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('releases both old and prepared roots when replacement removal throws',()=>{
  const source=signal(0);let runs=0;
  const view=()=>{effect(()=>{source();runs++;});return h('p',null,'view');};
  const first=mount(view,document.body);
  const failure=new Error('replace failed');
  const replace=vi.spyOn(document.body,'replaceChildren').mockImplementation(()=>{throw failure;});
  expect(()=>mount(view,document.body)).toThrow(failure);
  expect(getGraphTelemetry().nodes).toBe(1);
  source.set(1);expect(runs).toBe(2);
  first.unmount();replace.mockRestore();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('stale unmount handles release scopes without clearing the replacement',()=>{
  const source=signal(0);
  const first=mount(()=>{effect(()=>{source();});return h('p',null,'first');},document.body);
  const second=mount(()=>{effect(()=>{source();});return h('p',null,'second');},document.body);
  first.unmount();expect(document.body.textContent).toBe('second');
  expect(getGraphTelemetry().nodes).toBe(2);
  second.unmount();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});


it('rejects a view invalidated by graph reset before insertion',()=>{
  let fresh!: ReturnType<typeof signal<number>>;
  expect(()=>mount(()=>{resetReactiveSystem();fresh=signal(7);return h('p',null,'stale');},document.body)).toThrow('reset');
  expect(document.body.textContent).toBe('');fresh.set(8);expect(fresh()).toBe(8);fresh.dispose();
});

it('rejects mounting after its calling owner is disposed during rendering',()=>{
  let stop!:()=>void;
  createRoot(dispose=>{
    stop=dispose;
    expect(()=>mount(()=>{stop();return h('p',null,'stale');},document.body)).toThrow('disposed');
  });
  expect(document.body.textContent).toBe('');expect(getGraphTelemetry().nodes).toBe(0);
});

it('removes late fragment insertions when the graph resets inside DOM commit',()=>{
  const native=document.body.replaceChildren.bind(document.body);let fresh!:ReturnType<typeof signal<number>>;
  const replace=vi.spyOn(document.body,'replaceChildren').mockImplementation((...nodes)=>{
    resetReactiveSystem();fresh=signal(9);native(...nodes);
  });
  expect(()=>mount(()=>{const fragment=document.createDocumentFragment();fragment.append(h('p',null,'one'),h('p',null,'two'));return fragment;},document.body)).toThrow('reset');
  expect(document.body.textContent).toBe('');expect(getGraphTelemetry().nodes).toBe(1);
  replace.mockRestore();fresh.set(10);expect(fresh()).toBe(10);fresh.dispose();
});


it('checks graph lifetime again after old-app cleanup',()=>{
  mount(()=>{onCleanup(()=>resetReactiveSystem());return h('p',null,'old');},document.body);
  expect(()=>mount(()=>h('p',null,'stale'),document.body)).toThrow('reset');
  expect(document.body.textContent).toBe('');expect(getGraphTelemetry().nodes).toBe(0);
});

it('rolls back nodes attached before a failing native insertion',()=>{
  const native=document.body.replaceChildren.bind(document.body),failure=new Error('late insertion failed');
  const replace=vi.spyOn(document.body,'replaceChildren').mockImplementation((...nodes)=>{native(...nodes);throw failure;});
  expect(()=>mount(()=>{effect(()=>{});return h('p',null,'partial');},document.body)).toThrow(failure);
  expect(document.body.textContent).toBe('');expect(getGraphTelemetry().nodes).toBe(0);replace.mockRestore();
});

it('a newer mount from old-view cleanup supersedes the pending replacement',()=>{
  const source=signal(0);let newer!:ReturnType<typeof mount>;
  const view=(label:string)=>()=>{effect(()=>{source();});return h('p',null,label);};
  mount(()=>{onCleanup(()=>{newer=mount(view('newer'),document.body);});return h('p',null,'old');},document.body);
  expect(()=>mount(view('superseded'),document.body)).toThrow('superseded');
  expect(document.body.textContent).toBe('newer');expect(getGraphTelemetry().nodes).toBe(2);
  source.set(1);newer.unmount();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a newer mount after native insertion keeps the container registry and its DOM',()=>{
  const source=signal(0),native=document.body.replaceChildren.bind(document.body);let newer!:ReturnType<typeof mount>,nested=false;
  const replace=vi.spyOn(document.body,'replaceChildren').mockImplementation((...nodes)=>{
    native(...nodes);
    if(!nested){nested=true;newer=mount(()=>{effect(()=>{source();});return h('p',null,'newer');},document.body);}
  });
  expect(()=>mount(()=>{effect(()=>{source();});return h('p',null,'superseded');},document.body)).toThrow('superseded');
  expect(document.body.textContent).toBe('newer');expect(getGraphTelemetry().nodes).toBe(2);
  replace.mockRestore();newer.unmount();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('mounting a different container during replacement cleanup remains independent',()=>{
  const left=document.createElement('div'),right=document.createElement('div');document.body.append(left,right);let other!:ReturnType<typeof mount>;
  mount(()=>{onCleanup(()=>{other=mount(()=>h('p',null,'right'),right);});return h('p',null,'old');},left);
  const app=mount(()=>h('p',null,'left'),left);
  expect(left.textContent).toBe('left');expect(right.textContent).toBe('right');
  app.unmount();other.unmount();expect(getGraphTelemetry().nodes).toBe(0);
});
