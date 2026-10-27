import {compileComponents} from './components.js';
import {createVectorPreview} from './vector-preview.js';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
export {keyText} from './studio-window.js';

// Studio's own chrome is a Forma project: `studio-ui/ui/*.ui` composed from the shared control
// library, compiled by the same compiler and drawn by the same Rust/WASM runtime as a user's UI.
const raw={
 ...import.meta.glob('../vector-ui/controls/{components,assets}/*',{query:'?raw',import:'default',eager:true}),
 ...import.meta.glob('../studio-ui/{components,ui}/*.ui',{query:'?raw',import:'default',eager:true}),
};
export const shellFiles=materializeTheme(Object.fromEntries(Object.entries(raw).map(([path,value])=>[path.replace(/^\.\.\/(?:vector-ui\/controls|studio-ui)\//,''),value])),'dark');

// One floating piece of chrome. The host owns its data and passes it in as `state`; the markup
// reports clicks as `actions.<name>(args)` and edits of `<->` bindings as state paths.
export function createShellRegion({runtime,entry,className,onAction=()=>{},onBinding=()=>{},onKey}){
 const element=document.createElement('div');element.className='forma-region '+className;
 let signature='';
 const preview=createVectorPreview({runtime,onError:message=>console.error(`${entry}: ${message}`),
  onAction:(action,node)=>onAction(action.replace(/^actions\./,''),node?.eventArgs?.clicked??[]),
  onBindingChange:changes=>{for(const {path,value,node}of changes)onBinding(path.replace(/^state\./,''),value,node);},
  onControlKey:onKey});
 return {
  element,
  measure:(text,size)=>Math.ceil(preview.measureText(text,size)[0]),
  update(state){
   const next=JSON.stringify(state);
   if(next===signature)return;
   signature=next;
   const linked=compileComponents(shellFiles,entry,state,{measureText:preview.measureText});
   preview.render({container:element,...linked,designMode:false});
   const scene=preview.layoutSnapshot();
   if(scene)element.style.width=scene.renderWidth+'px';
  },
  focus:key=>preview.focusKey(key),
  destroy(){preview.destroy();element.remove();},
 };
}
