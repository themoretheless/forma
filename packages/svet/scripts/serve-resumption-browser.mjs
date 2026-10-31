import {resolve} from 'node:path';
import {JSDOM} from 'jsdom';
import {createServer} from 'vite';
import {getGraphTelemetry, h, onCleanup, renderToString, signal} from '../dist/package/index.js';

const dom = new JSDOM('<!doctype html><html><body></body></html>');
let renders = 0;
function renderFixture() {
  const globals = new Map(['document', 'Node', 'Element'].map(name => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  for (const name of globals.keys()) Object.defineProperty(globalThis, name, {value: dom.window[name], configurable: true, writable: true});
  try {
    const named = {};
    let isolatedNodesAfterCleanup = -1;
    const result = renderToString(() => {
      renders++;
      onCleanup(() => { isolatedNodesAfterCleanup = getGraphTelemetry().nodes; });
      for (const [key, value] of Object.entries({checked:true, amount:12, choice:'b', many:['a','c'], text:'<b>not markup</b> & "quoted"', disabled:false, kind:'text', typed:true})) {
        named[key] = signal(value);
        onCleanup(named[key].dispose);
      }
      const input = (id, type, key, props = {}) => h('input', {id, type, 'data-svet-model':key, model:named[key], ...props});
      return h('section', null,
        h('label', null, 'Checked ', input('checked', 'checkbox', 'checked')),
        h('label', null, 'Amount ', input('amount', 'number', 'amount')),
        h('label', null, 'Choice A ', input('choice-a', 'radio', 'choice', {name:'resume-choice',value:'a'})),
        h('label', null, 'Choice B ', input('choice-b', 'radio', 'choice', {name:'resume-choice',value:'b'})),
        h('select', {id:'many',multiple:true,'data-svet-model':'many',model:named.many}, ...['a','b','c'].map(value=>h('option',{value},value))),
        h('label', null, 'Text ', input('text', 'text', 'text')),
        h('label', null, 'Reactive type ', input('typed', named.kind, 'typed', {'data-svet-attr':'type:kind'})),
        h('button', {id:'pressed','aria-pressed':named.checked,'data-svet-attr':'aria-pressed:checked'}, 'Pressed state'),
        h('button', {id:'disabled',disabled:named.disabled,'data-svet-attr':'disabled:disabled'}, 'Disabled state'));
    }, named);
    return {...result, renderCount:renders, isolatedNodesAfterCleanup, mainGraphNodes:getGraphTelemetry().nodes};
  } finally {
    for (const [name, descriptor] of globals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else delete globalThis[name];
    }
  }
}

const server = await createServer({
  root: resolve(import.meta.dirname, '..'),
  server: {host:'127.0.0.1',port:5189,strictPort:true},
  plugins:[{name:'svet-node-ssr-fixture',configureServer(vite){
    vite.middlewares.use((request,response,next)=>{
      if (request.url !== '/__svet_resumption_fixture') return next();
      if (request.method !== 'GET') {response.writeHead(405);response.end();return;}
      try {
        const fixture = renderFixture();
        response.writeHead(200,{'content-type':'application/json','cache-control':'no-store'});
        response.end(JSON.stringify(fixture));
      } catch(error) {
        response.writeHead(500,{'content-type':'application/json'});
        response.end(JSON.stringify({error:String(error)}));
      }
    });
  }}],
});
await server.listen();
console.log('Svet Node SSR/browser resumption fixture: http://127.0.0.1:5189/tests/browser/resumption-roundtrip.html');
let closing = false;
const close = async () => {
  if (closing) return;
  closing = true;server.httpServer?.closeAllConnections();await server.close();dom.window.close();
};
process.once('SIGINT',close);process.once('SIGTERM',close);
