// The dev-server half of Studio: buttons that run local Rust (design data, the app window,
// a Cargo project) and the MCP command transport. It only exists while Vite serves the page,
// so the editor mounts it with `import.meta.hot` and a small facade instead of letting the
// websocket protocol live inside the editor module.
//
// studio: {anchor, log, showEvents, compiled, files, designReferences, setDesignData, compile,
//          setError, persist, ideCommand, showNativeInspection}
// env:    browser services, injectable so the request lifecycle is testable without a browser.
export function mountDevBridge(hot,studio,env=globalThis){
 const {document,confirm,crypto,setTimeout,clearTimeout}=env;
 const button=(text,title)=>{const b=document.createElement('button');b.textContent=text;b.title=title;return b;};

 // Design data: one request at a time; a timeout, a lost socket or a reload all release the button,
 // and a late reply for an older request is ignored.
 let designRequest,designTimer;
 const refreshDesign=button('↻ Данные дизайна','Выполнить локальный src/design.rs (без песочницы), получить текстовые константы и результаты методов');
 const finishDesignRequest=()=>{const request=designRequest;designRequest=undefined;clearTimeout(designTimer);designTimer=undefined;refreshDesign.disabled=false;return request;};
 const cancelDesignRequest=()=>{if(finishDesignRequest())studio.log('Соединение потеряно: результат дизайн-данных неизвестен. Обновите их после подключения.');};
 refreshDesign.onclick=()=>{
  if(!studio.compiled||designRequest)return;
  const refs=studio.designReferences(studio.compiled);
  if(!refs.length){studio.log('Нет ссылок design в дизайне');return;}
  if(!confirm('Выполнить src/design.rs для получения дизайн-данных? Rust-код работает с правами вашего пользователя.'))return;
  designRequest={id:crypto.randomUUID(),source:JSON.stringify(studio.files)};
  refreshDesign.disabled=true;
  designTimer=setTimeout(()=>{if(finishDesignRequest())studio.log('Ответ дизайн-данных не получен. Исполнение могло завершиться; автоматического повтора нет.');},45000);
  try{hot.send('forma:design-data',{id:designRequest.id,files:studio.files,refs});}
  catch(e){finishDesignRequest();studio.log('Не удалось отправить запрос дизайн-данных: '+e.message);}
 };
 const receiveDesignData=data=>{
  if(!designRequest||data?.id!==designRequest.id)return;
  const request=finishDesignRequest();
  if(request.source!==JSON.stringify(studio.files)){studio.log('Проект изменился: обновите дизайн-данные ещё раз');return;}
  if(data.error){studio.setError(data.error);return;}
  studio.setDesignData(data.values);studio.compile();studio.log('Дизайн-данные обновлены из Rust');
 };

 // Native runs: the app window and a plain Cargo project; the window reports inspection lines.
 const runApp=button('▶ Приложение','Запустить текущее приложение (⌘↵)');runApp.id='app-run';runApp.className='accent';
 runApp.onclick=()=>{studio.showEvents();studio.ideCommand('app_run');};
 const runRust=button('Cargo · диагностика','Собрать и запустить Cargo-проект без native-окна');
 const stopRust=button('■','Остановить Cargo-диагностику');stopRust.disabled=true;
 runRust.onclick=()=>{studio.persist();studio.showEvents();studio.log('Запуск Cargo-диагностики (offline)…');runRust.disabled=true;stopRust.disabled=false;hot.send('forma:rust-run',{files:studio.files});};
 stopRust.onclick=()=>hot.send('forma:rust-stop',{});
 let buffer='';
 const rustOutput=data=>{
  buffer+=data.text;let lineEnd;
  while((lineEnd=buffer.indexOf('\n'))>=0){const line=buffer.slice(0,lineEnd);buffer=buffer.slice(lineEnd+1);if(line.startsWith('FORMA_INSPECT ')){try{studio.showNativeInspection(JSON.parse(line.slice(14)));}catch(e){studio.log('Ошибка данных инспекции: '+e.message);}}}
  if(buffer.length>200000)buffer='';
  if(!data.text.startsWith('FORMA_INSPECT '))studio.log('[Rust] '+data.text);
  if(data.kind==='finished'||data.kind==='error'){runRust.disabled=false;stopRust.disabled=true;}
 };

 // MCP transport: the server forwards a command, the editor answers through the same facade.
 const hello=()=>hot.send('forma:hello',{});
 const handle=({id,command,args})=>{try{hot.send('forma:result',{id,result:studio.ideCommand(command,args)});}catch(e){hot.send('forma:result',{id,error:e.message});}};

 studio.anchor.before(refreshDesign,runApp,runRust,stopRust);
 const subscriptions=[['vite:ws:disconnect',cancelDesignRequest],['forma:design-data-result',receiveDesignData],['forma:rust-output',rustOutput],['forma:command',handle],['vite:ws:connect',hello]];
 for(const [event,listener]of subscriptions)hot.on(event,listener);
 hello();
 hot.dispose(()=>{finishDesignRequest();for(const [event,listener]of subscriptions)hot.off(event,listener);buffer='';});
}
