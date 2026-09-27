// Storage buffers retain capacity across scene changes and resize. Payload length
// lives in commands/tile headers, so unused capacity is never interpreted.
export function createStoragePool(device,usage){
  const buffers=[],previous=[];let allocations=0,uploadedBytes=0;
  return {
    buffers,
    update(index,data,diff=false,force=false){
      const bytes=data.byteLength,limit=Math.min(device.limits.maxStorageBufferBindingSize,device.limits.maxBufferSize??Infinity);
      if(!bytes||bytes%4||bytes>limit)throw Error('Vector scene exceeds GPU storage budget');
      const old=buffers[index];let next=old;
      if(!old||old.size<bytes){
        const capacity=Math.min(limit,Math.max(256,2**Math.ceil(Math.log2(bytes))));
        next=device.createBuffer({label:'Forma retained vector data',size:capacity,usage});
      }
      const view=new Uint8Array(data.buffer,data.byteOffset,bytes);
      const saved=previous[index];let start=0,end=bytes;
      if(diff&&!force&&next===old&&saved?.length===bytes){
        while(start<bytes&&saved[start]===view[start])start++;
        if(start===bytes)return false;
        while(end>start&&saved[end-1]===view[end-1])end--;
        start-=start%4;end=Math.ceil(end/4)*4;
      }
      try{device.queue.writeBuffer(next,start,start===0&&end===bytes?data:view.subarray(start,end));}catch(error){if(next!==old)next.destroy();previous[index]=undefined;throw error;}
      uploadedBytes+=end-start;
      if(diff){if(!saved||saved.length!==bytes)previous[index]=view.slice();else saved.set(view);}else previous[index]=undefined;
      if(next!==old){buffers[index]=next;allocations++;old?.destroy();return true;}
      return false;
    },
    snapshot(){return {bufferAllocations:allocations,bufferBytes:buffers.reduce((sum,b)=>sum+b.size,0),uploadedBytes};},
    destroy(){buffers.forEach(b=>b.destroy());buffers.length=0;previous.length=0;},
  };
}
