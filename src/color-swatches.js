import {EditorView,ViewPlugin,Decoration,WidgetType} from '@codemirror/view';
import {syntaxTree} from '@codemirror/language';

export function colorValue(raw){
  let value=raw.trim();
  if((value.startsWith("'")&&value.endsWith("'"))||(value.startsWith('"')&&value.endsWith('"')))value=value.slice(1,-1);
  const argb=/^argb\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)$/.exec(value);
  if(argb){const [a,r,g,b]=argb.slice(1).map(Number);if([a,r,g,b].some(n=>n>255))return null;return `rgb(${r} ${g} ${b} / ${a/255})`;}
  if(/^#[\da-f]{3,8}$/i.test(value)&&[4,5,7,9].includes(value.length))return value;
  if(/^(?:rgb|rgba|hsl|hsla|oklch|oklab)\([^{};]*\)$/i.test(value))return value;
  return null;
}
class ColorWidget extends WidgetType {
  constructor(color,label){super();this.color=color;this.label=label;}
  eq(other){return this.color===other.color&&this.label===other.label;}
  toDOM(){
    const swatch=document.createElement('span');swatch.className='cm-color-swatch';
    swatch.title=this.label;swatch.setAttribute('aria-label','Цвет '+this.label);
    const fill=document.createElement('span');fill.style.backgroundColor=this.color;swatch.append(fill);return swatch;
  }
  ignoreEvent(){return true;}
}
function decorations(view){
  const widgets=[],seen=new Set();
  for(const range of view.visibleRanges)syntaxTree(view.state).iterate({from:range.from,to:range.to,enter(node){
    if(!['String','Color','Call'].includes(node.name))return;
    const raw=view.state.doc.sliceString(node.from,node.to),color=colorValue(raw);
    if(!color||!CSS.supports('color',color)||seen.has(node.from))return;
    // String content belonging to text/placeholder is not an actual color property.
    let parent=node.node.parent;
    while(parent&&parent.name!=='Property')parent=parent.parent;
    if(!parent)return;
    const property=parent.getChild('PropertyName');
    const name=property&&view.state.doc.sliceString(property.from,property.to);
    if(!name||!/(?:^|\.)(?:background|color|fill|stroke|foreground)$/.test(name))return;
    seen.add(node.from);widgets.push(Decoration.widget({widget:new ColorWidget(color,raw),side:-1}).range(node.from));
  }});
  return Decoration.set(widgets,true);
}
export const colorSwatches=[ViewPlugin.fromClass(class{
  constructor(view){this.decorations=decorations(view);}
  update(update){if(update.docChanged||update.viewportChanged||syntaxTree(update.startState)!==syntaxTree(update.state))this.decorations=decorations(update.view);}
},{decorations:v=>v.decorations}),EditorView.baseTheme({
  '.cm-color-swatch':{display:'inline-block',width:'11px',height:'11px',marginRight:'5px',verticalAlign:'-1px',border:'1px solid #91a0b580',borderRadius:'2px',overflow:'hidden',background:'conic-gradient(#ddd 25%, #888 0 50%, #ddd 0 75%, #888 0) 0 0 / 6px 6px'},
  '.cm-color-swatch > span':{display:'block',width:'100%',height:'100%'}
})];
