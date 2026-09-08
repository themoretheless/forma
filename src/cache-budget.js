// Conservative accounting for retained JS data, not a measurement of VM heap.
// Shared objects are charged once per entry; independent entries may overcount.
export function estimateCacheBytes(value){
  const seen=new Set();
  function visit(v){
    if(typeof v==='string')return 24+v.length*2;
    if(v===null||typeof v!=='object')return 8;
    if(seen.has(v))return 0;seen.add(v);
    if(ArrayBuffer.isView(v))return 64+v.byteLength;
    if(v instanceof Map){let bytes=64;for(const [k,x]of v)bytes+=32+visit(k)+visit(x);return bytes;}
    let bytes=Array.isArray(v)?32:64;
    for(const k in v)if(Object.hasOwn(v,k))bytes+=16+visit(k)+visit(v[k]);
    return bytes;
  }
  return visit(value);
}

export function createCacheBudget({maxBytes=8*1024*1024,maxEntries=256}={}){
  if(!Number.isSafeInteger(maxBytes)||maxBytes<0||!Number.isSafeInteger(maxEntries)||maxEntries<0)throw Error('Invalid cache budget');
  const entries=new Map();let bytes=0,hits=0,misses=0,evictions=0;
  function remove(key){const entry=entries.get(key);if(entry){bytes-=entry.bytes;entries.delete(key);}}
  return {
    get(key){const entry=entries.get(key);if(!entry){misses++;return undefined;}hits++;entries.delete(key);entries.set(key,entry);return entry.value;},
    set(key,value){
      const size=estimateCacheBytes(value)+estimateCacheBytes(key);remove(key);
      if(size>maxBytes||!maxEntries)return false;
      while(entries.size>=maxEntries||bytes+size>maxBytes){remove(entries.keys().next().value);evictions++;}
      entries.set(key,{value,bytes:size});bytes+=size;return true;
    },
    delete:remove,
    clear(){entries.clear();bytes=0;},
    snapshot(){return {entries:entries.size,estimatedBytes:bytes,maxBytes,maxEntries,hits,misses,evictions};},
  };
}
