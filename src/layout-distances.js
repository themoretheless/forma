export function layoutDistances(bounds,others,frame){
 const [x,y,w,h]=bounds,overlap=(a,n,b,m)=>Math.min(a+n,b+m)>Math.max(a,b);
 const distances=[
 {side:'left',value:x,from:[0,y+h/2],to:[x,y+h/2],target:'Frame'},
 {side:'right',value:frame[0]-x-w,from:[x+w,y+h/2],to:[frame[0],y+h/2],target:'Frame'},
 {side:'top',value:y,from:[x+w/2,0],to:[x+w/2,y],target:'Frame'},
 {side:'bottom',value:frame[1]-y-h,from:[x+w/2,y+h],to:[x+w/2,frame[1]],target:'Frame'},
 ];
 for(const [a,b,c,d] of others){
  const possible=[];
  if(overlap(y,h,b,d)){
   if(a+c<=x)possible.push({side:'left',value:x-a-c,from:[a+c,y+h/2],to:[x,y+h/2]});
   if(a>=x+w)possible.push({side:'right',value:a-x-w,from:[x+w,y+h/2],to:[a,y+h/2]});
  }
  if(overlap(x,w,a,c)){
   if(b+d<=y)possible.push({side:'top',value:y-b-d,from:[x+w/2,b+d],to:[x+w/2,y]});
   if(b>=y+h)possible.push({side:'bottom',value:b-y-h,from:[x+w/2,y+h],to:[x+w/2,b]});
  }
  for(const p of possible){const i=distances.findIndex(d=>d.side===p.side);if(p.value<distances[i].value)distances[i]={...p,target:'сосед'};}
 }
 return distances.filter(d=>d.value>=0);
}
