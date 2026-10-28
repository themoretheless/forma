//! cargo run -p compute-cuda --example graph (requires a native NVIDIA device).
use compute_cuda::{CudaCaptureOptions, CudaPrepareOptions, CudaRuntime};
use tensor_core::{Shape, TensorBackend, UnaryOp};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = CudaRuntime::new()?;
    let mut input = runtime.upload_f32(Shape::new(vec![4])?, &[1., 2., 3., 4.])?;
    let mut builder = runtime.program();
    let x = builder.input(input.layout().clone())?;
    let square = builder.unary(x, UnaryOp::Square)?;
    let sum = builder.sum_axes(square, &[0], false)?;
    let prepared = builder.prepare(&[square, sum], CudaPrepareOptions::default())?;
    let mut graph = prepared.capture_owned(CudaCaptureOptions::default())?;
    println!("Graph allocations and scheduled work: {:?}", graph.stats());
    for values in [[1., 2., 3., 4.], [2., 3., 4., 5.]] {
        runtime.write_f32(&mut input, &values)?;
        let outputs = graph.run(&[&input])?;
        let square = runtime.read_f32(&outputs[0])?;
        let sum = runtime.read_f32(&outputs[1])?;
        let expected = values.map(|value| value * value);
        assert_eq!(square, expected);
        assert_eq!(sum, [expected.iter().sum::<f32>()]);
        println!("square={square:?}, sum={sum:?}");
    }
    graph.synchronize()?;
    Ok(())
}
