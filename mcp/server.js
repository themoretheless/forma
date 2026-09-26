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
 ['preview_scenario','Choose an existing design state by name; null shows the base values of the design file',{name:z.string().nullable()},false],
 ['design_states_read','Read the design file the states panel authors: the base block and every state, each entry with its type, key and the markup text of its values',{},true],
 ['design_state_create','Add an empty design state block. Returns the file text to use as expectedContent next.',{name:z.string(),expectedContent:z.string()},false],
 ['design_state_rename','Rename a design state block',{name:z.string(),newName:z.string(),expectedContent:z.string()},false],
 ['design_state_delete','Delete a design state block together with its overrides',{name:z.string(),expectedContent:z.string()},false],
 ['design_state_property','Override one property of one control in a state (state null means the base block), creating that entry when the block does not patch the key yet. Value is markup text as the file spells it: "Текст", 240, true, #e8edf7.',{state:z.string().nullable().optional(),type:z.string(),key:z.string(),property:z.string(),value:z.string(),expectedContent:z.string()},false],
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
 ['component_controls','List the controls the palette can insert, each with the markup it inserts',{},true],
 ['component_insert','Insert one control next to a node from component_tree; side is the slot: inside (a container), after (a sibling) or auto (follow the selection). Returns the node start the preview selected and the file text for the next expectedContent, with changed telling whether the file moved.',{start:z.number().int(),markup:z.string(),side:z.enum(['inside','after','auto']).optional(),expectedContent:z.string()},false],
 ['component_move','Move one control to another slot of the same parent, as the tree drag does. Reports changed:false with the unchanged text when the node already holds that slot.',{start:z.number().int(),target:z.number().int(),side:z.enum(['before','after']),expectedContent:z.string()},false],
 ['state_read','Read current preview state',{},true],
 ['state_set','Update existing top-level preview state fields',{values:z.record(z.string(),value)},false],
 ['event_dispatch','Dispatch a declared clicked event on a component; observes debugger pause',{start:z.number().int()},false],
 ['debug_break','Enable/disable pause before UI events',{enabled:z.boolean()},false],
 ['debug_continue','Continue the pending UI event (not a Rust debugger)',{},false],
 ['debug_reset','Restore the preview state and clear pause',{},false],
 ['events_read','Read event log',{},true],
 ['events_clear','Clear UI event log',{},false],
 ['diagnostics_read','Read current compilation diagnostics',{},true],
];
for(const [name,description,inputSchema,readOnlyHint]of definitions){server.registerTool(name,{description,inputSchema,annotations:{readOnlyHint,destructiveHint:name==='project_replace',openWorldHint:false}},async args=>{try{return {content:[{type:'text',text:JSON.stringify(await request(name,args),null,2)}]};}catch(e){return {isError:true,content:[{type:'text',text:e.message}]};}});}
server.registerResource('language','forma://language',{description:'Implemented language and IDE limitations'},async uri=>({contents:[{uri:uri.href,text:await readFile(new URL('../README.md',import.meta.url),'utf8'),mimeType:'text/markdown'}]}));
await server.connect(new StdioServerTransport());
