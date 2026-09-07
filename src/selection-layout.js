// Pure geometry shared by dragging, alignment and distribution. Bounds are logical pixels.
export function selectionBounds(items){
 if(!items.length)return null;
 const x=Math.min(...items.map(b=>b[0])),y=Math.min(...items.map(b=>b[1]));
 return [x,y,Math.max(...items.map(b=>b[0]+b[2]))-x,Math.max(...items.map(b=>b[1]+b[3]))-y];
}
export function alignSelection(items,kind){
 const union=selectionBounds(items.map(i=>i.bounds));if(!union||items.length<2)throw Error('Выберите минимум два элемента');
 const axes={left:[0,0],center:[0,.5],right:[0,1],top:[1,0],middle:[1,.5],bottom:[1,1]};
 const rule=axes[kind];if(!rule)throw Error('Неизвестное выравнивание');
 const [axis,factor]=rule,target=union[axis]+union[axis+2]*factor;
 return items.map(i=>({start:i.start,dx:axis===0?target-i.bounds[0]-i.bounds[2]*factor:0,dy:axis===1?target-i.bounds[1]-i.bounds[3]*factor:0}));
}
export function distributeSelection(items,axis){
 if(items.length<3)throw Error('Выберите минимум три элемента');
 const sorted=[...items].sort((a,b)=>a.bounds[axis]-b.bounds[axis]);
 const first=sorted[0].bounds,last=sorted.at(-1).bounds;
 const gap=(last[axis]+last[axis+2]-first[axis]-sorted.reduce((sum,i)=>sum+i.bounds[axis+2],0))/(items.length-1);
 let position=first[axis];return sorted.map(i=>{const delta=position-i.bounds[axis];position+=i.bounds[axis+2]+gap;return {start:i.start,dx:axis===0?delta:0,dy:axis===1?delta:0};});
}
export function snapSelection(bounds,dx,dy,others,frame,threshold=6){
 const delta=[dx,dy],guides=[];
 for(const axis of [0,1]){
  const targets=[0,frame[axis]/2,frame[axis],...others.flatMap(b=>[b[axis],b[axis]+b[axis+2]/2,b[axis]+b[axis+2]])];
  let best=null;
  for(const target of targets)for(const fraction of [0,.5,1]){
   const adjustment=target-(bounds[axis]+delta[axis]+bounds[axis+2]*fraction);
   if(Math.abs(adjustment)<=threshold&&(!best||Math.abs(adjustment)<Math.abs(best.adjustment)))best={adjustment,target};
  }
  if(best){delta[axis]+=best.adjustment;guides.push({axis,position:best.target});}
 }
 return {dx:delta[0],dy:delta[1],guides};
}
export function intersects(a,b){return a[0]<=b[0]+b[2]&&a[0]+a[2]>=b[0]&&a[1]<=b[1]+b[3]&&a[1]+a[3]>=b[1];}
