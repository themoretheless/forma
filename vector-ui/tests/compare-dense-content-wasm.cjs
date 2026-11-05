// Exact public Button dense frames across content-region storage changes.
const assert=require('node:assert/strict');
const {resolve}=require('node:path');
const {readFileSync,writeFileSync}=require('node:fs');
const {createHash}=require('node:crypto');
const [baselineDir,candidateDir,output]=process.argv.slice(2);
assert(baselineDir&&candidateDir&&output);
const modules=[baselineDir,candidateDir].map(dir=>require(resolve(dir,'forma.js')));
const contents=[
 '',
 "Text { text:'你好 мир🙂'; fontSize:14; color:#ffffff; }",
 "ContentText { x:-20; y:-12; width:110; height:21; text:'Яg🙂'; fontSize:16; color:#ffccaa80; } ContentClip { x:-6; y:-8; width:85; height:74; radius:6; } ContentShape { points:'-18 -14 80 -14 80 66 -18 66'; color:#ff000080; } ContentClip { x:12.3; y:8.7; width:40; height:30; radius:3; } ContentShape { points:'-2 -2 60 0 45 44'; color:#00ff0080; } ContentClipEnd {} ContentClipEnd {}",
 "ContentClip { x:0; y:0; width:20; height:30; } ContentClipEnd {}",
];
const rows=[];
for(const [x,y] of [[31.3,23.7],[199.75,139.75],[500,300]]) {
 for(const [contentId,content] of contents.entries()) {
  const source=`component App { Frame { width:200; height:140; radius:7; clip:false; background:#12345680; Button { x:${x}; y:${y}; width:68; height:49; } } }`;
  const component=`component Button { Rectangle { radius:4; background:#abcdef80; ${content} } }`;
  const views=modules.map(api=>{const view=new api.Button();view.load_component(source,component);return view;});
  try {
   for(const scale of [.75,1,1.25,1.5,2]) for(const background of [false,true]) {
    const frames=views.map(view=>Buffer.from(background?view.pixels(240,190,scale):view.content_pixels(240,190,scale)));
    assert.deepEqual(frames[1],frames[0],`content${contentId},x${x},y${y},scale${scale},background${background}`);
    rows.push({contentId,x,y,scale,background,sha256:createHash('sha256').update(frames[1]).digest('hex')});
   }
  }finally{views.forEach(view=>view.free());}
 }
}
const hash=dir=>createHash('sha256').update(readFileSync(resolve(dir,'forma_bg.wasm'))).digest('hex');
writeFileSync(output,JSON.stringify({status:'passed',scope:'Public Button dense CPU exact frames; mixed fonts, nested clips, overflow, clip-only/empty, DPI and background. No OS presentation claim.',baselineSha256:hash(baselineDir),candidateSha256:hash(candidateDir),rows},null,2)+'\n');
console.log(`${rows.length} dense-content frame comparisons passed`);
