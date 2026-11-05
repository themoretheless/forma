// Stateful exact pixel oracle for packed dense coverage and split paint passes.
const assert=require('node:assert/strict');
const {resolve}=require('node:path');
const {readFileSync,writeFileSync}=require('node:fs');
const {createHash}=require('node:crypto');
const [baselineDir,candidateDir,output]=process.argv.slice(2);
assert(baselineDir&&candidateDir&&output);
const modules=[baselineDir,candidateDir].map(dir=>require(resolve(dir,'forma.js')));
const sources=[
 "component App { Frame { width:200; height:140; radius:7; clip:false; background:#12345680; Button { x:31.3; y:23.7; width:68; height:49; } } }",
 "component App { Frame { width:200; height:140; radius:7; clip:true; background:#12345680; Button { width:200; height:140; } } }",
 "component App { Frame { width:64; height:48; radius:12; clip:true; padding:2; background:#12345680; Scroll { Button { width:160; height:120; } } } }",
];
const component="component Button { Rectangle { radius:4; background:Brush { color:#abcdef80; hover:#88776680; pressed:#55667780; transition:140ms; }; Border { width:1.3; background:#aabbcc90; } Reveal { color:#ffaa44; } Text { text:'你好 мир🙂'; fontSize:14; color:#ffffff; } ContentClip { x:-6; y:-8; width:85; height:74; radius:6; } ContentShape { points:'-18 -14 80 -14 80 66 -18 66'; color:#ff000080; } ContentClipEnd {} PointerArea { clicked -> events.clicked(); } } }";
const rows=[];
for(const [sourceId,source] of sources.entries()) {
 for(const scale of [.75,1,1.25,1.5,2]) for(const background of [false,true]) {
  const views=modules.map(api=>{const view=new api.Button();view.load_component(source,component);return view;});
  try {
   if(sourceId===2)assert(views[0].scrollable()&&views[1].scrollable());
   for(let state=0;state<7;state++) {
    for(const view of views) {
     if(state===1)view.pointer(40,40,0);
     if(state===2)view.pointer(55,42,0);
     if(state===3)view.pointer(55,42,1);
     if(state===4)view.pointer(55,42,2);
     if(state===5){view.pointer(230,180,3);view.scroll(17,23);}
     if(state===6)view.scroll(1000,1000);
     view.tick(state===6?1000:1000/60);
    }
    const frames=views.map(view=>Buffer.from(background?view.pixels(240,190,scale):view.content_pixels(240,190,scale)));
    assert.deepEqual(frames[1],frames[0],`source${sourceId},scale${scale},background${background},state${state}`);
    assert.deepEqual(Array.from(views[1].scroll_offset()),Array.from(views[0].scroll_offset()));
    rows.push({sourceId,scale,background,state,sha256:createHash('sha256').update(frames[1]).digest('hex')});
   }
  }finally{views.forEach(view=>view.free());}
 }
}
const hash=dir=>createHash('sha256').update(readFileSync(resolve(dir,'forma_bg.wasm'))).digest('hex');
writeFileSync(output,JSON.stringify({status:'passed',scope:'Exact Button dense pixels across animated fill/border/reveal, clipping/background, Scroll and DPI. No OS presentation claim.',baselineSha256:hash(baselineDir),candidateSha256:hash(candidateDir),rows},null,2)+'\n');
console.log(`${rows.length} stateful dense-paint frame comparisons passed`);
