export const canvasIcons=[
 ['#toggle-control-tree','folder','Дерево контролов'],
 ['#desktop','desktop','Desktop'],
 ['#mobile','mobile','Mobile'],
 ['#theme','theme','Тема'],
 ['[data-mode="design"]','cursor','Дизайн'],
 ['[data-mode="interact"]','play','Взаимодействие'],
 ['[data-zoom="out"]','minus','Уменьшить масштаб'],
 ['[data-zoom="in"]','plus','Увеличить масштаб'],
 ['[data-fit]','fit','Вписать'],
 ['[data-selection]','focus','К выделению'],
 ['[data-guides]','ruler','Размеры'],
 ['.layout-toggle','grid','Части и Grid'],
];
export const canvasIcon=element=>canvasIcons.find(([selector])=>element.matches(selector));

// Glyphs the editor's own chrome writes into button text: the run button, the project menu,
// the zoom pill's fit and the problems badge. The Rust shell's font has none of them, so a
// button led by one is painted with the matching icon and the rest of its text as the label.
export const glyphIcons=new Map([['▶','play'],['■','close'],['▾','chevron-down'],['⤢','fit'],['⚠','warning']]);
export function glyphIcon(text){
 const match=/^(\S)(?:\s+([^]*))?$/u.exec(String(text??'').replace(/\s+/g,' ').trim());
 return match&&glyphIcons.has(match[1])?{icon:glyphIcons.get(match[1]),text:match[2]??''}:null;
}
