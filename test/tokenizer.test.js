import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {lockedTokenizer} from '../scripts/tokenizer.mjs';

const entry=source=>`[[package]]\nname = "serde_json"\nversion = "1.0.0"\n\n[[package]]\nname = "themoretheless-tokenizer-forma"\nversion = "0.8.0"\n${source}dependencies = [\n "serde_json",\n]\n`;
const REV='6bc24a607a5e11c38da4ad4ba466a9a67173504d';
test('the tokenizer revision comes from the exact rev Cargo.lock pins',()=>{
 assert.deepEqual(lockedTokenizer(entry(`source = "git+https://github.com/themoretheless/tokenizer?rev=${REV}#${REV}"\n`)),
  {url:'https://github.com/themoretheless/tokenizer',rev:REV,version:'0.8.0'});
});
test('a path, registry, moving branch or mismatched tokenizer revision is not a reproducible pin',()=>{
 // A path dependency is what left CI unable to build: actions/checkout fetches only forma.
 for(const source of ['','source = "registry+https://github.com/rust-lang/crates.io-index"\n',`source = "git+https://github.com/themoretheless/tokenizer?branch=release#${REV}"\n`,`source = "git+https://github.com/themoretheless/tokenizer?rev=${REV}#d1c9ff7e7e69c903e02715645069e642872f14df"\n`])
  assert.throws(()=>lockedTokenizer(entry(source)),/exact tokenizer revision/);
});
test('the checked-in lock and Cargo manifest pin the same exact tokenizer revision',()=>{
 const manifest=readFileSync(new URL('../vector-ui/Cargo.toml',import.meta.url),'utf8');
 const rev=manifest.match(/themoretheless-tokenizer-forma = \{[^\n]*rev = "([0-9a-f]{40})"/)?.[1];
 assert.ok(rev,'Cargo.toml must declare an exact tokenizer revision');
 assert.equal(lockedTokenizer().rev,rev);
});
