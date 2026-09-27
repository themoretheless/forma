// One color grammar serves three surfaces: the editor swatch, the inspector swatch
// and the picker. Keeping it here means they cannot disagree about what is a color.
export function colorValue(raw){
  let value=raw.trim();
  if((value.startsWith("'")&&value.endsWith("'"))||(value.startsWith('"')&&value.endsWith('"')))value=value.slice(1,-1);
  const argb=/^argb\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*\)$/.exec(value);
  if(argb){const [a,r,g,b]=argb.slice(1).map(Number);if([a,r,g,b].some(n=>n>255))return null;return `rgb(${r} ${g} ${b} / ${a/255})`;}
  if(/^#[\da-f]{3,8}$/i.test(value)&&[4,5,7,9].includes(value.length))return value;
  if(/^(?:rgb|rgba|hsl|hsla|oklch|oklab)\([^{};]*\)$/i.test(value))return value;
  return null;
}
export function isColorProperty(name){return /(?:^|\.)(?:background|color|fill|stroke|foreground)$/.test(name);}
// <input type="color"> carries no alpha, so it is only offered for a plain six-digit
// value; expanding #rgb is lossless, dropping the alpha of #rrggbbaa is not.
export function pickerHex(raw){
  const value=colorValue(raw);
  if(!value)return null;
  const six=/^#([\da-f]{6})$/i.exec(value);
  if(six)return `#${six[1].toLowerCase()}`;
  const three=/^#([\da-f])([\da-f])([\da-f])$/i.exec(value);
  return three?`#${three.slice(1).map(c=>c.repeat(2)).join('').toLowerCase()}`:null;
}
