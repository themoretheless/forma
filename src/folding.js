// Match structural braces, ignoring comments and quoted strings.
export function blockRanges(text) {
  const stack=[],ranges=[];let quote=0,lineComment=false,blockComment=false,lastNewline=-1;
  const len=text.length;
  for(let i=0;i<len;i++){
    const c=text.charCodeAt(i);
    if(c===10)lastNewline=i;
    if(lineComment){if(c===10)lineComment=false;continue;}
    if(blockComment){if(c===42&&text.charCodeAt(i+1)===47){blockComment=false;i++;}continue;}
    if(quote){if(c===92){if(text.charCodeAt(i+1)===10)lastNewline=i+1;i++;continue;}if(c===quote)quote=0;continue;}
    if(c===47){const next=text.charCodeAt(i+1);if(next===47){lineComment=true;i++;continue;}if(next===42){blockComment=true;i++;continue;}}
    if(c===39||c===34){quote=c;continue;}
    if(c===123)stack.push(i);
    else if(c===125&&stack.length){const from=stack.pop();if(lastNewline>from)ranges.push({from:from+1,to:i});}
  }
  return ranges.sort((a,b)=>a.from-b.from);
}
