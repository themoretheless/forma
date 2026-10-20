export function layoutDistances(bounds,others,frame){
 const [x,y,w,h]=bounds,midY=y+h/2,midX=x+w/2,rightX=x+w,bottomY=y+h;
 const distances=[
 {side:'left',value:x,from:[0,midY],to:[x,midY],target:'Frame'},
 {side:'right',value:frame[0]-rightX,from:[rightX,midY],to:[frame[0],midY],target:'Frame'},
 {side:'top',value:y,from:[midX,0],to:[midX,y],target:'Frame'},
 {side:'bottom',value:frame[1]-bottomY,from:[midX,bottomY],to:[midX,frame[1]],target:'Frame'},
 ];
 for(let k=0;k<others.length;k++){
  const [a,b,c,d]=others[k],ar=a+c,bb=b+d;
  if((bottomY<bb?bottomY:bb)>(y>b?y:b)){
   if(ar<=x){const v=x-ar;if(v<distances[0].value)distances[0]={side:'left',value:v,from:[ar,midY],to:[x,midY],target:'сосед'};}
   if(a>=rightX){const v=a-rightX;if(v<distances[1].value)distances[1]={side:'right',value:v,from:[rightX,midY],to:[a,midY],target:'сосед'};}
  }
  if((rightX<ar?rightX:ar)>(x>a?x:a)){
   if(bb<=y){const v=y-bb;if(v<distances[2].value)distances[2]={side:'top',value:v,from:[midX,bb],to:[midX,y],target:'сосед'};}
   if(b>=bottomY){const v=b-bottomY;if(v<distances[3].value)distances[3]={side:'bottom',value:v,from:[midX,bottomY],to:[midX,b],target:'сосед'};}
  }
 }
 return distances.filter(d=>d.value>=0);
}
