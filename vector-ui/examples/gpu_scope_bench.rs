//! Real Metal clear-buffer + error-scope completion timings, without presentation.
//! This benchmark is macOS-only: upstream wgpu error-scope completion does not imply queue idle.
#[cfg(all(feature="gpu",target_os="macos"))]
use forma::gpu_runtime as wgpu;
#[cfg(not(feature="gpu"))]
fn main(){panic!("Run with --features gpu");}
#[cfg(all(feature="gpu",not(target_os="macos")))]
fn main(){panic!("The GPU scope benchmark requires the in-tree Metal runtime on macOS");}
#[cfg(all(feature="gpu",target_os="macos"))]
fn main(){pollster::block_on(run());}
#[cfg(all(feature="gpu",target_os="macos"))]
async fn run(){
    use std::{future::{poll_fn,Future},task::Poll,time::Instant};
    let instance=wgpu::Instance::default();
    let adapter=instance.request_adapter(&Default::default()).await.expect("hardware adapter");
    let info=adapter.get_info();assert_ne!(info.device_type,wgpu::DeviceType::Cpu);
    let (device,queue)=adapter.request_device(&Default::default()).await.unwrap();
    let bytes=1_048_576usize;
    let buffer=device.create_buffer(&wgpu::BufferDescriptor {label:Some("scope benchmark clear target"),size:bytes as u64,
        usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let second=wgpu::Instance::default();
    let second_adapter=second.request_adapter(&Default::default()).await.unwrap();
    assert_eq!(second_adapter.get_info().name,info.name);
    let (control_device,control_queue)=second_adapter.request_device(&Default::default()).await.unwrap();
    let control_buffer=control_device.create_buffer(&wgpu::BufferDescriptor {label:Some("independent explicit-wait target"),size:bytes as u64,
        usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false});
    let contexts=[(device,queue,buffer),(control_device,control_queue,control_buffer)];
    let warmups=std::env::var("FORMA_SCOPE_WARMUPS").map(|v|v.parse::<usize>().unwrap()).unwrap_or(16);
    let measured=std::env::var("FORMA_SCOPE_SAMPLES").map(|v|v.parse::<usize>().unwrap()).unwrap_or(64);
    assert!((1..=1024).contains(&warmups) && (4..=4096).contains(&measured));
    let sentinel=vec![0xa5;bytes];
    let mut samples=Vec::new();
    for scopes in [1,8,32,64] {
        for round in 0..warmups+measured {
            // Alternating order avoids assigning every first operation to one mode.
            for explicit_wait in if round%2==0 {[false,true]} else {[true,false]} {
                let (device,queue,buffer)=&contexts[usize::from(explicit_wait)];
                queue.write_buffer(&buffer,0,&sentinel);
                let start=Instant::now();
                let guards:Vec<_>=(0..scopes).map(|_|device.push_error_scope(wgpu::ErrorFilter::Internal)).collect();
                let mut encoder=device.create_command_encoder(&Default::default());
                encoder.clear_buffer(&buffer,0,None);queue.submit([encoder.finish()]);
                let mut futures:Vec<_>=guards.into_iter().map(|guard|Some(Box::pin(guard.pop()))).collect();
                let submit_ms=start.elapsed().as_secs_f64()*1000.;
                let wait=Instant::now();
                if explicit_wait {device.poll(wgpu::PollType::wait_indefinitely()).unwrap();}
                poll_fn(|cx| {
                    for future in &mut futures {
                        if let Some(pending)=future {
                            if let Poll::Ready(error)=pending.as_mut().poll(cx) {
                                assert!(error.is_none(),"unexpected GPU scope error: {error:?}");*future=None;
                            }
                        }
                    }
                    if futures.iter().all(Option::is_none) {Poll::Ready(())} else {Poll::Pending}
                }).await;
                let wait_ms=wait.elapsed().as_secs_f64()*1000.;
                let total_ms=start.elapsed().as_secs_f64()*1000.;
                assert!(device.is_queue_idle(),"scope pop completed before submitted work");
                buffer.slice(..).map_async(wgpu::MapMode::Read,|r|r.unwrap());
                {let mapped=buffer.slice(..).get_mapped_range();assert!(mapped.iter().all(|&x|x==0),"clear output is not exact");}
                buffer.unmap();
                if round>=warmups {samples.push(serde_json::json!({"scopes":scopes,"explicit_wait":explicit_wait,"round":round-warmups,"submit_ms":submit_ms,"wait_ms":wait_ms,"total_ms":total_ms}));}
            }
        }
    }
    println!("{}",serde_json::json!({"status":"partial","adapter":info.name,"backend":format!("{:?}",info.backend),"buffer_bytes":bytes,"warmups":warmups,"measured_samples_per_group":measured,"independent_mode_backends":true,
        "workload":"Nested scopes around one real clear command; pop all, await concurrently; exact zero-byte readback after every sample; configurable warmup/measured alternating-order rounds; separate Metal backends per mode",
        "limits":["One process/machine; descriptive timing, not paired optimization qualification","No OS input/presentation, GPU timestamps, RSS, thread sampling or energy measurement","Explicit wait blocks caller; async mode uses completion service; timings include CPU encoding and scheduling"],"samples":samples}));
}
