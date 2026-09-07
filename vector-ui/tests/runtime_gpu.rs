#![cfg(feature = "gpu")]
use forma::{gpu::Renderer, Runtime};

fn read(renderer:&mut Renderer,model:&Runtime,scale:f32)->Vec<u8>{
    let w=(128.*scale)as u32;let h=(128.*scale)as u32;
    let target=renderer.device.create_texture(&wgpu::TextureDescriptor{label:Some("Independent controls"),size:wgpu::Extent3d{width:w,height:h,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8Unorm,usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[]});
    renderer.draw(model,&target.create_view(&Default::default()),w,h,scale,true,false).unwrap();
    let stride=(w*4).div_ceil(256)*256;
    let buffer=renderer.device.create_buffer(&wgpu::BufferDescriptor{label:None,size:stride as u64*h as u64,usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let mut encoder=renderer.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(target.as_image_copy(),wgpu::TexelCopyBufferInfo{buffer:&buffer,layout:wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(stride),rows_per_image:Some(h)}},wgpu::Extent3d{width:w,height:h,depth_or_array_layers:1});
    renderer.queue.submit(Some(encoder.finish()));
    let (tx,rx)=std::sync::mpsc::channel();buffer.slice(..).map_async(wgpu::MapMode::Read,move |r|{tx.send(r).unwrap();});
    renderer.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();rx.recv().unwrap().unwrap();
    let mapped=buffer.slice(..).get_mapped_range();let mut out=Vec::new();for row in mapped.chunks_exact(stride as usize){out.extend_from_slice(&row[..w as usize*4]);}out
}
#[test]
#[ignore="requires a real GPU"]
fn animations_change_only_their_own_pixels_at_integer_and_fractional_dpi(){
    pollster::block_on(async{
        let instance=wgpu::Instance::default();let adapter=instance.request_adapter(&Default::default()).await.unwrap();
        let mut renderer=Renderer::new(&adapter,wgpu::TextureFormat::Rgba8Unorm).await.unwrap();
        let source="component Demo { Frame { width:128; height:128; padding:8; gap:12; clip:true; radius:12; background:#112233; Button { key:'one'; width:100; height:40; background:#ff0000; } Button { key:'two'; width:100; height:40; background:#00ff00; } } }";
        let template="component Button { Rectangle { radius:8; background:Brush { color:props.background; hover:#0000ff; pressed:#ffffff; transition:0ms; }; Border { width:1; background:#aabbcc; } PointerArea { clicked -> events.clicked(); } } }";
        for scale in [1.,1.25,2.]{
            let mut model=Runtime::from_sources(source,template).unwrap();let before=read(&mut renderer,&model,scale);let uploads=renderer.uploads;
            model.pointer(30.,25.,0);let hovered=read(&mut renderer,&model,scale);
            assert_eq!(renderer.uploads,uploads,"paint-only changes must not upload geometry");
            let w=(128.*scale)as usize;
            let pixel=|data:&[u8],x:usize,y:usize|->Vec<u8>{let i=(((y as f32*scale)as usize)*w+(x as f32*scale)as usize)*4;data[i..i+4].to_vec()};
            assert_eq!(pixel(&before,30,25),vec![255,0,0,255]);assert_eq!(pixel(&hovered,30,25),vec![0,0,255,255]);
            assert_eq!(pixel(&hovered,30,75),vec![0,255,0,255]);
            let lower=(55.*scale)as usize*w*4;
            assert_eq!(&before[lower..],&hovered[lower..],"the entire second control including AA/text/clip must remain byte-identical");
            assert_eq!(hovered[3],0,"rounded root clips all children");
            model.pointer(30.,75.,1);let pressed=read(&mut renderer,&model,scale);assert_eq!(pixel(&pressed,30,25),vec![255,0,0,255]);assert_eq!(pixel(&pressed,30,75),vec![255;4]);
        }
    });
}
