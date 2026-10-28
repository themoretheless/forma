use compute_mlx::MlxBackend;
use tensor_core::{LowDtype,Shape,TensorLowBackend,TensorLowOpsBackend,BinaryOp};
fn main()->Result<(),Box<dyn std::error::Error>> {
 let b=MlxBackend::new_gpu()?;let dtype=LowDtype::Bf16;
 let shape=Shape::new(vec![1])?;let mut g=b.program();let x=g.input_low(dtype,shape.clone())?;let y=g.input_low(dtype,shape.clone())?;let z=g.binary_low(x,y,BinaryOp::Multiply)?;let p=g.compile(&[z])?;
 for (l,r) in [(0x0001u16,0x7f7fu16),(0x7f7f,0x0001),(0x007f,0x7f7f),(0x7f7f,0x007f)] {
  let a=b.upload_low(dtype,shape.clone(),&[l])?;let c=b.upload_low(dtype,shape.clone(),&[r])?;
  let eager=b.binary_low(BinaryOp::Multiply,&a,&c)?;let out=p.run_typed(&[(&a).into(),(&c).into()])?;
  let reference=f64::from(f32::from_bits(u32::from(l)<<16))*f64::from(f32::from_bits(u32::from(r)<<16));
  println!("left={l:04x} right={r:04x} f64_product={reference} eager_low={:04x} compiled_low={:04x} traces={}",b.read_low_bits(&eager)?[0],b.read_low_bits(out[0].as_low()?)?[0],p.trace_count());
 }
 Ok(())
}
