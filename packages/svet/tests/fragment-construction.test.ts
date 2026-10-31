/** @vitest-environment jsdom */
import {afterEach,beforeEach,expect,it,vi} from 'vitest';
import {computed,createRoot,effect,Fragment,getGraphTelemetry,onCleanup,resetReactiveSystem,signal,type ChildNodeValue,type Signal} from '../src/engine';
let stops:Array<()=>void>=[],sources:Array<Signal<any>>=[];
beforeEach(()=>{resetReactiveSystem();document.body.replaceChildren();});
afterEach(()=>{for(const stop of stops.reverse())stop();for(const source of sources)source.dispose();stops=[];sources=[];vi.restoreAllMocks();});
function source(){const value=signal(0);sources.push(value);return value;}
it('reads children metadata once and preserves reactive children after fragment consumption',()=>{
 const value=source(),host=document.createElement('div');let reads=0,dispose!:()=>void;createRoot(stop=>{dispose=stop;stops.push(stop);host.append(Fragment({get children(){reads++;return value;}}));});expect(reads).toBe(1);expect(host.textContent).toBe('0');value.set(1);expect(host.textContent).toBe('1');dispose();value.set(2);expect(host.textContent).toBe('1');
});
it('disposed callers are rejected before native fragment creation and metadata reads',()=>{
 const create=vi.spyOn(document,'createDocumentFragment');let reads=0;createRoot(stop=>{stops.push(stop);stop();expect(()=>Fragment({get children(){reads++;return 'stale';}})).toThrow(/disposed/);});expect(create).not.toHaveBeenCalled();expect(reads).toBe(0);
});
it('throwing children metadata retires provisional effects while its caller survives',()=>{
 const value=source(),failure=Error('children getter failed');let runs=0;createRoot(stop=>{stops.push(stop);const before=getGraphTelemetry().nodes;expect(()=>Fragment({get children():ChildNodeValue{effect(()=>{value();runs++;});throw failure;}})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(before);});value.set(1);expect(runs).toBe(1);
});
it('metadata teardown does not reread the getter or move existing children',()=>{
 const node=document.createTextNode('original'),host=document.createElement('div');host.append(node);let reads=0;createRoot(stop=>{stops.push(stop);expect(()=>Fragment({get children(){reads++;stop();return node;}})).toThrow(/disposed/);});expect(reads).toBe(1);expect(node.parentNode).toBe(host);
});
it('native fragment creation teardown rejects before reading children',()=>{
 const create=document.createDocumentFragment.bind(document);let reads=0;createRoot(stop=>{stops.push(stop);vi.spyOn(document,'createDocumentFragment').mockImplementationOnce(()=>{const fragment=create();stop();return fragment;});expect(()=>Fragment({get children(){reads++;return undefined;}})).toThrow(/disposed/);});expect(reads).toBe(0);
});
it('native creation reset preserves independent replacement effects and skips metadata',()=>{
 const create=document.createDocumentFragment.bind(document),values:number[]=[];let fresh!:Signal<number>,observer!:ReturnType<typeof effect>,reads=0;createRoot(stop=>{stops.push(stop);vi.spyOn(document,'createDocumentFragment').mockImplementationOnce(()=>{const fragment=create();resetReactiveSystem();fresh=signal(17);observer=effect(()=>{values.push(fresh());});return fragment;});expect(()=>Fragment({get children(){reads++;return undefined;}})).toThrow(/reset/);});fresh.set(18);expect(reads).toBe(0);expect(values).toEqual([17,18]);observer.dispose();fresh.dispose();
});
it('metadata reset rejects existing-node output and preserves replacement lifetimes',()=>{
 const node=document.createTextNode('original'),host=document.createElement('div'),values:number[]=[];host.append(node);let fresh!:Signal<number>,observer!:ReturnType<typeof effect>,reads=0;createRoot(stop=>{stops.push(stop);expect(()=>Fragment({get children(){reads++;resetReactiveSystem();fresh=signal(17);observer=effect(()=>{values.push(fresh());});return node;}})).toThrow(/reset/);});fresh.set(18);expect(reads).toBe(1);expect(node.parentNode).toBe(host);expect(values).toEqual([17,18]);observer.dispose();fresh.dispose();
});
it('failed child coercion retires metadata-owned resources and preserves the primary exception',()=>{
 const value=source(),failure=Error('coercion failed');let runs=0,cleaned=0;createRoot(stop=>{stops.push(stop);const before=getGraphTelemetry().nodes;expect(()=>Fragment({get children(){effect(()=>{value();runs++;});onCleanup(()=>{cleaned++;throw Error('cleanup failed');});return {toString(){throw failure;}} as unknown as ChildNodeValue;}})).toThrow(failure);expect(getGraphTelemetry().nodes).toBe(before);});value.set(1);expect([runs,cleaned]).toEqual([1,1]);
});
it('rerun cleanup of an earlier Fragment cannot execute a later old Fragment subscription',()=>{
 const trigger=source(),secondary=source();let runs=0,computes=0,observed=-1,derived!:()=>number;createRoot(stop=>{stops.push(stop);effect(()=>{trigger();Fragment({get children(){onCleanup(()=>{secondary.set(secondary.peek()+1);observed=derived();});return null;}});Fragment({get children(){derived=computed(()=>{computes++;return secondary();});effect(()=>{derived();runs++;});return null;}});});});trigger.set(1);expect([runs,computes,observed]).toEqual([2,2,0]);
});
