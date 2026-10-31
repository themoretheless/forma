import {EditorView,ViewPlugin,Decoration} from '@codemirror/view';
import {rainbowBrackets} from './brackets.js';

const COMMENT='#66757d';
const STRING='#f5cf8d';
const NUMBER='#bda6ff';
const KEYWORD='#ff9d66';
const IDENT='#dfe7ec';
const MEMBER='#75e9d0';
const TYPE='#7ec3e8';
const PUNCT='#93a0a7';
const OPERATOR='#bda6ff';
const TEXT='#cfd8dd';
const ERROR='#ff657a';
const URL_PART='#7ee8a2';

const KIND_COLORS={
  comment:COMMENT,'line-comment':COMMENT,'block-comment':COMMENT,'doc-comment':COMMENT,
  string:STRING,'basic-string':STRING,'literal-string':STRING,'multi-line-basic-string':STRING,'multi-line-literal-string':STRING,'single-quoted-string':STRING,'single-quoted-scalar':STRING,'double-quoted-scalar':STRING,'code-block-line':STRING,'code-span':STRING,color:STRING,'quoted-value':STRING,'escaped-char':STRING,'escape-sequence':STRING,'quoted-field':STRING,'escaped-quote':STRING,'heredoc-open':STRING,'heredoc-body':STRING,'heredoc-close':STRING,escape:STRING,character:STRING,'string-escape':STRING,'byte-value':STRING,'style-content':STRING,'style-rule':STRING,'quoted-param-value':STRING,
  number:NUMBER,integer:NUMBER,float:NUMBER,'hex-integer':NUMBER,'octal-integer':NUMBER,'binary-integer':NUMBER,inf:NUMBER,nan:NUMBER,percentage:NUMBER,'hex-number':NUMBER,infinity:NUMBER,'integer-value':NUMBER,'float-value':NUMBER,'decimal-value':NUMBER,'integer-field':NUMBER,'decimal-field':NUMBER,bigint:NUMBER,decimal:NUMBER,'radix-integer':NUMBER,ratio:NUMBER,'special-number':NUMBER,'cue-index':NUMBER,'time-hour':NUMBER,'time-minute':NUMBER,'time-second':NUMBER,millisecond:NUMBER,
  keyword:KEYWORD,boolean:KEYWORD,true:KEYWORD,false:KEYWORD,null:KEYWORD,'document-start':KEYWORD,'document-end':KEYWORD,directive:KEYWORD,'heading-marker':KEYWORD,'setext-underline':KEYWORD,'list-marker':KEYWORD,strong:KEYWORD,emphasis:KEYWORD,strikethrough:KEYWORD,'image-marker':KEYWORD,'universal-selector':KEYWORD,'nesting-selector':KEYWORD,important:KEYWORD,'at-rule':KEYWORD,'flag-key':KEYWORD,'boolean-value':KEYWORD,'null-value':KEYWORD,'boolean-field':KEYWORD,'namespaced-keyword':KEYWORD,'boolean-literal':KEYWORD,'nil-literal':KEYWORD,'byte-tag':KEYWORD,tag:KEYWORD,'instant-tag':KEYWORD,'uuid-tag':KEYWORD,discard:KEYWORD,signature:KEYWORD,'block-marker':KEYWORD,'markup-name':KEYWORD,'timing-tag':KEYWORD,'emphasis-tag':KEYWORD,'structure-marker':KEYWORD,
  identifier:IDENT,variable:IDENT,parameter:IDENT,symbol:IDENT,'namespaced-symbol':IDENT,
  property:MEMBER,attribute:MEMBER,function:MEMBER,'bare-key':MEMBER,'key-indicator':MEMBER,anchor:MEMBER,alias:MEMBER,'link-label':MEMBER,'html-block':MEMBER,'html-inline':MEMBER,'link-text':MEMBER,'footnote-ref':MEMBER,'footnote-definition-label':MEMBER,'class-selector':MEMBER,'attribute-selector':MEMBER,'quoted-key':MEMBER,'single-quoted-key':MEMBER,'unquoted-key':MEMBER,'numeric-key':MEMBER,'keyword-key':MEMBER,key:MEMBER,'attribute-name':MEMBER,'block-label':MEMBER,'namespaced-map-key':MEMBER,'namespaced-map-prefix':MEMBER,'header-name':MEMBER,'style-reference':MEMBER,'region-property':MEMBER,'region-reference':MEMBER,'cue-identifier':MEMBER,'cue-id':MEMBER,'setting-name':MEMBER,'markup-value':MEMBER,'inline-tag':MEMBER,'voice-tag':MEMBER,'class-tag':MEMBER,'property-name':MEMBER,'parameter-name':MEMBER,
  type:TYPE,class:TYPE,'offset-date-time':TYPE,'local-date-time':TYPE,'local-date':TYPE,'local-time':TYPE,'heading-text':TYPE,'fence-info':TYPE,'type-selector':TYPE,'id-selector':TYPE,'section-name':TYPE,'header-field':TYPE,'block-type':TYPE,'instant-value':TYPE,'uuid-value':TYPE,'cue-start':TYPE,'cue-end':TYPE,'component-name':TYPE,'date-value':TYPE,'date-time-value':TYPE,'duration-value':TYPE,'period-value':TYPE,
  punctuation:PUNCT,comma:PUNCT,colon:PUNCT,'left-brace':PUNCT,'right-brace':PUNCT,'left-bracket':PUNCT,'right-bracket':PUNCT,delimiter:PUNCT,equals:PUNCT,dot:PUNCT,'block-entry':PUNCT,'value-indicator':PUNCT,'flow-sequence-start':PUNCT,'flow-sequence-end':PUNCT,'flow-mapping-start':PUNCT,'flow-mapping-end':PUNCT,'flow-entry':PUNCT,'thematic-break':PUNCT,'code-fence-marker':PUNCT,'table-delimiter':PUNCT,'table-pipe':PUNCT,'front-matter-delimiter':PUNCT,semicolon:PUNCT,'left-paren':PUNCT,'right-paren':PUNCT,'trailing-comma':PUNCT,separator:PUNCT,'section-marker':PUNCT,quote:PUNCT,'list-open':PUNCT,'list-close':PUNCT,'vector-open':PUNCT,'vector-close':PUNCT,'map-open':PUNCT,'map-close':PUNCT,'set-open':PUNCT,'time-separator':PUNCT,'millisecond-separator':PUNCT,'timing-arrow':PUNCT,'header-separator':PUNCT,'region-separator':PUNCT,'setting-separator':PUNCT,'markup-punctuation':PUNCT,'closing-tag':PUNCT,'value-delimiter':PUNCT,'value-list-delimiter':PUNCT,'parameter-delimiter':PUNCT,'parameter-assignment':PUNCT,
  operator:OPERATOR,'block-scalar-header':OPERATOR,'blockquote-marker':OPERATOR,'pseudo-class':OPERATOR,'pseudo-element':OPERATOR,unit:OPERATOR,interpolation:OPERATOR,'cue-timing':OPERATOR,
  text:TEXT,'plain-scalar':TEXT,'table-cell':TEXT,'at-rule-prelude-text':TEXT,value:TEXT,'bare-value':TEXT,field:TEXT,'tagged-value':TEXT,'header-value':TEXT,'region-value':TEXT,'cue-text':TEXT,'cue-text-continuation':TEXT,'setting-value':TEXT,'text-value':TEXT,'recurrence-value':TEXT,'bare-param-value':TEXT,
  invalid:ERROR,error:ERROR,
  'link-destination':URL_PART,autolink:URL_PART,'email-autolink':URL_PART,'uri-value':URL_PART,
};

const INVISIBLE_KINDS=new Set([
  'whitespace','bom','newline','line-break','hard-break','record-break','byte-order-mark','padding','fold-marker','identifier','text',
]);

const FALLBACK_PALETTE=['#7ec3e8','#d8b4fe','#86d7a6','#e8c07e','#e88ea0','#8fd8d2','#c7d87e'];

function kindColor(kind){
  if(INVISIBLE_KINDS.has(kind))return null;
  const exact=KIND_COLORS[kind];
  if(exact)return exact;
  if(kind.startsWith('u-'))return kind==='u-sep'?PUNCT:URL_PART;
  if(kind.includes('comment'))return COMMENT;
  if(kind.includes('string'))return STRING;
  if(kind.includes('number')||kind.includes('int')||kind.includes('float'))return NUMBER;
  if(kind.includes('keyword'))return KEYWORD;
  if(kind.includes('type')||kind.includes('class'))return TYPE;
  if(kind.includes('func')||kind.includes('prop')||kind.includes('key')||kind.includes('attr'))return MEMBER;
  if(kind.includes('error')||kind.includes('invalid'))return ERROR;
  let hash=0;
  for(let i=0;i<kind.length;i++)hash=(hash*31+kind.charCodeAt(i))>>>0;
  return FALLBACK_PALETTE[hash%FALLBACK_PALETTE.length];
}

const decoCache=new Map();
function markForKind(kind){
  let deco=decoCache.get(kind);
  if(deco!==undefined)return deco;
  const color=kindColor(kind);
  if(!color){decoCache.set(kind,null);return null;}
  const isComment=color===COMMENT;
  deco=Decoration.mark({attributes:{style:isComment?`color:${color};font-style:italic`:`color:${color}`}});
  decoCache.set(kind,deco);
  return deco;
}

const EXT_TO_LANG={
  rs:{lang:'rust',mode:'',label:'Rust'},
  toml:{lang:'toml',mode:'',label:'TOML'},
  js:{lang:'javascript',mode:'',label:'JavaScript'},
  mjs:{lang:'javascript',mode:'',label:'JavaScript'},
  cjs:{lang:'javascript',mode:'',label:'JavaScript'},
  jsx:{lang:'javascript',mode:'',label:'JSX'},
  ts:{lang:'typescript',mode:'',label:'TypeScript'},
  mts:{lang:'typescript',mode:'',label:'TypeScript'},
  cts:{lang:'typescript',mode:'',label:'TypeScript'},
  tsx:{lang:'typescript',mode:'',label:'TSX'},
  json:{lang:'json',mode:'jsonc',label:'JSON'},
  jsonc:{lang:'json',mode:'jsonc',label:'JSONC'},
  md:{lang:'markdown',mode:'',label:'Markdown'},
  markdown:{lang:'markdown',mode:'',label:'Markdown'},
  css:{lang:'css',mode:'',label:'CSS'},
  html:{lang:'html',mode:'',label:'HTML'},
  htm:{lang:'html',mode:'',label:'HTML'},
  xml:{lang:'xml',mode:'',label:'XML'},
  svg:{lang:'xml',mode:'',label:'SVG'},
  yaml:{lang:'yaml',mode:'',label:'YAML'},
  yml:{lang:'yaml',mode:'',label:'YAML'},
  sh:{lang:'bash',mode:'',label:'Shell'},
  bash:{lang:'bash',mode:'',label:'Bash'},
  zsh:{lang:'bash',mode:'',label:'Zsh'},
  py:{lang:'python',mode:'',label:'Python'},
  sql:{lang:'sql',mode:'',label:'SQL'},
  c:{lang:'c',mode:'',label:'C'},
  h:{lang:'c',mode:'',label:'C Header'},
  cpp:{lang:'cpp',mode:'',label:'C++'},
  cc:{lang:'cpp',mode:'',label:'C++'},
  hpp:{lang:'cpp',mode:'',label:'C++'},
  scad:{lang:'cpp',mode:'',label:'OpenSCAD'},
  go:{lang:'go',mode:'',label:'Go'},
  java:{lang:'java',mode:'',label:'Java'},
  kt:{lang:'kotlin',mode:'',label:'Kotlin'},
  swift:{lang:'swift',mode:'',label:'Swift'},
  rb:{lang:'ruby',mode:'',label:'Ruby'},
  php:{lang:'php',mode:'',label:'PHP'},
  cs:{lang:'csharp',mode:'',label:'C#'},
  lua:{lang:'lua',mode:'',label:'Lua'},
  zig:{lang:'zig',mode:'',label:'Zig'},
  ini:{lang:'ini',mode:'',label:'INI'},
  hcl:{lang:'hcl',mode:'',label:'HCL'},
  tf:{lang:'hcl',mode:'',label:'Terraform'},
  graphql:{lang:'graphql',mode:'',label:'GraphQL'},
  gql:{lang:'graphql',mode:'',label:'GraphQL'},
  rush:{lang:'rush',mode:'',label:'Rush'},
};

export function detectTokenizerLanguage(path=''){
  const p=String(path).toLowerCase();
  if(p.endsWith('.ui'))return {lang:'forma',mode:'',label:'Forma UI'};
  const base=p.split('/').at(-1)??'';
  if(base==='cargo.lock')return {lang:'toml',mode:'',label:'Cargo.lock'};
  const ext=base.includes('.')?base.split('.').at(-1):'';
  return EXT_TO_LANG[ext]??null;
}

let wasmModule=null;
let wasmPromise=null;
const activeViews=new Set();

function ensureTokenizerWasm(){
  if(wasmModule)return Promise.resolve(wasmModule);
  if(wasmPromise)return wasmPromise;
  const url=new URL('/__forma_tokenizer/tokenizer_wasm.js',window.location.origin).href;
  wasmPromise=import(/* @vite-ignore */ url).then(async mod=>{
    await mod.default(new URL('/__forma_tokenizer/tokenizer_wasm_bg.wasm',window.location.origin));
    wasmModule=mod;
    for(const view of activeViews){
      try{view.dispatch({});}catch{}
    }
    return mod;
  }).catch(()=>null);
  return wasmPromise;
}

// Build a mapping from UTF-8 byte offset -> UTF-16 code unit index for a string.
function buildUtf8ToUtf16Map(str){
  let asciiOnly=true;
  for(let i=0;i<str.length;i++){
    if(str.charCodeAt(i)>0x7f){asciiOnly=false;break;}
  }
  if(asciiOnly)return null; // 1:1 fast path
  // Worst case UTF-8 length is str.length * 3
  const map=new Uint32Array(str.length*3+1);
  let bytePos=0;
  for(let i=0;i<str.length;i++){
    const code=str.charCodeAt(i);
    if(code<=0x7f){
      map[bytePos++]=i;
    }else if(code<=0x7ff){
      map[bytePos]=i;map[bytePos+1]=i;bytePos+=2;
    }else if(code>=0xd800&&code<=0xdbff&&i+1<str.length){
      const next=str.charCodeAt(i+1);
      if(next>=0xdc00&&next<=0xdfff){
        map[bytePos]=i;map[bytePos+1]=i;map[bytePos+2]=i;map[bytePos+3]=i;
        bytePos+=4;i++;
        continue;
      }
      map[bytePos]=i;map[bytePos+1]=i;map[bytePos+2]=i;bytePos+=3;
    }else{
      map[bytePos]=i;map[bytePos+1]=i;map[bytePos+2]=i;bytePos+=3;
    }
  }
  map[bytePos]=str.length;
  return map;
}

function buildDecorations(view,info){
  if(!wasmModule||!info)return Decoration.none;
  const text=view.state.doc.toString();
  if(!text||text.length>250_000)return Decoration.none;
  let parsed=null;
  try{
    const raw=wasmModule.tokenize(info.lang,text,info.mode||'','semantic');
    parsed=JSON.parse(raw);
    if(!parsed?.tokens?.length){
      const rawSyntax=wasmModule.tokenize(info.lang,text,info.mode||'','syntax');
      parsed=JSON.parse(rawSyntax);
    }
  }catch{return Decoration.none;}
  const tokens=parsed?.tokens;
  if(!Array.isArray(tokens)||!tokens.length)return Decoration.none;
  const byteMap=buildUtf8ToUtf16Map(text);
  const ranges=[];
  const len=text.length;
  for(let i=0;i<tokens.length;i++){
    const tok=tokens[i];
    const deco=markForKind(tok.kind);
    if(!deco)continue;
    const from=byteMap?byteMap[tok.start]:tok.start;
    const to=byteMap?byteMap[tok.end]:tok.end;
    if(from>=0&&to>from&&to<=len){
      ranges.push(deco.range(from,to));
    }
  }
  return Decoration.set(ranges,true);
}

export function tokenizerHighlight(path){
  const info=detectTokenizerLanguage(path);
  if(!info||info.lang==='forma'||typeof window==='undefined')return [];
  ensureTokenizerWasm();
  const plugin=ViewPlugin.fromClass(class{
    constructor(view){
      this.view=view;
      activeViews.add(view);
      this.decorations=buildDecorations(view,info);
    }
    update(update){
      if(update.docChanged||!this.decorations.size&&wasmModule){
        this.decorations=buildDecorations(update.view,info);
      }
    }
    destroy(){
      activeViews.delete(this.view);
    }
  },{decorations:v=>v.decorations});
  return [plugin,rainbowBrackets];
}
