import css from './style.css?inline';
export function nativeSnapshot(preview,compiled){
  const clone=preview.cloneNode(true);
  clone.removeAttribute('id');clone.classList.remove('light');clone.classList.add('forma-native-root');clone.style.cssText='width:100%;min-height:100vh;background:#1b2230;color:#c3cada';
  const nodes=new Map();function walk(list){for(const n of list){nodes.set(String(n.start),n);walk(n.children);}}walk(compiled.nodes);
  clone.querySelectorAll('[data-start]').forEach(el=>{
    el.classList.remove('selected');const n=nodes.get(el.dataset.start);
    if(n?.bindings.value==='state.query'){el.dataset.query='true';el.value='';el.removeAttribute('value');}
    if(n?.events.clicked==='actions.search')el.dataset.search='true';
    for(const key of ['text','disabled'])if(n?.props[key]?.expr?.startsWith('state.')){
      el.dataset[key+'State']=n.props[key].expr.slice(6);
      if(key==='text')el.textContent='';else el.disabled=false;
    }
  });
  const bodyHtml=clone.outerHTML.replace(/<\/script/gi,'<\\/script').replace(/\u2028/g,'&#8232;').replace(/\u2029/g,'&#8233;');
  const script=`window.bindNativeControls=function(){document.querySelectorAll('[data-query]').forEach(e=>e.oninput=()=>window.ipc.postMessage(JSON.stringify({kind:'query',value:e.value})));document.querySelectorAll('[data-search]').forEach(e=>e.onclick=()=>window.ipc.postMessage(JSON.stringify({kind:'search'})));};window.applyState=function(s){window.__lastState=s;document.querySelectorAll('[data-query]').forEach(e=>{if(e.value!==s.query)e.value=s.query??''});document.querySelectorAll('[data-text-state]').forEach(e=>e.textContent=s[e.dataset.textState]??'');document.querySelectorAll('[data-disabled-state]').forEach(e=>e.disabled=!!s[e.dataset.disabledState]);document.body.style.visibility='visible';};window.applyHotSnapshot=function(bodyHtml,s){const root=document.querySelector('.forma-native-root');if(root&&bodyHtml){root.outerHTML=bodyHtml;window.bindNativeControls();}if(s||window.__lastState)window.applyState(s||window.__lastState);};window.bindNativeControls();window.ipc.postMessage(JSON.stringify({kind:'ready'}));`;
  return {bodyHtml,html:`<!doctype html><html lang="ru"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><style>${css.replace(/@import\s+url\([^)]*\)\s*;/g,'')}body{visibility:hidden;margin:0;overflow:auto;font-family:system-ui}.ui-node{cursor:default}</style>${bodyHtml}<script>${script}</script></html>`};
}
