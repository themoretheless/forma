import {SSR_STREAM_VERSION,SSR_STREAM_HTML_REFERENCE_CODEC,type SsrStreamChunk} from './stream-protocol.ts';
/** Optional transport framing. Decode before passing records to a v1 receiver. */
export {SSR_STREAM_HTML_REFERENCE_CODEC} from './stream-protocol.ts';
export interface SsrStreamHtmlReference {
  codec:typeof SSR_STREAM_HTML_REFERENCE_CODEC;
  record:SsrStreamChunk;
  referenceSequence:number;
  prefix:string;
  suffix:string;
}
export type SsrStreamTransportFrame=SsrStreamChunk|SsrStreamHtmlReference;
export interface SsrStreamTransportEncoder {encode(chunk:SsrStreamChunk):SsrStreamTransportFrame;dispose():void;}
export interface SsrStreamTransportDecoder {decode(frame:SsrStreamTransportFrame):SsrStreamChunk;dispose():void;}
function transportState(options:{maxChunkBytes?:number}) {
  const maxBytes=options.maxChunkBytes??8*1024*1024;
  if(!Number.isSafeInteger(maxBytes)||maxBytes<1)throw RangeError('Invalid SSR codec maxChunkBytes');
  const encoder=new TextEncoder();
  let previous:{html:string;sequence:number}|undefined,id:string|undefined,sequence=0,closed=false,chunkBytes=0;
  const dispose=()=>{closed=true;previous=undefined;id=undefined;};
  const size=(value:unknown)=>encoder.encode(JSON.stringify(value)).byteLength;
  function canonical(chunk:SsrStreamChunk):SsrStreamChunk {
    if(closed)throw Error('SSR transport codec is closed');
    if(chunk.version!==SSR_STREAM_VERSION||chunk.sequence!==sequence)throw Error('Invalid SSR codec version/sequence');
    if(typeof chunk.streamId!=='string'||!/^[a-zA-Z0-9-]{1,80}$/.test(chunk.streamId)||(id!==undefined&&id!==chunk.streamId))throw Error('Invalid SSR codec stream identity');
    const payload=chunk.payload;
    if(!payload||typeof payload.html!=='string')throw Error('Invalid SSR codec HTML');
    if(sequence===0&&payload.type!=='shell'||sequence>0&&payload.type==='shell')throw Error('Invalid SSR codec shell');
    let result:SsrStreamChunk;
    if(payload.type==='shell')result={version:1,sequence,streamId:chunk.streamId,payload:{type:'shell',html:payload.html}};
    else if(payload.type==='patch'&&Number.isSafeInteger(payload.boundary)&&payload.boundary>0)result={version:1,sequence,streamId:chunk.streamId,payload:{type:'patch',boundary:payload.boundary,html:payload.html}};
    else if(payload.type==='complete'&&typeof payload.stateJson==='string')result={version:1,sequence,streamId:chunk.streamId,payload:{type:'complete',html:payload.html,stateJson:payload.stateJson}};
    else throw Error('Invalid SSR codec payload');
    chunkBytes=size(result);
    if(chunkBytes>maxBytes)throw RangeError('SSR codec chunk exceeds maxChunkBytes');
    return result;
  }
  const commit=(chunk:SsrStreamChunk)=>{id=chunk.streamId;sequence++;previous={html:chunk.payload.html,sequence:chunk.sequence};if(chunk.payload.type==='complete')dispose();};
  return {maxBytes,canonical,commit,size,dispose,get previous(){return previous;},get chunkBytes(){return chunkBytes;}};
}
/** Retains at most the immediately preceding HTML, released on complete/error/dispose. */
export function createSsrStreamTransportEncoder(options:{maxChunkBytes?:number}={}):SsrStreamTransportEncoder {
  const state=transportState(options);
  return {dispose:state.dispose,encode(input){try{
    const chunk=state.canonical(input),previous=state.previous;let frame:SsrStreamTransportFrame=chunk;
    if(previous&&previous.html.length>=128){
      const offset=chunk.payload.html.indexOf(previous.html);
      if(offset>=0){
        const candidate:SsrStreamHtmlReference={codec:SSR_STREAM_HTML_REFERENCE_CODEC,record:{...chunk,payload:{...chunk.payload,html:''}},referenceSequence:previous.sequence,prefix:chunk.payload.html.slice(0,offset),suffix:chunk.payload.html.slice(offset+previous.html.length)};
        if(state.size(candidate)<state.chunkBytes)frame=candidate;
      }
    }
    state.commit(chunk);return frame;
  }catch(error){state.dispose();throw error;}}};
}
export function createSsrStreamTransportDecoder(options:{maxChunkBytes?:number}={}):SsrStreamTransportDecoder {
  const state=transportState(options);
  return {dispose:state.dispose,decode(frame){try{
    let input:SsrStreamChunk;
    if('codec' in frame){
      if(frame.codec!==SSR_STREAM_HTML_REFERENCE_CODEC)throw Error('Unsupported SSR transport codec');
      const previous=state.previous;
      if(!previous||frame.referenceSequence!==previous.sequence||typeof frame.prefix!=='string'||typeof frame.suffix!=='string'||frame.record.payload.html!=='')throw Error('Invalid SSR HTML reference');
      // Validate identity/sequence and bounded framing before concatenating strings.
      state.canonical(frame.record);
      if(frame.prefix.length+previous.html.length+frame.suffix.length>state.maxBytes||state.size(frame)>state.maxBytes)throw RangeError('SSR codec chunk exceeds maxChunkBytes');
      input={...frame.record,payload:{...frame.record.payload,html:frame.prefix+previous.html+frame.suffix}};
    }else input=frame;
    const chunk=state.canonical(input);state.commit(chunk);return chunk;
  }catch(error){state.dispose();throw error;}}};
}
