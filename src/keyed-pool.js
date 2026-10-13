// Idle objects kept for reuse under a key; several may share one. Past either limit the oldest
// go first, and `discard` receives every item the pool lets go of without handing it out.
export function createKeyedPool({maxEntries=256,maxBytes=Infinity,discard=()=>{}}={}){
  const byKey=new Map(),order=new Set();let bytes=0;
  function drop(entry){
    order.delete(entry);bytes-=entry.bytes;
    const list=byKey.get(entry.key);list.splice(list.indexOf(entry),1);if(!list.length)byKey.delete(entry.key);
  }
  return {
    put(key,item,size=0){
      if(size>maxBytes||!maxEntries){discard(item);return false;}
      const entry={key,item,bytes:size};order.add(entry);bytes+=size;
      const list=byKey.get(key);if(list)list.push(entry);else byKey.set(key,[entry]);
      while(order.size>maxEntries||bytes>maxBytes){const oldest=order.values().next().value;drop(oldest);discard(oldest.item);}
      return true;
    },
    // The most recently parked item under the key, which leaves the pool.
    take(key){const entry=byKey.get(key)?.at(-1);if(!entry)return undefined;drop(entry);return entry.item;},
    clear(){const items=[...order].map(entry=>entry.item);order.clear();byKey.clear();bytes=0;items.forEach(discard);},
    snapshot:()=>({entries:order.size,bytes,maxEntries,maxBytes}),
  };
}
