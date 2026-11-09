import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {squash} from './support/source-text.js';

// Patterns below contain no whitespace: the source is compared with all whitespace removed.
const read=path=>squash(readFileSync(new URL('../'+path,import.meta.url),'utf8'));

test('the primary app launch stays visible and owns Cmd/Ctrl+Enter',()=>{
 const shell=read('src/minimal-shell.js');
 assert.match(shell,/constappButton=\(\)=>\$\('#app-run'\)/);
 assert.match(shell,/el!==appButton\(\)/,'the project menu must not swallow the primary app button');
 assert.match(shell,/\(appButton\(\)\?\?run\)\.click\(\)/,'the shortcut launches the app, not preview interaction');
 assert.match(shell,/label:'Запуститьприложение',key:'⌘↵'/);
 assert.match(shell,/label:run\.title,key:app\?undefined:'⌘↵'/,'static preview keeps the shortcut only when native launch is unavailable');
});

test('preview interaction is visually and textually secondary to application launch',()=>{
 const main=read('src/main.js');
 assert.match(main,/<buttonid="run"title="Переключитьвстроенныйpreview/);
 assert.doesNotMatch(main,/<buttonid="run"class="accent"/);
 assert.match(main,/mode==='design'\?'Взаимодействие':'Дизайн'/);
 const bridge=read('src/dev-bridge.js');
 assert.match(bridge,/button\('▶Приложение','Запуститьтекущееприложение\(⌘↵\)'\)/);
 assert.match(bridge,/button\('Cargo·диагностика'/);
});
