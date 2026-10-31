/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {createRoot,getGraphTelemetry,insertChild,resetReactiveSystem} from '../src/engine';
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});afterEach(()=>vi.restoreAllMocks());

it('coercion cannot append static child text after owner teardown',()=>{
 const host=document.createElement('div');host.textContent='existing';
 createRoot(dispose=>{expect(()=>insertChild(host,{toString(){dispose();return 'stale';}} as unknown as string)).toThrow(/disposed/);});expect(host.textContent).toBe('existing');expect(getGraphTelemetry().nodes).toBe(0);
});
