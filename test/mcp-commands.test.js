import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

// mcp/server.js and the bridge in src/main.js are two halves of one surface that never import each
// other: a tool the server registers but the IDE does not answer fails the agent with 'Unknown IDE
// command', and a command the IDE handles with no tool stays invisible to it. The names are read
// out of both files so neither half can drift on its own.
const root=new URL('..',import.meta.url);
const read=path=>readFileSync(new URL(path,root),'utf8');
const handled=text=>[...text.matchAll(/^\s*case '([a-z_]+)':/gm)].map(match=>match[1]);
const registered=text=>[...text.matchAll(/^ \['([a-z_]+)',/gm)].map(match=>match[1]);

test('the tool table and the IDE bridge name the same commands',()=>{
  const tools=registered(read('mcp/server.js'));
  const commands=handled(read('src/main.js'));
  assert.ok(tools.length>30,'the tool table no longer parses as expected');
  assert.deepEqual(tools.filter(name=>!commands.includes(name)),[],'registered with no handler');
  assert.deepEqual(commands.filter(name=>!tools.includes(name)),[],'handled with no tool');
  assert.equal(new Set(tools).size,tools.length,'a tool is registered twice');
});

test('every design state an agent can reach has a tool of its own',()=>{
  const tools=registered(read('mcp/server.js'));
  assert.deepEqual(tools.filter(name=>name.startsWith('design_state')),
    ['design_states_read','design_state_create','design_state_rename','design_state_delete','design_state_property']);
});
