import {expect,it} from 'vitest';
import {installSvetWasmModule,createWasmCoreInstance,getWasmCore,signal} from '../src/engine';
import {decodeWasmBinary} from '../src/engine/wasm-binary';
it('validates a precompiled core before bootstrap and prevents module identity replacement',()=>{
 expect(()=>installSvetWasmModule({} as WebAssembly.Module)).toThrow(/precompiled/);
 const blank=new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]));
 expect(()=>installSvetWasmModule(blank)).toThrow(/export shape/);
 const bytes=decodeWasmBinary(),module=new WebAssembly.Module(bytes as BufferSource);installSvetWasmModule(module);installSvetWasmModule(module);
 const core=getWasmCore(),source=signal(2);expect(source()).toBe(2);source.set(3);expect(source()).toBe(3);source.dispose();expect(core.svet_get_node_count()).toBe(0);
 const isolated=createWasmCoreInstance();expect(isolated).not.toBe(core);expect(isolated.memory).not.toBe(core.memory);
 expect(()=>installSvetWasmModule(new WebAssembly.Module(bytes as BufferSource))).toThrow(/already initialized/);
 expect(getWasmCore()).toBe(core);installSvetWasmModule(module);
});
