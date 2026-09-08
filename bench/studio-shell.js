import init,{Runtime,text_metrics} from '../public/vector-pkg/forma.js';
import {compileComponents} from '../src/components.js';
import {materializeTheme} from '../vector-ui/controls/tokens.js';
import {shellControl,shellScene} from '../src/studio-shell-controls.js';
import {createShellPainter} from '../src/studio-shell-painter.js';
const raw=import.meta.glob('../vector-ui/controls/{components,assets}/*',{query:'?raw',import:'default',eager:true});
const library=materializeTheme(Object.fromEntries(Object.entries(raw).map(([k,v])=>[k.slice('../vector-ui/controls/'.length),v])),'dark');
const output=document.querySelector('#result');
try{
 await init();
 const spec=shellControl({text:'Reveal',width:160,height:32});
 const out=compileComponents({...library,'Bench.ui':shellScene(spec)},'Bench.ui',{}, {measureText:text_metrics});
 const runtime=new Runtime(),frames=[];
 try{runtime.load_component(out.source,out.template);runtime.set_reduced_motion(true);for(let i=0;i<12;i++){runtime.reveal_pointer(i*16,8,true);runtime.tick(16);frames.push(runtime.content_pixels(320,64,2));}}finally{runtime.free();}
 const scratch=document.createElement('canvas'),context=scratch.getContext('2d'),legacy=document.querySelector('#legacy');
 const painter=createShellPainter(document),surface=painter.surface(document.querySelector('#direct'));
 const old=pixels=>{scratch.width=320;scratch.height=64;context.putImageData(new ImageData(new Uint8ClampedArray(pixels.buffer,pixels.byteOffset,pixels.byteLength),320,64),0,0);legacy.style.background=`url("${scratch.toDataURL()}") center/100% 100%`;};
 const next=pixels=>surface.paint(pixels,320,64);
 const iterations=160,samples={png:[],canvas:[]};
 for(let i=0;i<24;i++){old(frames[i%12]);next(frames[i%12]);}
 for(let round=0;round<6;round++){
  for(const name of round%2?['canvas','png']:['png','canvas']){
   await new Promise(requestAnimationFrame);const start=performance.now(),paint=name==='png'?old:next;
   for(let i=0;i<iterations;i++)paint(frames[i%12]);samples[name].push((performance.now()-start)/iterations);
  }
 }
 const median=a=>[...a].sort((a,b)=>a-b).slice(2,4).reduce((a,b)=>a+b)/2;
 const before=median(samples.png),after=median(samples.canvas),during=painter.snapshot();
 surface.destroy();painter.destroy();
 output.textContent=JSON.stringify({size:'160×32 at DPR 2',iterationsPerSample:iterations,samplesMsPerPaint:samples,medianMs:{png:before,canvas:after},speedup:before/after,canvasCounters:during,afterDispose:painter.snapshot(),notes:'Canvas bytes count backing-store RGBA only; JS/native/GPU process memory is not measured.'},null,2);
}catch(error){output.textContent=error.stack;}
