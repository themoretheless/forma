import {beforeEach,expect,it} from 'vitest';
import {computed,createRoot,getGraphTelemetry,resetReactiveSystem,signal,type Signal} from '../src/engine';
beforeEach(()=>resetReactiveSystem());

it.each(['signal','computed'])('%s does not allocate a native node when the equals getter throws',kind=>{
 const failure=Error('equals getter failed'),options={get equals():((a:number,b:number)=>boolean){throw failure;}},baseline=getGraphTelemetry().nodes;
 expect(()=>kind==='signal'?signal(1,options):computed(()=>1,options)).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(baseline);
});

it.each(['signal','computed'])('%s rejects reset during the equals getter without touching the replacement graph',kind=>{
 let fresh!:Signal<number>;const options={get equals(){resetReactiveSystem();fresh=signal(17);return Object.is;}};
 expect(()=>kind==='signal'?signal(1,options):computed(()=>1,options)).toThrow(/reset/);expect(fresh()).toBe(17);fresh.set(18);expect(fresh()).toBe(18);expect(getGraphTelemetry().nodes).toBe(1);fresh.dispose();
});

it('computed rechecks owner lifetime after reading equals before allocating a native node',()=>{
 createRoot(stop=>{const options={get equals(){stop();return Object.is;}};expect(()=>computed(()=>1,options)).toThrow(/disposed/);expect(getGraphTelemetry().nodes).toBe(0);});
});

it.each(['signal','computed'])('%s rejects invalid comparator values before allocation',kind=>{
 for(const equals of [false,0,'same',{}]){
  const options={equals} as unknown as {equals:(a:number,b:number)=>boolean},baseline=getGraphTelemetry().nodes;
  expect(()=>kind==='signal'?signal(1,options):computed(()=>1,options)).toThrow(TypeError);expect(getGraphTelemetry().nodes).toBe(baseline);
 }
});

it('computed rejects a nonfunction body before options callbacks or native allocation',()=>{
 let reads=0;const options={get equals(){reads++;return Object.is;}};
 expect(()=>computed(1 as unknown as ()=>number,options)).toThrow(TypeError);expect(reads).toBe(0);expect(getGraphTelemetry().nodes).toBe(0);
});

it.each(['signal','computed'])('%s snapshots comparator metadata once and preserves normal behavior',kind=>{
 let reads=0,calls=0;const options={get equals(){reads++;return (a:number,b:number)=>{calls++;return a===b;};}},source=signal(1);
 const value=kind==='signal'?signal(1,options):computed(()=>source(),options);
 expect(reads).toBe(1);expect(value()).toBe(1);if(kind==='signal')(value as Signal<number>).set(2);else source.set(2);expect(value()).toBe(2);expect(reads).toBe(1);expect(calls).toBe(1);value.dispose();source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('a throwing getter retains its primary error after resetting and creating fresh state',()=>{
 const failure=Error('primary getter failure');let fresh!:Signal<number>;
 expect(()=>signal(1,{get equals():(a:number,b:number)=>boolean{resetReactiveSystem();fresh=signal(17);throw failure;}})).toThrow(failure);expect(fresh()).toBe(17);expect(getGraphTelemetry().nodes).toBe(1);fresh.dispose();
});

it('bare signals retain independent lifetime when the comparator getter tears down a caller root',()=>{
 let source!:Signal<number>;
 createRoot(stop=>{source=signal<number>(1,{get equals(){stop();return Object.is;}});});source.set(2);expect(source()).toBe(2);expect(getGraphTelemetry().nodes).toBe(1);source.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});
