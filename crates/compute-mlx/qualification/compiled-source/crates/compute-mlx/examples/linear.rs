//! A native MLX GPU graph: matrix product, broadcast bias, ReLU and axis sum.
use compute_mlx::MlxBackend;
use tensor_core::{BinaryOp, MatmulPrecision, Shape, TensorBackend};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = MlxBackend::new_gpu()?;
    let shape = |dims: &[usize]| Shape::new(dims.to_vec());
    let input = backend.upload_f32(shape(&[2, 3])?, &[1., 2., 3., 4., 5., 6.])?;
    let weights = backend.upload_f32(shape(&[3, 2])?, &[1., -1., 2., 0., 0., 1.])?;
    let bias = backend.upload_f32(shape(&[2])?, &[-3., 1.])?;
    let zero = backend.upload_f32(shape(&[])?, &[0.])?;
    let projected = backend.matmul(&input, &weights, MatmulPrecision::F32)?;
    let shifted = backend.binary(BinaryOp::Add, &projected, &bias)?;
    let activated = backend.binary(BinaryOp::Max, &shifted, &zero)?;
    let total = backend.sum_axes(&activated, &[0], false)?;
    let actual = backend.read_f32(&total)?;
    assert_eq!(actual, vec![13., 6.]);
    println!(
        "backend={:?} runtime={} output={actual:?}",
        backend.kind(),
        backend.version()
    );
    Ok(())
}
