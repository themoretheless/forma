import {spawnSync} from 'node:child_process';
import {existsSync,readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {root} from './versions.mjs';

// Rust takes tokenizer-forma from the tokenizer `release` branch, and vector-ui/Cargo.lock pins the
// exact commit. npm cannot install a package from a subdirectory of a git repository, so the JS
// adapters and the language WASM come from a checkout of that same commit next to forma
// (package.json: file:../tokenizer/packages/forma). Everything here keeps the two in step.
export const tokenizerDir=resolve(root,'../tokenizer');
const PACKAGE=resolve(tokenizerDir,'packages/forma');
const SOURCES=['crates/tokenizer-forma','packages/forma'];

export function lockedTokenizer(lock=readFileSync(resolve(root,'vector-ui/Cargo.lock'),'utf8')){
  const block=lock.split('[[package]]').find(entry=>/^name = "themoretheless-tokenizer-forma"$/m.test(entry));
  const source=block?.match(/^source = "git\+([^?"]+)\?branch=release#([0-9a-f]{40})"$/m);
  const version=block?.match(/^version = "([^"]+)"$/m)?.[1];
  if(!source||!version)throw Error('vector-ui/Cargo.lock must pin themoretheless-tokenizer-forma to the tokenizer release branch');
  return {url:source[1],rev:source[2],version};
}

function run(command,args,options={}){
  const result=spawnSync(command,args,{encoding:'utf8',stdio:'pipe',...options});
  if(result.error||result.status!==0)throw Error(`${command} ${args.join(' ')} failed: ${result.error?.message??(result.stderr.trim()||result.status)}`);
  return result.stdout?.trim()??'';
}
const git=args=>run('git',args,{cwd:tokenizerDir});

// A checkout on another commit is fine while the two directories forma consumes match the lock;
// local work elsewhere in tokenizer is not forma's business.
export function checkTokenizer(locked=lockedTokenizer()){
  if(!existsSync(resolve(tokenizerDir,'.git')))throw Error(`No tokenizer checkout at ${tokenizerDir}; run node scripts/tokenizer.mjs checkout`);
  if(spawnSync('git',['cat-file','-e',`${locked.rev}^{commit}`],{cwd:tokenizerDir}).status!==0)
    throw Error(`../tokenizer does not have the locked release ${locked.rev.slice(0,8)}; run git -C ../tokenizer fetch origin release`);
  const changed=git(['status','--porcelain','--untracked-files=normal','--',...SOURCES]);
  const differs=spawnSync('git',['diff','--quiet',locked.rev,'--',...SOURCES],{cwd:tokenizerDir}).status!==0;
  if(changed||differs)throw Error(`../tokenizer ${SOURCES.join(' and ')} differ from the locked release ${locked.rev.slice(0,8)}; check it out, or move the lock with cargo update -p themoretheless-tokenizer-forma`);
  const version=JSON.parse(readFileSync(resolve(PACKAGE,'package.json'),'utf8')).version;
  if(version!==locked.version)throw Error(`packages/forma is ${version}, Cargo.lock pins ${locked.version}`);
  return locked;
}

export function checkoutTokenizer(locked=lockedTokenizer()){
  if(!existsSync(tokenizerDir)){
    run('git',['clone','--quiet','--no-checkout','--filter=blob:none',locked.url,tokenizerDir],{cwd:root});
    git(['checkout','--quiet','--detach',locked.rev]);
  }
  return checkTokenizer(locked);
}

export function installTokenizer(){
  run(process.platform==='win32'?'npm.cmd':'npm',['ci','--ignore-scripts','--no-audit','--no-fund'],{cwd:PACKAGE,shell:process.platform==='win32'});
}

// The tokenizer script reads its wasm from wasm/target, so a CARGO_TARGET_DIR set for forma must
// not leak into it, and it has no toolchain file of its own: build with the one forma pins.
export function buildTokenizerWasm(){
  const channel=readFileSync(resolve(root,'rust-toolchain.toml'),'utf8').match(/^channel = "([^"]+)"$/m)?.[1];
  if(!channel)throw Error('rust-toolchain.toml has no channel');
  const env={...process.env,RUSTUP_TOOLCHAIN:channel};delete env.CARGO_TARGET_DIR;
  run(process.execPath,['build-wasm.mjs'],{cwd:PACKAGE,env,stdio:'inherit'});
}

const invoked=process.argv[1]&&import.meta.url===new URL(`file://${resolve(process.argv[1])}`).href;
if(invoked){
  const commands={check:checkTokenizer,checkout:checkoutTokenizer,install:installTokenizer,build:buildTokenizerWasm};
  const names=process.argv.slice(2);
  if(!names.length||names.some(name=>!commands[name])){console.error(`usage: node scripts/tokenizer.mjs ${Object.keys(commands).join('|')} ...`);process.exit(2);}
  try{
    for(const name of names)commands[name]();
    const locked=lockedTokenizer();
    console.log(`tokenizer ${locked.version} @ ${locked.rev.slice(0,12)} (release): ${names.join(', ')} ok`);
  }catch(error){console.error(error.message);process.exit(1);}
}
