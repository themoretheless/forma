import {resolve} from './language.js';
import {colorValue} from './color-values.js';

// Handoff describes what the canvas drew: the box the layout computed and the visual properties
// the compiler resolved, so a state expression, a component default or a design token comes out as
// the concrete value the designer is looking at. A value CSS cannot carry is left out rather than
// guessed at, because a partial export is still a correct one.
const slug=value=>String(value).toLowerCase().replace(/[^a-z0-9]+/,'-').replace(/^-|-$/g,'')||'control';
// `key` is the name the designer gave the control, so it is the name a handoff should use.
export function handoffName(node){
 const key=node.props?.key;
 const named=typeof key==='string'?key.match(/^[a-z_][a-z0-9_-]*$/i)?.[0]:null;
 return named?named.toLowerCase():slug(node.type);
}
const number=value=>typeof value==='number'&&Number.isFinite(value)?value:
 typeof value==='string'&&/^\d+(?:\.\d+)?(?:px)?$/.test(value.trim())?Number(value.trim().replace(/px$/,'')):null;
const length=value=>{const n=number(value);return n===null?null:`${Math.round(n*100)/100}px`;};
const value=(raw,state)=>resolve(raw,state??{});
// A length list is already the CSS order: padding comes out top-right-bottom-left and gap
// row-column, the same shapes the layout reads them in.
const list=(raw,state)=>{const parts=String(value(raw,state)??'').trim().split(/\s+/).filter(Boolean);
 const sizes=parts.map(part=>length(part));return sizes.every(part=>part)?sizes.join(' '):null;};
const color=(raw,state)=>{const resolved=value(raw,state);
 return typeof resolved==='string'||typeof resolved==='number'?colorValue(String(resolved)):null;};
// Markup spells a duration with its unit (`80ms`), which is also how CSS spells it.
const duration=raw=>{const resolved=typeof raw==='string'?raw.match(/^\d+(?:\.\d+)?(?:ms|s)$/)?.[0]:null;return resolved;};

// Declarations come out in one order whatever order the markup listed them in, so two exports of
// the same screen differ only where the screen actually differs.
export function controlDeclarations(node,bounds,state){
 const p=node.props??{},rules=[];
 const push=(name,text)=>{if(text)rules.push(`  ${name}: ${text};`);};
 push('width',length(bounds?.[2]??value(p.width,state)));
 push('height',length(bounds?.[3]??value(p.height,state)));
 push('background',color(p.background,state));
 push('color',color(p.color,state));
 push('font-size',length(value(p.fontSize??p['font.size'],state)));
 push('border-radius',length(value(p.radius,state)));
 const width=length(value(p.borderWidth,state)),line=color(p.borderColor,state);
 if(width&&line)push('border',`${width} solid ${line}`);
 else if(width)push('border-width',width);
 else if(line)push('border-color',line);
 push('padding',list(p.padding,state));
 push('gap',list(p.gap,state));
 if(value(p.clip,state)===true)push('overflow','hidden');
 push('transition-duration',duration(value(p.transitionDuration,state)));
 return rules;
}
// The states a control declares are the CSS pseudo-classes a browser applies, and each of them
// paints a different property: a focus ring is a border, a placeholder is a text color.
// A state that paints exactly what the control already paints asks the developer for a rule that
// changes nothing, so it is left out — a component whose hover matches its background is the norm.
export function controlStates(node,state){
 const p=node.props??{},rules=[];
 for(const [selector,name,property,base] of [[':hover','hoverBackground','background','background'],[':active','pressedBackground','background','background'],[':disabled','disabledBackground','background','background'],[':focus','focusBorderColor','border-color','borderColor'],['::placeholder','placeholderColor','color','color']]){
  const painted=color(p[name],state);
  if(painted&&painted!==color(p[base],state))rules.push({selector,property,value:painted});
 }
 return rules;
}
export function controlCss(node,bounds,state){
 const rules=controlDeclarations(node,bounds,state),selector=`.${handoffName(node)}`;
 const blocks=rules.length?[`${selector} {\n${rules.join('\n')}\n}`]:[];
 for(const rule of controlStates(node,state))blocks.push(`${selector}${rule.selector} {\n  ${rule.property}: ${rule.value};\n}`);
 return blocks.join('\n');
}
