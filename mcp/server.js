import {McpServer} from '@modelcontextprotocol/sdk/server/mcp.js';
import {StdioServerTransport} from '@modelcontextprotocol/sdk/server/stdio.js';
import {readFile} from 'node:fs/promises';
import {z} from 'zod';

const server=new McpServer({name:'forma-studio',version:'0.1.0'});
const path=z.string().min(1),value=z.union([z.string(),z.number(),z.boolean(),z.null()]);
async function request(command,args){
  const descriptor=JSON.parse(await readFile(new URL('../.forma/bridge.json',import.meta.url),'utf8'));
  if(descriptor.url!=='http://127.0.0.1:5173/__forma_mcp')throw Error('Invalid bridge address');
  const response=await fetch(descriptor.url,{method:'POST',headers:{'Content-Type':'application/json',Authorization:`Bearer ${descriptor.token}`},body:JSON.stringify({command,args}),signal:AbortSignal.timeout(10000)});
  const result=await response.json();if(!response.ok||result.error)throw Error(result.error||'Bridge failed');return result.result;
}
const definitions=[
 ['app_run','Launch current UI: vector renderer uses .ui source and components/Button.ui; HTML renderer uses WebView/SearchState contract',{},false],
 ['app_stop','Close the running native application',{},false],
 ['rust_run','Compile and execute current virtual Cargo project locally, offline. Runs user Rust code; inspect events_read for completion.',{},false],
 ['rust_stop','Stop active Cargo run',{},false],
 ['ide_status','Read active file, preview, diagnostics, selection and debugger status',{},true],
 ['project_read','Read the full current virtual project (also serves as export)',{},true],
 ['file_read','Read one project file',{path},true],
 ['file_write','Create/update a virtual project file. expectedContent prevents overwriting concurrent edits; omit only to create.',{path,content:z.string(),expectedContent:z.string().optional()},false],
 ['file_open','Open a file in the editor',{path},false],
 ['editor_fold','Collapse/expand a block at one-based line; omit line for all blocks in active file',{line:z.number().int().positive().optional(),collapsed:z.boolean()},false],
 ['project_replace','Replace project using a previously read full project as concurrency guard',{files:z.record(z.string(),z.string()),expectedFiles:z.record(z.string(),z.string())},false],
 ['preview_open','Set a component as preview entry',{path},false],
 ['preview_renderer','Select HTML compatibility preview or native-shared Rust/WASM vector primitives',{renderer:z.enum(['html','vector'])},false],
 ['vector_status','Read loaded vector source, template, actual CPU/WebGPU backend, geometry uploads, frame counters and fallback reason',{},true],
 ['preview_scenario','Choose an existing design scenario by name',{name:z.string()},false],
 ['preview_mode','Select design or interaction mode',{mode:z.enum(['design','interact'])},false],
 ['preview_viewport','Choose desktop/mobile and dark/light surface',{device:z.enum(['desktop','mobile']),theme:z.enum(['dark','light'])},false],
 ['component_tree','Read compiled nodes with source offsets and properties',{},true],
 ['designer_tree','Read the visible designer control-tree panel, source paths, offsets and selection',{},true],
 ['designer_tree_view','Show/hide the control-tree panel or switch between preview hierarchy and active-file structure',{visible:z.boolean().optional(),scope:z.enum(['designer','file']).optional()},false],
 ['designer_tree_select','Select a source control from designer_tree by id; opens its file without selecting text',{id:z.string()},false],
 ['designer_tree_fold','Collapse/expand a visible node from designer_tree',{id:z.string(),collapsed:z.boolean()},false],
 ['component_spacing','Read declared and computed margin/padding of selected design component',{},true],
 ['component_select','Select a component by its source start offset from component_tree',{start:z.number().int().nonnegative()},false],
 ['component_property','Set a simple existing literal property; rejects stale source and expression replacement',{start:z.number().int(),property:z.string(),value,expectedContent:z.string()},false],
 ['state_read','Read current preview state',{},true],
 ['state_set','Update existing top-level preview state fields',{values:z.record(z.string(),value)},false],
 ['event_dispatch','Dispatch a declared clicked event on a component; observes debugger pause',{start:z.number().int()},false],
 ['debug_break','Enable/disable pause before UI events',{enabled:z.boolean()},false],
 ['debug_continue','Continue the pending UI event (not a Rust debugger)',{},false],
 ['debug_reset','Restore current design scenario and clear pause',{},false],
 ['events_read','Read event log',{},true],
 ['events_clear','Clear UI event log',{},false],
 ['diagnostics_read','Read current compilation diagnostics',{},true],
];
for(const [name,description,inputSchema,readOnlyHint]of definitions){server.registerTool(name,{description,inputSchema,annotations:{readOnlyHint,destructiveHint:name==='project_replace',openWorldHint:false}},async args=>{try{return {content:[{type:'text',text:JSON.stringify(await request(name,args),null,2)}]};}catch(e){return {isError:true,content:[{type:'text',text:e.message}]};}});}
server.registerResource('language','forma://language',{description:'Implemented language and IDE limitations'},async uri=>({contents:[{uri:uri.href,text:await readFile(new URL('../README.md',import.meta.url),'utf8'),mimeType:'text/markdown'}]}));
await server.connect(new StdioServerTransport());
