const assert = require('node:assert/strict');
const {resolve} = require('node:path');
const {readFileSync,writeFileSync} = require('node:fs');
const {createHash} = require('node:crypto');
const [baselineDir,candidateDir,output] = process.argv.slice(2);
assert(baselineDir && candidateDir && output);
const modules = [baselineDir,candidateDir].map(dir=>require(resolve(dir,'forma.js')));
const rows=[];
for(const [x,y] of [[500,300],[300,0],[0,300],[0,0],[199.75,99.75]]) {
 for(const content of ['empty','shape','text']) {
  const dx=10-x,dy=10-y;
  const child=content==='shape'?`ContentShape { points:'${dx} ${dy} ${dx+20} ${dy} ${dx+20} ${dy+20} ${dx} ${dy+20}'; color:#ff0000; }`:
   content==='text'?`ContentText { x:${dx}; y:${dy}; width:100; height:30; text:'Я🙂'; fontSize:20; color:#ffffff; }`:'';
  const component=`component Button { Rectangle { background:#00000000; ${child} } }`;
  const source=`component App { Frame { width:200; height:100; padding:0; clip:false; background:#00000000; Button { x:${x}; y:${y}; width:40; height:30; } Button { x:140; y:60; width:40; height:30; } } }`;
  const views=modules.map(api=>{const view=new api.Runtime();view.load_component(source,component);assert.equal(view.control_count(),2);return view;});
  try {
   for(const scale of [.75,1,1.25,2]) {
    const width=Math.ceil(200*scale),height=Math.ceil(100*scale);
    const frames=views.map(view=>Buffer.from(view.content_pixels(width,height,scale)));
    assert.deepEqual(frames[1],frames[0],`${content}: ${x},${y},scale${scale}`);
    if(content!=='empty') assert(frames[1].some((v,i)=>i%4===3&&v>0),'overflow fixture must paint visible pixels');
    else assert(!frames[1].some(v=>v!==0),'transparent fixture must remain empty');
    rows.push({content,x,y,scale,sha256:createHash('sha256').update(frames[1]).digest('hex')});
   }
  } finally {views.forEach(view=>view.free());}
 }
}
const hash=dir=>createHash('sha256').update(readFileSync(resolve(dir,'forma_bg.wasm'))).digest('hex');
writeFileSync(output,JSON.stringify({status:'passed',scope:'Exact multi-control CPU content frames, offscreen parent with visible text/shape overflow, transparent empty cases and DPI cache-key changes; no GPU/OS presentation claim',baselineSha256:hash(baselineDir),candidateSha256:hash(candidateDir),rows},null,2)+'\n');
console.log(`${rows.length} sparse-cache frame comparisons passed`);
