// Access itself may throw (privacy settings / sandboxed origins).
export function readStorage(kind,key,host=globalThis){try{return host[kind]?.getItem(key)??null;}catch{return null;}}
export function writeStorage(kind,key,value,host=globalThis){try{const storage=host[kind];if(!storage)return false;storage.setItem(key,value);return true;}catch{return false;}}
export function validateProject(data){
 if(!data||Array.isArray(data)||typeof data!=='object'||!Object.keys(data).length||Object.values(data).some(v=>typeof v!=='string'))throw Error('Ожидается JSON проекта: пути файлов и текст');
 for(const path of Object.keys(data))if(path.includes('\\')||path.includes('\0')||path.split('/').some(part=>!part||['.','..','__proto__','prototype','constructor'].includes(part)))throw Error(`Недопустимый путь проекта: ${path}`);
 return data;
}
export function restoreProject(raw,fallback){try{return raw===null?fallback:validateProject(JSON.parse(raw));}catch{return fallback;}}
// Preserve an unreadable project before replacing it. If recovery storage is
// unavailable (including quota errors), leave the original untouched.
export function writeProject(key,files,host=globalThis){
 try{
  const storage=host.localStorage;if(!storage)return false;
  const value=JSON.stringify(validateProject(files));
  const previous=storage.getItem(key);
  // value was validated above; identical persisted bytes need no parse or write.
  if(previous===value)return true;
  if(previous!==null){
   let valid=true;try{validateProject(JSON.parse(previous));}catch{valid=false;}
   if(!valid){
    const recoveryKey=key+':recovery';
    const recovery=storage.getItem(recoveryKey);
    if(recovery!==null&&recovery!==previous)return false;
    if(recovery===null)storage.setItem(recoveryKey,previous);
   }
  }
  storage.setItem(key,value);return true;
 }catch{return false;}
}
