import {test} from 'node:test';
import assert from 'node:assert/strict';
import {lockedTokenizer} from '../scripts/tokenizer.mjs';

const entry=source=>`[[package]]\nname = "serde_json"\nversion = "1.0.0"\n\n[[package]]\nname = "themoretheless-tokenizer-forma"\nversion = "0.8.0"\n${source}dependencies = [\n "serde_json",\n]\n`;
const REV='6bc24a607a5e11c38da4ad4ba466a9a67173504d';
test('the tokenizer revision comes from the release branch Cargo.lock pins',()=>{
 assert.deepEqual(lockedTokenizer(entry(`source = "git+https://github.com/themoretheless/tokenizer?branch=release#${REV}"\n`)),
  {url:'https://github.com/themoretheless/tokenizer',rev:REV,version:'0.8.0'});
});
test('a path, registry or other-branch tokenizer is not a reproducible pin',()=>{
 // A path dependency is what left CI unable to build: actions/checkout fetches only forma.
 for(const source of ['','source = "registry+https://github.com/rust-lang/crates.io-index"\n',`source = "git+https://github.com/themoretheless/tokenizer?branch=main#${REV}"\n`])
  assert.throws(()=>lockedTokenizer(entry(source)),/release branch/);
});
test('the checked-in lock pins a tokenizer release',()=>{
 assert.match(lockedTokenizer().rev,/^[0-9a-f]{40}$/);
});
