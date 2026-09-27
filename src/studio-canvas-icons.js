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
