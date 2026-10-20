// One color grammar serves three surfaces: the editor swatch, the inspector swatch
// and the picker. Keeping it here means they cannot disagree about what is a color.
const ARGB_RE=/^argb\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)$/;
const HEX_RE=/^#[\da-f]{3,8}$/i;
const CSS_COLOR_RE=/^(?:rgb|rgba|hsl|hsla|oklch|oklab)\([^{};]*\)$/i;
const COLOR_PROP_RE=/(?:^|\.)(?:background|color|fill|stroke|foreground)$/;
const HEX6_RE=/^#([\da-f]{6})$/i;
const HEX3_RE=/^#([\da-f])([\da-f])([\da-f])$/i;
export function colorValue(raw){
  let value=raw.trim();
  if((value.startsWith("'")&&value.endsWith("'"))||(value.startsWith('"')&&value.endsWith('"')))value=value.slice(1,-1);
  if(value.startsWith('#')){
    const len=value.length;
    return (len===4||len===5||len===7||len===9)&&HEX_RE.test(value)?value:null;
  }
  if(value.startsWith('argb(')){
    const argb=ARGB_RE.exec(value);
    if(!argb)return null;
    const a=Number(argb[1]),r=Number(argb[2]),g=Number(argb[3]),b=Number(argb[4]);
    if(a>255||r>255||g>255||b>255)return null;
    return `rgb(${r} ${g} ${b} / ${a/255})`;
  }
  if(CSS_COLOR_RE.test(value))return value;
  return null;
}
export function isColorProperty(name){return COLOR_PROP_RE.test(name);}
// <input type="color"> carries no alpha, so it is only offered for a plain six-digit
// value; expanding #rgb is lossless, dropping the alpha of #rrggbbaa is not.
export function pickerHex(raw){
  const value=colorValue(raw);
  if(!value||value[0]!=='#')return null;
  if(value.length===7){const six=HEX6_RE.exec(value);return six?`#${six[1].toLowerCase()}`:null;}
  if(value.length===4){const three=HEX3_RE.exec(value);return three?`#${(three[1]+three[1]+three[2]+three[2]+three[3]+three[3]).toLowerCase()}`:null;}
  return null;
}
