// Match structural braces, ignoring comments and quoted strings.
export function blockRanges(text) {
  const stack=[],ranges=[];let quote=null,lineComment=false,blockComment=false;
  for(let i=0;i<text.length;i++){
    const c=text[i],next=text[i+1];
    if(lineComment){if(c==='\n')lineComment=false;continue;}
    if(blockComment){if(c==='*'&&next==='/'){blockComment=false;i++;}continue;}
    if(quote){if(c==='\\'){i++;continue;}if(c===quote)quote=null;continue;}
    if(c==='/'&&next==='/'){lineComment=true;i++;continue;}
    if(c==='/'&&next==='*'){blockComment=true;i++;continue;}
    if(c==="'"||c==='"'){quote=c;continue;}
    if(c==='{')stack.push(i);
    else if(c==='}'&&stack.length){const from=stack.pop();if(text.slice(from,i).includes('\n'))ranges.push({from:from+1,to:i});}
  }
  return ranges.sort((a,b)=>a.from-b.from);
}
