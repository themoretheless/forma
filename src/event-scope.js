// A single owner for DOM listeners, including capture/passive options.
export function createEventScope(){
 const controller=new AbortController();
 return {
  listen(target,type,listener,options={}){target.addEventListener(type,listener,{...(typeof options==='boolean'?{capture:options}:options),signal:controller.signal});},
  dispose(){controller.abort();},
 };
}
