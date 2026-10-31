import {beforeEach,expect,it} from 'vitest';
import {signal,computed,effect,untrack,createRoot,createGpuTensor,resetReactiveSystem,getGraphTelemetry,ReactiveStabilizationError} from '../src/engine';

beforeEach(()=>resetReactiveSystem());

it.each(['scalar','primed','computed','tensor'] as const)('%s untracked self-writes stabilize after the callback commits',mode=>{
  const source=signal(mode==='primed'?-1:0),tensor=createGpuTensor([0]),derived=computed(()=>source());const values:number[]=[];
  const observer=effect(()=>{
    const value=mode==='tensor'?tensor()[0]:mode==='computed'?derived():source();values.push(value);
    if(value>=0&&value<3)untrack(()=>{if(mode==='tensor')tensor.setAt(0,value+1);else source.set(value+1);});
  });
  if(mode==='primed')source.set(0);
  expect(values).toEqual(mode==='primed'?[-1,0,1,2,3]:[0,1,2,3]);
  observer.dispose();derived.dispose();tensor.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a nested computed write reschedules a paused outer effect that already read the source',()=>{
  const source=signal(0),writer=computed(()=>{source.set(1);return 0;});const values:number[]=[];
  const observer=effect(()=>{values.push(source());untrack(()=>writer());});
  expect(values).toEqual([0,1]);observer.dispose();writer.dispose();source.dispose();
});

it('nested effect initialization cannot hide a write from its paused outer observer',()=>{
  const source=signal(0),values:number[]=[],stops:Array<()=>void>=[];
  const observer=effect(()=>{const value=source();values.push(value);if(value===0)createRoot(stop=>{stops.push(stop);effect(()=>{source.set(1);});});});
  expect(values).toEqual([0,1]);observer.dispose();stops.forEach(stop=>stop());source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('untracked reads do not create a dependency or request false self-write reruns',()=>{
  const source=signal(0);let runs=0;
  const observer=effect(()=>{runs++;untrack(()=>{const value=source();if(value<3)source.set(value+1);});});
  expect(runs).toBe(1);expect(source.peek()).toBe(1);observer.dispose();source.dispose();
});

it('a replacement source with a recycled ID cannot reschedule an old retired read',()=>{
  const old=signal(0);let fresh!:ReturnType<typeof signal<number>>,runs=0;
  const observer=effect(()=>{old();runs++;if(!fresh)untrack(()=>{old.dispose();fresh=signal(100);fresh.set(101);});});
  expect(runs).toBe(1);observer.dispose();fresh.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('nonstabilizing untracked writes still hit the typed self-write limit',()=>{
  const source=signal(0);let runs=0;
  expect(()=>effect(()=>{runs++;const value=source();untrack(()=>source.set(value+1));})).toThrow(ReactiveStabilizationError);
  expect(runs).toBe(100);expect(getGraphTelemetry().nodes).toBe(1);source.dispose();
});

it('paused read prefixes include dependency storage beyond the inline pair',()=>{
  const sources=[signal(0),signal(0),signal(0),signal(0)],values:number[]=[];
  const observer=effect(()=>{sources[0]();sources[1]();const value=sources[2]();sources[3]();values.push(value);if(value===0)untrack(()=>sources[2].set(1));});
  expect(values).toEqual([0,1]);observer.dispose();sources.forEach(source=>source.dispose());
});

it('dependencies dropped in this invocation do not cause false untracked reruns',()=>{
  const gate=signal(0),source=signal(0);let runs=0;
  const observer=effect(()=>{runs++;if(gate()===0)source();else untrack(()=>source.set(source.peek()+1));});
  gate.set(1);expect(runs).toBe(2);expect(source.peek()).toBe(1);observer.dispose();gate.dispose();source.dispose();
});

it('paused outer computed dependencies see writes inside a nested computation',()=>{
  const source=signal(0),derived=computed(()=>source()),writer=computed(()=>{source.set(1);return 0;}),values:number[]=[];
  const observer=effect(()=>{values.push(derived());untrack(()=>writer());});
  expect(values).toEqual([0,1]);observer.dispose();derived.dispose();writer.dispose();source.dispose();
});
