// Deliberately bounded SVG subset. Never executes SVG, fetches URLs or embeds HTML.
// Unsupported SVG features are diagnostics, not silently missing artwork.
export function svgShapes(source,box,currentColor='#ffffff'){
  if(typeof source!=='string'||source.length>100_000)throw Error('SVG должен быть текстом до 100 КБ');
  source=source.replace(/<!--[^]*?-->/g,'').replace(/<\?xml[^]*?\?>/g,'');
  const tags=[...source.matchAll(/<([^>]+)>/g)];if(source.replace(/<[^>]+>/g,'').trim())throw Error('SVG: текстовые элементы пока не поддерживаются');
  const out=[];let view=null,root=false,closed=false;
  function number(v,fallback=0){const n=v===undefined?fallback:Number(v);if(!Number.isFinite(n)||Math.abs(n)>100_000)throw Error('SVG: неверная координата');return n;}
  function ink(v){if(v==='none')return null;if(v==='currentColor')v=currentColor;if(!/^#(?:[\da-f]{3}|[\da-f]{4}|[\da-f]{6}|[\da-f]{8})$/i.test(v))throw Error('SVG: поддерживаются hex и currentColor');return v;}
  function emit(points,c){if(!c)return;const [vx,vy,vw,vh]=view,[x,y,w,h]=box;const s=Math.min(w/vw,h/vh),ox=x+(w-vw*s)/2,oy=y+(h-vh*s)/2;out.push({color:c,points:points.map(([a,b])=>[ox+(a-vx)*s,oy+(b-vy)*s])});}
  function segment(a,b,width,c){const dx=b[0]-a[0],dy=b[1]-a[1],l=Math.hypot(dx,dy);if(!l)return;const x=-dy/l*width/2,y=dx/l*width/2;emit([[a[0]+x,a[1]+y],[b[0]+x,b[1]+y],[b[0]-x,b[1]-y],[a[0]-x,a[1]-y]],c);}
  for(const tag of tags){
    const raw=tag[1].trim();if(raw==='/svg'){closed=true;continue;}
    if(closed)throw Error('SVG: лишние элементы после корня');
    const name=raw.match(/^[a-zA-Z]+/)?.[0];if(!['svg','rect','circle','ellipse','line','polyline','polygon'].includes(name))throw Error(`SVG ${name??raw}: пока поддерживаются rect, circle, ellipse, line, polyline, polygon`);
    const attrs={};let tail=raw.slice(name.length).replace(/\/$/,'');
    const regex=/\s+([\w:-]+)\s*=\s*(['"])(.*?)\2/gy;let pos=0;
    while(pos<tail.length){if(!tail.slice(pos).trim())break;regex.lastIndex=pos;const m=regex.exec(tail);if(!m)throw Error('SVG: некорректные атрибуты');if(Object.hasOwn(attrs,m[1]))throw Error('SVG: повторный атрибут');attrs[m[1]]=m[3];pos=regex.lastIndex;}
    const allowed=name==='svg'?['xmlns','viewBox','width','height']:['fill','stroke','stroke-width',...(name==='rect'?['x','y','width','height']:name==='circle'?['cx','cy','r']:name==='ellipse'?['cx','cy','rx','ry']:name==='line'?['x1','y1','x2','y2']:['points'])];
    for(const k of Object.keys(attrs))if(!allowed.includes(k))throw Error(`SVG: атрибут ${k} пока не поддерживается`);
    if(name==='svg'){if(root)throw Error('SVG: один корень');root=true;view=attrs.viewBox?.trim().split(/[\s,]+/).map(v=>number(v))??[0,0,number(attrs.width),number(attrs.height)];if(view.length!==4||view[2]<=0||view[3]<=0)throw Error('SVG требует положительный viewBox');continue;}
    if(!root||!raw.endsWith('/'))throw Error('SVG: примитивы внутри svg должны быть самозакрывающимися');
    const fill=ink(attrs.fill??'#000000'),stroke=ink(attrs.stroke??'none'),sw=number(attrs['stroke-width'],1);if(sw<0)throw Error('SVG: отрицательная толщина');
    let points=[],close=true;
    if(name==='rect'){const x=number(attrs.x),y=number(attrs.y),w=number(attrs.width),h=number(attrs.height);if(w<0||h<0)throw Error('SVG: отрицательный размер');points=[[x,y],[x+w,y],[x+w,y+h],[x,y+h]];}
    else if(name==='circle'||name==='ellipse'){const x=number(attrs.cx),y=number(attrs.cy),rx=number(attrs.rx??attrs.r),ry=number(attrs.ry??attrs.r);if(rx<0||ry<0)throw Error('SVG: отрицательный радиус');points=Array.from({length:64},(_,i)=>[x+rx*Math.cos(i*Math.PI/32),y+ry*Math.sin(i*Math.PI/32)]);}
    else if(name==='line'){points=[[number(attrs.x1),number(attrs.y1)],[number(attrs.x2),number(attrs.y2)]];close=false;}
    else {const coords=attrs.points?.trim().split(/[\s,]+/).map(v=>number(v))??[];if(coords.length<4||coords.length%2||coords.length>2048)throw Error('SVG: неверный points');points=Array.from({length:coords.length/2},(_,i)=>coords.slice(i*2,i*2+2));close=name==='polygon';}
    if(name!=='line')emit(points,fill);
    if(stroke&&sw)for(let i=0;i<points.length-(close?0:1);i++)segment(points[i],points[(i+1)%points.length],sw,stroke);
  }
  if(!root||!closed)throw Error('SVG: незакрытый корень');if(out.length>2048)throw Error('SVG: слишком много примитивов');return out;
}
