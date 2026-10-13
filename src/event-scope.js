// A single owner for DOM listeners and observers: one dispose() releases everything a surface
// attached, so shells and panels no longer keep their own AbortController plus observer arrays.
export function createEventScope(){
 const controller=new AbortController(),observers=new Set();
 const track=observer=>{observers.add(observer);return observer;};
 return {
  get signal(){return controller.signal;},
  listen(target,type,listener,options={}){target.addEventListener(type,listener,{...(typeof options==='boolean'?{capture:options}:options),signal:controller.signal});},
  // Observes mutations and runs the callback once immediately, so the initial state is mirrored
  // the same way later changes are.
  observe(target,callback,options,{immediate=true}={}){const observer=track(new MutationObserver(callback));observer.observe(target,options);if(immediate)callback([],observer);return observer;},
  resize(callback){return track(new ResizeObserver(callback));},
  intersect(callback,options){return track(new IntersectionObserver(callback,options));},
  dispose(){controller.abort();for(const observer of observers)observer.disconnect();observers.clear();},
 };
}
