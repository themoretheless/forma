//! Resident batched inference-style pipeline. Only the final per-row energies
//! cross to the CPU; inputs can change without rebuilding the prepared program.
use compute_core::{
    BinaryOp, ComputeRuntime, GpuTensor, UnaryOp, gpu_compute::GpuContext, tensor_core::Shape,
};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = GpuContext::new().ok_or("no shader GPU adapter available")?;
    let runtime = ComputeRuntime::new(&context)?;
    let (batch, rows, inner, columns) = (2, 3, 5, 7);
    let inputs = GpuTensor::from_array(
        runtime.zeros(batch * rows * inner)?,
        Shape::new(vec![batch, rows, inner])?,
    )?;
    let weights_host: Vec<f32> = (0..inner * columns)
        .map(|i| (i % 11) as f32 / 16.0 - 0.25)
        .collect();
    let weights = GpuTensor::from_array(
        runtime.upload(&weights_host)?,
        Shape::new(vec![inner, columns])?,
    )?;
    let bias_host: Vec<f32> = (0..columns).map(|i| i as f32 / 8.0).collect();
    let bias = GpuTensor::from_array(runtime.upload(&bias_host)?, Shape::new(vec![columns])?)?;

    let mut program = runtime.program();
    let projected = program.tensor_matmul(&inputs, &weights)?;
    let biased = program.tensor_binary(BinaryOp::Add, &projected, &bias)?;
    let squared = program.tensor_unary(UnaryOp::Square, &biased)?;
    let energy = program.tensor_sum(&squared, &[2], false)?;

    for iteration in 0..3 {
        let values: Vec<f32> = (0..batch * rows * inner)
            .map(|i| ((i + iteration) % 13) as f32 / 8.0)
            .collect();
        runtime.write(inputs.values(), 0, &values)?;
        let actual = program
            .submit_read(energy.values())?
            .wait(Duration::from_secs(10))?;
        let expected: Vec<f64> = (0..batch * rows)
            .map(|row| {
                (0..columns)
                    .map(|column| {
                        let projected = (0..inner)
                            .map(|k| {
                                f64::from(values[row * inner + k])
                                    * f64::from(weights_host[k * columns + column])
                            })
                            .sum::<f64>()
                            + f64::from(bias_host[column]);
                        projected * projected
                    })
                    .sum()
            })
            .collect();
        for (&actual, &expected) in actual.iter().zip(&expected) {
            assert!((f64::from(actual) - expected).abs() <= 3e-5 * expected.abs().max(1.0));
        }
        println!(
            "{} iteration {iteration}: shape {:?}, energies {actual:?}",
            context.backend_label(),
            energy.shape().dims()
        );
    }
    Ok(())
}
