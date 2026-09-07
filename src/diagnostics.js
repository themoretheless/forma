export function similarName(name,names){
 const distance=(a,b)=>{let row=Array.from({length:b.length+1},(_,i)=>i);for(let i=0;i<a.length;i++){const next=[i+1];for(let j=0;j<b.length;j++)next.push(Math.min(next[j]+1,row[j+1]+1,row[j]+(a[i]===b[j]?0:1)));row=next;}return row[b.length];};
 const ranked=[...new Set(names)].map(value=>({value,score:distance(name.toLowerCase(),value.toLowerCase())})).sort((a,b)=>a.score-b.score||a.value.localeCompare(b.value));
 return ranked[0]?.score<=Math.max(1,Math.floor(name.length/3))&&ranked[0]?.score!==ranked[1]?.score?ranked[0].value:null;
}
export function sourceError(message,source,related){
 const error=new Error(message);error.diagnostic={message,source,related};return error;
}
