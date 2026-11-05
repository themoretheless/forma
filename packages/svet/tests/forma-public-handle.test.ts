/** @vitest-environment jsdom */
import {beforeEach,expect,it,vi} from 'vitest';
import {compileFormaComponent,getGraphTelemetry,resetReactiveSystem,signal} from '../src/engine';
beforeEach(()=>resetReactiveSystem());
const source=`component App { state count: Number = 1; prop limit: Number = 5; let doubled = state.count * 2; Column { Button { text:doubled; clicked -> state.increment('count',1); } } }`;
it('allows action overrides to wrap originals and intercept mounted events',()=>{
 const instance=compileFormaComponent(source),container=document.createElement('div');
 const original=instance.actions.increment;
 const wrapped=vi.fn((...args:Parameters<typeof original>)=>original(...args));
 instance.actions.increment=wrapped;
 const mount=instance.mount(container);
 try {
  container.querySelector('button')!.click();expect(wrapped).toHaveBeenCalledTimes(1);expect(instance.state.count()).toBe(2);
  instance.actions.increment=original;
  container.querySelector('button')!.click();expect(wrapped).toHaveBeenCalledTimes(1);expect(instance.state.count()).toBe(3);
 } finally {instance.dispose();mount.unmount();}
 expect(getGraphTelemetry().nodes).toBe(0);
});
it('keeps cached reads and inert actions after disposal',()=>{
 const instance=compileFormaComponent(source),container=document.createElement('div'),mount=instance.mount(container);
 expect(instance.computedLets.doubled()).toBe(2);instance.dispose();
 expect(instance.state.count()).toBe(1);expect(instance.props.limit()).toBe(5);expect(instance.computedLets.doubled()).toBe(2);
 instance.state.count.set(99);instance.actions.increment('count',10);expect(instance.state.count()).toBe(1);
 expect(instance.exportHmrSnapshot().state.count).toBe(1);expect(()=>instance.render()).toThrow('disposed');
 mount.unmount();mount.unmount();instance.dispose();expect(getGraphTelemetry().nodes).toBe(0);
});

it('does not evaluate unused lazy expressions during terminal snapshot',()=>{
 const instance=compileFormaComponent(`component App { state count: Number = 1; let unused = state.count * 2; Column {} }`);
 const peek=vi.spyOn(instance.computedLets.unused,'peek').mockImplementation(()=>{throw Error('lazy evaluated');});
 expect(()=>instance.dispose()).not.toThrow();expect(peek).not.toHaveBeenCalled();
 expect(instance.computedLets.unused()).toBeUndefined();expect(getGraphTelemetry().nodes).toBe(0);
});
it('preserves cached external signal values without private cache metadata',()=>{
 const external=signal(7);delete (external as any).__svet_cached_value;
 const instance=compileFormaComponent(`component App { state count: Number = 1; Column {} }`,{viewModel:{count:external}});
 try {expect(instance.state.count()).toBe(7);instance.dispose();expect(instance.state.count()).toBe(7);
 instance.state.count.set(9);expect(external()).toBe(7);
 } finally {instance.dispose();external.dispose();}
 expect(getGraphTelemetry().nodes).toBe(0);
});
