import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,writeFileSync,mkdirSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {digestInputs} from '../scripts/runtime-digest.mjs';
test('build fingerprint detects edited, added and removed input files deterministically',t=>{
 const root=mkdtempSync(join(tmpdir(),'forma-digest-'));t.after(()=>rmSync(root,{recursive:true,force:true}));mkdirSync(join(root,'src'));
 writeFileSync(join(root,'src/a'),'one');writeFileSync(join(root,'lock'),'version');
 const initial=digestInputs(root,['src','lock']);assert.equal(digestInputs(root,['lock','src']),initial);
 writeFileSync(join(root,'src/a'),'two');assert.notEqual(digestInputs(root,['src','lock']),initial);
 writeFileSync(join(root,'src/a'),'one');assert.equal(digestInputs(root,['src','lock']),initial);
 writeFileSync(join(root,'src/b'),'new');assert.notEqual(digestInputs(root,['src','lock']),initial);
 rmSync(join(root,'src/b'));assert.equal(digestInputs(root,['src','lock']),initial);
});
