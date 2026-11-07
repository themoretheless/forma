/** Progressive SSR data protocol. Chunks contain data, never executable patch scripts. */
import {captureReactiveContext,runWithOwnerScope,onCleanup,registerReactiveResetFinalizer,type Signal} from './reactivity.ts';
import {getWasmCore} from './wasm-bridge.ts';
import {captureViewLifetime} from './dom.ts';
import {disposeResumedContainer,resumeContainer,serializeDomNodeToHtml} from './resumable.ts';

import {SSR_STREAM_VERSION,type SsrStreamChunk} from './stream-protocol.ts';
export {SSR_STREAM_VERSION,type SsrStreamChunk} from './stream-protocol.ts';
export interface SsrStreamReceiver {
  readonly active:boolean;
  apply(chunk:SsrStreamChunk): Record<string,Signal<any>> | undefined;
  /** Validate transport EOF; requires a successfully applied complete frame. */
  finish():void;
  dispose():void;
}
const receivers=new WeakMap<HTMLElement,{dispose:()=>void}>();
/** Owns partial DOM and final resumed bindings; dispose or parent teardown closes it. */
export function createSsrStreamReceiver(container:HTMLElement,options:{maxChunkBytes?:number}={}):SsrStreamReceiver {
  const slot:{current?:SsrStreamReceiver}={};
  slot.current=createReceiver(container,options,()=>{slot.current=undefined;});
  return receiverHandle(slot);
}
// Keep the public handle in a separate closure from DOM, context and cleanup state.
// Teardown clears the only edge from a caller-retained handle to that state.
function receiverHandle(slot:{current?:SsrStreamReceiver}):SsrStreamReceiver {
  return {
    get active(){return slot.current?.active??false;},
    dispose(){slot.current?.dispose();},
    finish(){const receiver=slot.current;if(!receiver)throw Error('SSR stream receiver is disposed');receiver.finish();},
    apply(chunk){
      const receiver=slot.current;
      if(!receiver)throw Error('SSR stream receiver is disposed');
      return receiver.apply(chunk);
    },
  };
}
function createReceiver(container:HTMLElement,options:{maxChunkBytes?:number},release:()=>void):SsrStreamReceiver {
  const assertCaller=captureViewLifetime();assertCaller();
  const maxBytes=options.maxChunkBytes??8*1024*1024;assertCaller();if(!Number.isSafeInteger(maxBytes)||maxBytes<1)throw RangeError('Invalid SSR receiver maxChunkBytes');
  const encoder=new TextEncoder();
  receivers.get(container)?.dispose();assertCaller();
  let stop!:()=>void,entry!:ReturnType<typeof captureReactiveContext>,active=true;
  let streamId:string|undefined,complete=false,expectedSequence=0,ownsResumption=false;
  const ranges=new Map<number,{start:Comment;end:Comment}>();
  const token={dispose:()=>stop()};
  runWithOwnerScope([],dispose=>{stop=dispose;entry=captureReactiveContext();
    const unregisterReset=registerReactiveResetFinalizer(getWasmCore(),()=>stop());onCleanup(unregisterReset);
    onCleanup(()=>{
    active=false;release();ranges.clear();if(receivers.get(container)===token){receivers.delete(container);if(ownsResumption)disposeResumedContainer(container);}
  });});
  receivers.set(container,token);
  const fragment=(html:string,parent:Node):DocumentFragment=>{
    const range=container.ownerDocument.createRange();range.selectNodeContents(parent);return range.createContextualFragment(html);
  };
  const scanRanges=(root:Node,id:string)=>{
    const found=new Map<number,{start:Comment;end:Comment}>(),stacks=new Map<Node,number[]>();
    const starts=new Map<number,Comment>(),prefix=`svet:${id}:`,pending:Node[]=[root];
    while(pending.length){const node=pending.pop()!;
      if(node.nodeType===8){const value=node.nodeValue??'';if(value.startsWith(prefix)){
        const rest=value.slice(prefix.length);if(!/^[1-9][0-9]*(?::end)?$/.test(rest))throw Error('Invalid SSR stream boundary');
        const closing=rest.endsWith(':end'),id=Number(closing?rest.slice(0,-4):rest);
        const parent=node.parentNode!;let stack=stacks.get(parent);if(!stack)stacks.set(parent,stack=[]);
        if(!Number.isSafeInteger(id)||id<1)throw Error('Invalid SSR stream boundary');
        if(closing){const start=starts.get(id);if(!start||start.parentNode!==node.parentNode||found.has(id)||stack.pop()!==id)throw Error('Invalid SSR stream range');found.set(id,{start,end:node as Comment});starts.delete(id);}
        else {if(starts.has(id)||found.has(id))throw Error('Duplicate SSR stream boundary');starts.set(id,node as Comment);stack.push(id);}
      }}
      for(let child=node.lastChild;child;child=child.previousSibling)pending.push(child);
    }
    if(starts.size)throw Error('Unclosed SSR stream range');
    return found;
  };
  const acceptRanges=(found:Map<number,{start:Comment;end:Comment}>)=>{ranges.clear();for(const [id,range] of found)ranges.set(id,range);};
  const indexRanges=()=>acceptRanges(scanRanges(container,streamId!));
  return {
    get active(){return active&&entry.active;},dispose:()=>stop(),
    finish(){return entry.run(()=>{
      const assertCurrent=captureViewLifetime();assertCurrent();
      if(!complete){const error=Error('SSR stream ended before complete');try{stop();}catch{/* Preserve EOF failure. */}throw error;}
    });},
    apply(chunk){return entry.run(()=>{
      const assertCurrent=captureViewLifetime();assertCurrent();
      try{
        const version=chunk.version;assertCurrent();
        if(version!==SSR_STREAM_VERSION)throw Error('Unsupported SSR stream version');
        const sequence=chunk.sequence;assertCurrent();
        if(!Number.isSafeInteger(sequence)||sequence!==expectedSequence)throw Error('Invalid SSR stream sequence');
        const id=chunk.streamId;assertCurrent();
        if(streamId!==undefined&&id!==streamId)throw Error('SSR chunk belongs to another stream');
        if(complete)throw Error('SSR stream already completed');
        const payload=chunk.payload;assertCurrent();
        if(!payload||typeof payload!=='object')throw TypeError('Invalid SSR stream payload');
        const type=payload.type;assertCurrent();const html=payload.html;assertCurrent();
        if(typeof id!=='string'||!/^[a-zA-Z0-9-]{1,80}$/.test(id)||typeof html!=='string')throw TypeError('Invalid SSR stream chunk');
        if(encoder.encode(html).byteLength>maxBytes)throw RangeError('SSR chunk exceeds maxChunkBytes');
        if(type==='shell'){
          if(streamId!==undefined)throw Error('Duplicate SSR shell');
          const staged=fragment(html,container);assertCurrent();const found=scanRanges(staged,id);assertCurrent();disposeResumedContainer(container);assertCurrent();
          ownsResumption=true;streamId=id;container.replaceChildren(staged);acceptRanges(found);assertCurrent();expectedSequence++;return;
        }
        if(streamId===undefined||streamId!==id)throw Error('SSR chunk belongs to another stream');
        if(type==='patch'){
          const boundary=payload.boundary;assertCurrent();const range=ranges.get(boundary);
          if(!Number.isSafeInteger(boundary)||!range||range.start.parentNode!==range.end.parentNode||!container.contains(range.start))throw Error('SSR patch range is unavailable');
          const parent=range.start.parentNode!,staged=fragment(html,parent);assertCurrent();const added=scanRanges(staged,id);assertCurrent();
          const nodes:Node[]=[];let node=range.start.nextSibling;
          while(node&&node!==range.end){nodes.push(node);node=node.nextSibling;}
          if(node!==range.end)throw Error('SSR patch range is invalid');
          for(const boundaryId of added.keys()){
            const existing=ranges.get(boundaryId);
            if(existing&&!(nodes.some(node=>node.contains(existing.start))&&nodes.some(node=>node.contains(existing.end))))throw Error('Duplicate SSR stream boundary');
          }
          parent.insertBefore(staged,range.end);for(const old of nodes)old.parentNode?.removeChild(old);indexRanges();assertCurrent();expectedSequence++;return;
        }
        if(type!=='complete')throw TypeError('Unknown SSR stream chunk');
        const stateJson=payload.stateJson;assertCurrent();if(typeof stateJson!=='string')throw TypeError('Invalid SSR stream state');
        if(encoder.encode(html).byteLength+encoder.encode(stateJson).byteLength>maxBytes)throw RangeError('SSR chunk exceeds maxChunkBytes');
        for(const range of ranges.values()){range.start.remove();range.end.remove();}ranges.clear();
        const current=Array.from(container.childNodes).map(serializeDomNodeToHtml).join('');
        // Final HTML reconciles reactive writes made after a boundary was sent.
        // Matching trees keep their existing native DOM identity.
        if(current!==html){const staged=fragment(html,container);assertCurrent();container.replaceChildren(staged);}
        const state=resumeContainer(container,stateJson);assertCurrent();complete=true;expectedSequence++;return state;
      }catch(error){try{stop();}catch{/* Preserve protocol failure. */}throw error;}
    });},
  };
}
