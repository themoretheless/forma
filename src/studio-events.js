// The one channel Studio uses to tell its shells what changed. The editor publishes facts
// (mode, selection, problems, save status); a shell subscribes instead of reading them back out
// of DOM text or class lists, so a shell never has to know how the editor draws its own chrome.
export const studioEventTypes=Object.freeze({
 mode:'mode',             // {mode:'design'|'interact'}
 selection:'selection',   // {selected:boolean}
 problems:'problems',     // {count:number}
 save:'save',             // {status:'saved'|'changed'|'unsaved'|'damaged',text:string}
});

export function createStudioEvents(){
 const target=new EventTarget(),last=new Map();
 return {
  emit(type,detail){last.set(type,detail);target.dispatchEvent(new CustomEvent(type,{detail}));},
  // A late subscriber gets the last published value at once, like a MutationObserver callback that
  // ran immediately, so a shell mounted after the editor still starts in the right state.
  // Returns the unsubscribe function; a signal in options works too.
  on(type,listener,options={}){const handler=event=>listener(event.detail);target.addEventListener(type,handler,options);if(last.has(type))listener(last.get(type));return ()=>target.removeEventListener(type,handler,options);},
  current(type){return last.get(type);},
 };
}
