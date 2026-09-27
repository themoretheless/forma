import {EditorView,ViewPlugin,Decoration,WidgetType} from '@codemirror/view';
import {syntaxTree} from '@codemirror/language';
import {colorValue,isColorProperty} from './color-values.js';

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
    if(!name||!isColorProperty(name))return;
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
