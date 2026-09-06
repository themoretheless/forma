// Storage buffers retain capacity across scene changes and resize. Payload length
// lives in commands/tile headers, so unused capacity is never interpreted.
export function createStoragePool(device,usage){
  const buffers=[];let allocations=0,uploadedBytes=0;
  return {
    buffers,
    update(index,data){
      const bytes=data.byteLength,limit=device.limits.maxStorageBufferBindingSize;
      if(!bytes||bytes%4||bytes>limit)throw Error('Vector scene exceeds GPU storage budget');
      const old=buffers[index];let next=old;
      if(!old||old.size<bytes){
        const capacity=Math.min(limit,Math.max(256,2**Math.ceil(Math.log2(bytes))));
        next=device.createBuffer({label:'Forma retained vector data',size:capacity,usage});
      }
      try{device.queue.writeBuffer(next,0,data);}catch(error){if(next!==old)next.destroy();throw error;}
      uploadedBytes+=bytes;
      if(next!==old){buffers[index]=next;allocations++;old?.destroy();return true;}
      return false;
    },
    snapshot(){return {bufferAllocations:allocations,bufferBytes:buffers.reduce((sum,b)=>sum+b.size,0),uploadedBytes};},
    destroy(){buffers.forEach(b=>b.destroy());buffers.length=0;},
  };
}
