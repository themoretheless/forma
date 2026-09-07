import shader from '../vector-ui/src/vector.wgsl?raw';
import {createStoragePool} from './gpu-buffers.js';

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
  let disposed=false,bindGroup,modelKey=null,scaleKey=null,tilesXKey=0,tilesYKey=0,scrollXKey=0,scrollYKey=0,geometryKey=null,visualKey=null,widthKey=0,heightKey=0;
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
  const pool=createStoragePool(device,GPUBufferUsage.STORAGE|GPUBufferUsage.COPY_DST);
  const validate=data=>{if(data&&(!data.byteLength||data.byteLength%4||data.byteLength>device.limits.maxStorageBufferBindingSize))throw Error('Vector scene exceeds GPU storage budget');};
  return {
    draw(model,width,height,scale){
      if(disposed)throw Error('GPU painter disposed');
      const tilesX=Math.ceil(width/32),tilesY=Math.ceil(height/32),scroll=model.scroll_offset();
      if(width>device.limits.maxTextureDimension2D||height>device.limits.maxTextureDimension2D)throw Error('GPU canvas exceeds texture limit');
      const geometry=model.geometry_revision?.()??0,visual=model.visual_revision?.();
      const changed=modelKey!==model||scaleKey!==scale||scrollXKey!==scroll[0]||scrollYKey!==scroll[1]||geometryKey!==geometry;
      const tilesChanged=changed||tilesXKey!==tilesX||tilesYKey!==tilesY;
      // Older scene adapters without a revision retain their eager upload path.
      const paintsChanged=changed||visual===undefined||visualKey!==visual;
      const paramsChanged=paintsChanged||widthKey!==width||heightKey!==height;
      try{
        const tiles=tilesChanged?model.gpu_tiles(width,height,scale,false):null;
        const commands=changed?model.gpu_commands(scale,false):null;
        let edges=changed?model.gpu_edges(scale,false):null;
        if(edges&&!edges.length)edges=new Float32Array(4);
        const paints=paintsChanged?model.gpu_paints():null;
        validate(commands);validate(edges);validate(tiles);validate(paints);
        // Invalidate immediately: a later upload can fail after an old buffer
        // was destroyed. A retry must never keep its stale bind group.
        if(commands&&pool.update(0,commands))bindGroup=null;
        if(edges&&pool.update(1,edges))bindGroup=null;
        if(tiles&&pool.update(2,tiles))bindGroup=null;
        if(paints&&pool.update(3,paints))bindGroup=null;
        if(!bindGroup)bindGroup=device.createBindGroup({layout:pipeline.getBindGroupLayout(0),entries:[{binding:0,resource:{buffer:uniform}},...pool.buffers.map((buffer,i)=>({binding:i+1,resource:{buffer}}))]});
        if(canvas.width!==width)canvas.width=width;if(canvas.height!==height)canvas.height=height;
        if(paramsChanged)device.queue.writeBuffer(uniform,0,model.gpu_params(width,height,scale,false));
        const encoder=device.createCommandEncoder();
        const pass=encoder.beginRenderPass({colorAttachments:[{view:context.getCurrentTexture().createView(),clearValue:{r:0,g:0,b:0,a:0},loadOp:'clear',storeOp:'store'}]});
        pass.setPipeline(pipeline);pass.setBindGroup(0,bindGroup);pass.draw(3);pass.end();device.queue.submit([encoder.finish()]);frames++;
        modelKey=model;geometryKey=geometry;scaleKey=scale;tilesXKey=tilesX;tilesYKey=tilesY;scrollXKey=scroll[0];scrollYKey=scroll[1];visualKey=visual;widthKey=width;heightKey=height;if(changed)uploads++;
      }catch(error){
        // A partial upload may overwrite retained buffers even if no buffer
        // grew. Retrying either the new or the previous model must restore all
        // payloads instead of treating the previous keys as a complete scene.
        modelKey=null;
        throw error;
      }
    },
    snapshot(){return {backend:'webgpu-vector',uploads,frames,...pool.snapshot()};},
    destroy(){if(disposed)return;disposed=true;modelKey=null;bindGroup=null;pool.destroy();uniform.destroy();context.unconfigure();device.destroy();},
  };
}
