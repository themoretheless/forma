use compute_core::{Binding,ComputeRuntime,Kernel,gpu_compute::GpuContext,shaders};
#[path="../src/tensor/low/statistics_sources.rs"] mod sources;
fn main(){let c=GpuContext::new().unwrap();let r=ComputeRuntime::new(&c).unwrap();
let source=sources::small().replace("output[destination] = bitcast<u32>(normalized);",r#"
if lid.x==0u {
 output[0]=bitcast<u32>(x);output[1]=bitcast<u32>(extrema.x);output[2]=bitcast<u32>(extrema.y);
 output[3]=select(0u,1u,lifted);output[4]=bitcast<u32>(anchor);output[5]=bitcast<u32>(scale);
 output[6]=bitcast<u32>(transformed);output[7]=bitcast<u32>(mean);output[8]=bitcast<u32>(centered);output[9]=bitcast<u32>(scaled_variance);
 output[10]=bitcast<u32>(normalized);output[11]=bitcast<u32>((scale/bitcast<f32>(params[8]))*5.421010862427522e-20);
}"#);
let k=Kernel::new(r.device(),"debug",&source,"main",&[Binding::StorageRead,Binding::StorageRead,Binding::StorageReadWrite,Binding::StorageReadWrite]).unwrap();
for raw in [vec![0x8001u16,1],vec![0x7f7f,0x7f7e,0x7f7d,0x7f7b,0x7f77]] {
let mut m=vec![1,0,0,raw.len() as u32,1,1,1,4,f32::from_bits(1).sqrt().to_bits(),0,0,raw.len() as u32,1,raw.len() as u32,1,1,1,4];
if raw.len()>2 {m[8]=1e-5f32.sqrt().to_bits();}
let words:Vec<_>=raw.chunks(2).map(|c|u32::from(c[0])|(u32::from(*c.get(1).unwrap_or(&0))<<16)).collect();
let meta=r.upload(&m).unwrap();let input=r.upload(&words).unwrap();let out=r.zeros::<u32>(12).unwrap();let v=r.zeros::<u32>(1).unwrap();
k.dispatch(r.device(),r.queue(),&[meta.view().raw(),input.view().raw(),out.view().raw(),v.view().raw()],256);
let raw=r.read(&out).unwrap().wait(std::time::Duration::from_secs(10)).unwrap();println!("{:x?}",raw);println!("{:?}",raw.iter().map(|v|f32::from_bits(*v)).collect::<Vec<_>>());
}}
