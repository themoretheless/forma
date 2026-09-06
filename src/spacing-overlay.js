const NS='http://www.w3.org/2000/svg';
export function createSpacingOverlay(canvas,getSelection){
  const svg=document.createElementNS(NS,'svg');
  svg.classList.add('spacing-overlay');svg.setAttribute('aria-hidden','true');document.body.append(svg);
  let frame;
  const add=(tag,attrs,text)=>{const el=document.createElementNS(NS,tag);for(const [k,v]of Object.entries(attrs))el.setAttribute(k,v);if(text!==undefined)el.textContent=text;svg.append(el);return el;};
  function measure(){
    const node=getSelection();if(!node)return null;
    const element=canvas.querySelector(`[data-start="${node.start}"]`);if(!element)return null;
    const rect=element.getBoundingClientRect(),css=getComputedStyle(element),spacing={};
    for(const kind of ['padding','margin'])for(const side of ['top','right','bottom','left']){
      const property=Object.hasOwn(node.props,`${kind}.${side}`)?`${kind}.${side}`:Object.hasOwn(node.props,kind)?kind:null;
      if(property)spacing[`${kind}.${side}`]={declared:node.props[property],pixels:parseFloat(css[kind+side[0].toUpperCase()+side.slice(1)])||0};
    }
    return {node,element,rect,css,spacing};
  }
  function draw(){
    svg.replaceChildren();const m=measure();svg.style.display=m?'block':'none';if(!m)return;
    const viewport=canvas.getBoundingClientRect();Object.assign(svg.style,{left:viewport.left+'px',top:viewport.top+'px',width:viewport.width+'px',height:viewport.height+'px'});
    svg.setAttribute('viewBox',`0 0 ${viewport.width} ${viewport.height}`);
    const x=m.rect.left-viewport.left,y=m.rect.top-viewport.top,w=m.rect.width,h=m.rect.height;
    const defs=add('defs',{});
    for(const [kind,color]of [['padding','#6de0bb'],['margin','#ffbb76']]){
      const marker=document.createElementNS(NS,'marker');for(const [k,v]of Object.entries({id:`spacing-${kind}`,viewBox:'0 0 6 6',refX:3,refY:3,markerWidth:5,markerHeight:5,orient:'auto-start-reverse'}))marker.setAttribute(k,v);
      const path=document.createElementNS(NS,'path');path.setAttribute('d','M 6 0 L 0 3 L 6 6');path.setAttribute('fill','none');path.setAttribute('stroke',color);marker.append(path);defs.append(marker);
      for(const side of ['top','right','bottom','left']){
        const spec=m.spacing[`${kind}.${side}`];if(!spec)continue;const amount=spec.pixels;
        const inner=kind==='padding',vertical=side==='top'||side==='bottom';
        const edge=vertical?(side==='top'?y:y+h):(side==='left'?x:x+w);
        const border=inner?parseFloat(m.css['border'+side[0].toUpperCase()+side.slice(1)+'Width'])||0:0;
        const direction=(side==='top'||side==='left'?1:-1)*(inner?1:-1);
        const a=edge+(inner?direction*border:0),b=a+direction*amount;
        const center=vertical?x+w*(inner?.5:.7):y+h*(inner?.5:.7);
        const x1=vertical?center:a,y1=vertical?a:center,x2=vertical?center:b,y2=vertical?b:center;
        if(amount!==0){
          add('rect',{x:vertical?x:Math.min(a,b),y:vertical?Math.min(a,b):y,width:vertical?w:Math.abs(amount),height:vertical?Math.abs(amount):h,fill:color,'fill-opacity':'.10'});
          add('line',{x1,y1,x2,y2,stroke:color,'stroke-width':1.5,'marker-start':`url(#spacing-${kind})`,'marker-end':`url(#spacing-${kind})`});
        }
        const label=`${kind} ${Math.round(amount*100)/100}`;
        const labelWidth=label.length*6.3+12;
        let lx=vertical?center+8:Math.min(a,b)-labelWidth/2+Math.abs(amount)/2;
        let ly=vertical?Math.min(a,b)+Math.abs(amount)/2-9:center-24;
        lx=Math.max(2,Math.min(viewport.width-labelWidth-2,lx));ly=Math.max(2,Math.min(viewport.height-20,ly));
        add('rect',{x:lx,y:ly,width:labelWidth,height:18,rx:4,fill:'#121c28',stroke:color,'stroke-opacity':'.55'});
        add('text',{x:lx+6,y:ly+12.5,fill:color,'font-size':10,'font-family':'monospace'},label);
      }
    }
    add('rect',{x,y,width:w,height:h,fill:'none',stroke:'#9ab3ff','stroke-width':1});
  }
  function update(){cancelAnimationFrame(frame);frame=requestAnimationFrame(draw);}
  const observer=new ResizeObserver(update);observer.observe(canvas);observer.observe(canvas.querySelector('#preview'));
  canvas.addEventListener('scroll',update);window.addEventListener('resize',update);
  return {update,read(){const m=measure();return m?{type:m.node.type,start:m.node.start,spacing:m.spacing}:null;}};
}
