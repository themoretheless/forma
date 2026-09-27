import {contentProps, layoutProps, standard} from './components.js';

// A designer can only add a property the compiler accepts for this node type, so the
// candidate list is built from the compiler's own data: the type's content properties
// widened with the shared visual set. Dotted aliases stay out because `fontSize` is the
// spelling a designer picks, and a name with no start value is not offered at all.
const order=['width','height','minWidth','maxWidth','minHeight','maxHeight','padding','gap','radius','clip','background','color','fontSize','text','value','placeholder','multiline','disabled','x','y','columns','rows','source','key','cell','row','column','placeholderColor','hoverBackground','pressedBackground','disabledBackground','borderWidth','borderColor','focusBorderColor','transitionDuration'];
// Markup text, not a JS value: these are written straight into the block, so a quoted string
// stays quoted and a bare `#hex` stays bare the way hand-written markup spells it.
export const propertyStart={
  width:'240', height:'40', minWidth:'0', maxWidth:'1000', minHeight:'0', maxHeight:'1000',
  padding:'8', gap:'8', radius:'4', clip:'true', background:'#20272f', color:'#e8edf7',
  fontSize:'16', text:"'Текст'", value:"''", placeholder:"'Подсказка'", multiline:'false',
  disabled:'true', x:'0', y:'0', columns:'1', rows:'1', source:"'asset.png'", key:"'element'",
  cell:'[1, 1]', row:'1', column:'1', placeholderColor:'#98a4ba', hoverBackground:'#2b3441',
  pressedBackground:'#1a2028', disabledBackground:'#2a2f36', borderWidth:'1',
  borderColor:'#465775', focusBorderColor:'#9ab3ff', transitionDuration:'200'
};
export function addableProperties(type,declared=[]){
  const named=new Set([...(contentProps.get(type)??[]),...standard,...layoutProps]);
  const taken=new Set(declared);
  return [...named].filter(name=>!name.includes('.')&&!taken.has(name)&&name in propertyStart)
    .sort((a,b)=>(order.indexOf(a)-order.indexOf(b))||a.localeCompare(b));
}
