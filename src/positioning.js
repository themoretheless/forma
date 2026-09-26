import {resolve} from './language.js';
import {length} from './component-layout.js';
// The scene lays a control out in the flow and only then moves the drawing to its own
// coordinates, so its siblings keep the slot and the size the flow gave it. A Grid or a Stack
// child never reads x/y at all, and the page container has no parent to measure against, so the
// markup can carry a coordinate that has to be ignored instead of rejected.
const flows=new Set(['Row','Column','Frame','Scroll']);
export function takesCoordinates(node,parent){
  if(!parent||!(node.props.x!==undefined||node.props.y!==undefined))return false;
  if(!flows.has(parent.type))return false;
  return !('columns'in parent.props||'rows'in parent.props);
}
// Both rectangles are measured in one coordinate space, so the offset the scene computes from the
// parent's outer corner comes out of CSS offsets in pixels without touching the layout: an offset
// element still reserves its flow slot, which is exactly what the scene does with it.
export function coordinateShift(node,parent,state,parentRect,rect){
  if(!takesCoordinates(node,parent))return null;
  const edges=[[parentRect.left,rect.left,'left',parentRect.width,'x'],[parentRect.top,rect.top,'top',parentRect.height,'y']];
  const shift={};
  for(const [origin,side,css,span,key]of edges){
    if(node.props[key]===undefined)continue;
    shift[css]=Math.round((origin+length(resolve(node.props[key],state),span,0,key)-side)*100)/100;
  }
  return shift;
}
