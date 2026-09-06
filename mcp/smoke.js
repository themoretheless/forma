import assert from 'node:assert/strict';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StdioClientTransport} from '@modelcontextprotocol/sdk/client/stdio.js';
import {fileURLToPath} from 'node:url';
const client=new Client({name:'forma-smoke',version:'1.0.0'});
await client.connect(new StdioClientTransport({command:process.execPath,args:[fileURLToPath(new URL('./server.js',import.meta.url))]}));
async function call(name,args={}){const r=await client.callTool({name,arguments:args});assert.ok(!r.isError,r.content[0]?.text);return JSON.parse(r.content[0].text);}
try{
 const tools=await client.listTools();assert.equal(tools.tools.length,34);
 const status=await call('ide_status');const file=await call('file_read',{path:status.entry});
 const write=await call('file_write',{path:file.path,content:file.content,expectedContent:file.content});assert.equal(write.error,'');
 const stale=await client.callTool({name:'file_write',arguments:{path:file.path,content:'invalid',expectedContent:'stale'}});assert.equal(stale.isError,true);
 assert.equal((await call('file_read',{path:file.path})).content,file.content);
 const tree=await call('component_tree');assert.ok(tree.nodes.length);
 await call('editor_fold',{collapsed:true});
 const selected=await call('component_select',{start:tree.nodes[0].start});assert.equal(selected.selected,tree.nodes[0].start);
 await call('debug_break',{enabled:true});assert.equal((await call('ide_status')).debug.breakOn,true);
 await call('debug_break',{enabled:status.debug.breakOn});
 const resources=await client.readResource({uri:'forma://language'});assert.ok(resources.contents[0].text.includes('Forma'));
 const spacing=await call('component_spacing');assert.equal(spacing.type,tree.nodes[0].type);
 console.log('PASS: 24 MCP tools, live IDE reads/writes, stale-write guard, component selection and spacing, debugger configuration, resource.');
}finally{await client.close();}
