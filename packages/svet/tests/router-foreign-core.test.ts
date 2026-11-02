/** @vitest-environment jsdom */
import {expect,it} from 'vitest';
import {createWasmCoreInstance,withWasmCore,createRoot,createRouter,effect} from '../src/engine';
it('navigate publishes all URL fields atomically in its own core',()=>{
 const core=createWasmCoreInstance();let router!: ReturnType<typeof createRouter>,stop!:()=>void;
 const seen:unknown[]=[];
 withWasmCore(core,()=>createRoot(dispose=>{stop=dispose;router=createRouter({routes:[],initialPath:'/start'});effect(()=>{seen.push([router.path(),router.params().id,router.query().q,router.hash()]);});}));
 try{router.navigate('/next/7?q=value#hash');expect(seen).toEqual([['/start',undefined,undefined,''],['/next/7',undefined,'value','hash']]);}
 finally{stop();}expect(core.svet_get_node_count()).toBe(0);
});
