import shader from '../vector-ui/src/vector.wgsl?raw';

// WebGPU is only a transport/backend. Rust builds all geometry and tile lists;
// the identical WGSL is used by native wgpu. No canvas bitmap is uploaded.
export async function createGpuPainter(canvas,onFailure){
  if(!navigator.gpu)throw Error('WebGPU недоступен в этом браузере');
  const adapter=await navigator.gpu.requestAdapter();
  if(!adapter)throw Error('Не найден WebGPU adapter');
  const device=await adapter.requestDevice();
  const context=canvas.getContext('webgpu');
  if(!context){device.destroy();throw Error('WebGPU canvas недоступен');}
  const format=navigator.gpu.getPreferredCanvasFormat();
  let disposed=false,buffers=[],bindGroup,modelKey=null,scaleKey=null,sizeKey='',scrollKey='';
  let uploads=0,frames=0;
  const fail=error=>{if(!disposed)onFailure(error instanceof Error?error:Error(String(error)));};
  device.lost.then(info=>fail(Error(`GPU device lost: ${info.message}`)));
  device.addEventListener('uncapturederror',event=>fail(event.error));
  let pipeline;
  try{
    const module=device.createShaderModule({code:shader,label:'Forma vector'});
    pipeline=await device.createRenderPipelineAsync({layout:'auto',vertex:{module,entryPoint:'vs'},fragment:{module,entryPoint:'fs',targets:[{format}]},primitive:{topology:'triangle-list'}});
    context.configure({device,format,alphaMode:'premultiplied'});
  }catch(error){disposed=true;device.destroy();throw error;}
  const uniform=device.createBuffer({size:48,usage:GPUBufferUsage.UNIFORM|GPUBufferUsage.COPY_DST});
  return {
    draw(model,width,height,scale){
      if(disposed)throw Error('GPU painter disposed');
      const size=`${width}:${height}`,scroll=Array.from(model.scroll_offset()).join(':');
      if(width>device.limits.maxTextureDimension2D||height>device.limits.maxTextureDimension2D)throw Error('GPU canvas exceeds texture limit');
      const changed=modelKey!==model||scaleKey!==scale||scrollKey!==scroll;
      if(changed||sizeKey!==size){
        const tiles=model.gpu_tiles(width,height,scale,false);
        const arrays=changed?[model.gpu_commands(scale,false),model.gpu_edges(scale,false),tiles]:[tiles];
        if(changed&&!arrays[1].length)arrays[1]=new Float32Array(4);
        if(arrays.some(a=>!a.byteLength||a.byteLength>device.limits.maxStorageBufferBindingSize))throw Error('Vector scene exceeds GPU storage budget');
        const next=changed?[]:buffers.slice(0,2);
        try{
          for(const data of arrays){const buffer=device.createBuffer({size:data.byteLength,usage:GPUBufferUsage.STORAGE|GPUBufferUsage.COPY_DST});next.push(buffer);device.queue.writeBuffer(buffer,0,data);}
          const nextGroup=device.createBindGroup({layout:pipeline.getBindGroupLayout(0),entries:[{binding:0,resource:{buffer:uniform}},...next.map((buffer,i)=>({binding:i+1,resource:{buffer}}))]});
          (changed?buffers:buffers.slice(2)).forEach(b=>b.destroy());buffers=next;bindGroup=nextGroup;
        }catch(error){(changed?next:next.slice(2)).forEach(b=>b.destroy());throw error;}
        modelKey=model;scaleKey=scale;sizeKey=size;scrollKey=scroll;if(changed)uploads++;
      }
      if(canvas.width!==width)canvas.width=width;if(canvas.height!==height)canvas.height=height;
      device.queue.writeBuffer(uniform,0,model.gpu_params(width,height,scale,false));
      const encoder=device.createCommandEncoder();
      const pass=encoder.beginRenderPass({colorAttachments:[{view:context.getCurrentTexture().createView(),clearValue:{r:0,g:0,b:0,a:0},loadOp:'clear',storeOp:'store'}]});
      pass.setPipeline(pipeline);pass.setBindGroup(0,bindGroup);pass.draw(3);pass.end();device.queue.submit([encoder.finish()]);frames++;
    },
    snapshot(){return {backend:'webgpu-vector',uploads,frames};},
    destroy(){if(disposed)return;disposed=true;buffers.forEach(b=>b.destroy());uniform.destroy();context.unconfigure();device.destroy();},
  };
}
