import {expect,it} from 'vitest';
import {negotiateSsrStreamVersion,assertSsrStreamResponse,SSR_STREAM_ACCEPT_HEADER,SSR_STREAM_VERSION_HEADER} from '../src/engine';
it('negotiates only explicit supported versions from a bounded valid offer',()=>{
 for(const value of ['1','2, 1',' 1 , 3 ','2,1,2'])expect(negotiateSsrStreamVersion(value)).toBe(1);
 for(const value of [null,'','2','*','01','0','-1','1.0','1;q=1','1,','1,,2','1,foo','1,9007199254740992',Array(18).fill('1').join(','),'1'.repeat(257)])expect(negotiateSsrStreamVersion(value)).toBeUndefined();
 expect(SSR_STREAM_ACCEPT_HEADER).not.toBe(SSR_STREAM_VERSION_HEADER);
});
it('checks successful HTTP status, explicit version and content type before body access',()=>{
 const response=(ok:boolean,version:string|null,type:string|null)=>({ok,headers:{get:(name:string)=>name===SSR_STREAM_VERSION_HEADER?version:name==='content-type'?type:null},get body():never{throw Error('Body accessed');}});
 expect(()=>assertSsrStreamResponse(response(true,'1','Application/X-NDJSON; charset=utf-8'))).not.toThrow();
 for(const value of [response(false,'1','application/x-ndjson'),response(true,null,'application/x-ndjson'),response(true,'2','application/x-ndjson'),response(true,'1','text/html'),response(true,'1',null)])expect(()=>assertSsrStreamResponse(value)).toThrow(/HTTP/);
});

it('requires agreement on the selected codec before accepting response data',async()=>{
 const {negotiateSsrStreamCodec,SSR_STREAM_CODEC_HEADER,SSR_STREAM_HTML_REFERENCE_CODEC:codec}=await import('../src/engine');
 expect(negotiateSsrStreamCodec(null)).toBe('identity');expect(negotiateSsrStreamCodec('identity, '+codec)).toBe(codec);
 expect(negotiateSsrStreamCodec('future,identity')).toBe('identity');
 for(const value of ['future','identity,','*','Identity','identity;q=1'])expect(negotiateSsrStreamCodec(value)).toBeUndefined();
 const response={ok:true,headers:{get:(name:string)=>name===SSR_STREAM_CODEC_HEADER?codec:name===SSR_STREAM_VERSION_HEADER?'1':'application/x-ndjson'}};
 expect(()=>assertSsrStreamResponse(response)).toThrow(/codec/);
 expect(()=>assertSsrStreamResponse(response,{codec})).not.toThrow();
});
