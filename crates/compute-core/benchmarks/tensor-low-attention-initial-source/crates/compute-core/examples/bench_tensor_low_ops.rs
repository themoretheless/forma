//! Packed low operations versus an explicit low -> f32 -> operation baseline.
//! Resident inputs and reusable programs; run on an otherwise idle GPU.
use compute_core::{
    ComputeRuntime,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{LowDtype, ReduceOp, Shape, TensorLowBackend, UnaryOp},
};
#[path = "support/low_bench.rs"]
mod low_bench;
use low_bench::{Result, measure};

fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={};31 rotated samples;200ms warmup;resident low inputs;reused programs;separate unprofiled host+readback and GPU shared-pass timings;exact independent reference every run",
        context.backend_label()
    );
    for (name, rows, cols, transpose) in [
        ("small", 17, 19, false),
        ("rows", 1024, 1025, false),
        ("all", 1, 1_048_581, false),
        ("strided", 257, 4097, true),
    ] {
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            // Fixed dyadic inputs and their squared encodings are independent
            // of all GPU casts. Sums remain exactly representable in f32.
            let (values, squares): ([u16; 9], [u16; 9]) = match dtype {
                LowDtype::F16 => (
                    [
                        0, 0x3400, 0x3800, 0x3a00, 0x3c00, 0x3d00, 0x3e00, 0x3f00, 0x4000,
                    ],
                    [
                        0, 0x2c00, 0x3400, 0x3880, 0x3c00, 0x3e40, 0x4080, 0x4220, 0x4400,
                    ],
                ),
                LowDtype::Bf16 => (
                    [
                        0, 0x3e80, 0x3f00, 0x3f40, 0x3f80, 0x3fa0, 0x3fc0, 0x3fe0, 0x4000,
                    ],
                    [
                        0, 0x3d80, 0x3e80, 0x3f10, 0x3f80, 0x3fc8, 0x4010, 0x4044, 0x4080,
                    ],
                ),
            };
            let physical: Vec<_> = (0..rows * cols)
                .map(|index| {
                    let logical = if transpose {
                        (index % rows) * cols + index / rows
                    } else {
                        index
                    };
                    values[logical % 9]
                })
                .collect();
            let dims = if transpose {
                vec![cols, rows]
            } else {
                vec![rows, cols]
            };
            let mut input = rt.upload_low(dtype, Shape::new(dims)?, &physical)?;
            if transpose {
                input = input.permute(&[1, 0])?;
            }
            let input_bytes = input.allocation_bytes();
            println!(
                "case={name} dtype={dtype:?} rows={rows} cols={cols} transpose={transpose} packed_input_bytes={input_bytes} baseline_expanded_input_bytes={} baseline_square_temporary_bytes={} direct_square_temporary_bytes=0",
                rows * cols * 4,
                rows * cols * 4
            );
            let mut baseline = rt.program();
            let expanded = baseline.tensor_cast_to_f32(&input)?;
            let squared = baseline.tensor_unary(UnaryOp::Square, &expanded)?;
            let baseline_output = baseline.tensor_cast_to_low(&squared, dtype)?;
            let mut direct = rt.program();
            let direct_output = direct.tensor_unary_low(UnaryOp::Square, &input)?;
            let expected: Vec<u32> = (0..rows * cols)
                .step_by(2)
                .map(|i| {
                    u32::from(squares[i % 9])
                        | if i + 1 < rows * cols {
                            u32::from(squares[(i + 1) % 9]) << 16
                        } else {
                            0
                        }
                })
                .collect();
            measure(
                &rt,
                &timer,
                &format!("{name}_{dtype:?}_square"),
                &[baseline, direct],
                &[
                    baseline_output.packed_words().clone(),
                    direct_output.packed_words().clone(),
                ],
                &expected,
                None,
            )?;

            let mut baseline = rt.program();
            let expanded = baseline.tensor_cast_to_f32(&input)?;
            let baseline_output = baseline.tensor_reduce(ReduceOp::Sum, &expanded, &[1], false)?;
            let mut direct = rt.program();
            let direct_output = direct.tensor_reduce_low_f32(ReduceOp::Sum, &input, &[1], false)?;
            let expected: Vec<f32> = (0..rows)
                .map(|row| {
                    (0..cols)
                        .map(|col| ((row * cols + col) % 9) as f64 / 4.)
                        .sum::<f64>() as f32
                })
                .collect();
            measure(
                &rt,
                &timer,
                &format!("{name}_{dtype:?}_sum"),
                &[baseline, direct],
                &[
                    baseline_output.values().clone(),
                    direct_output.values().clone(),
                ],
                &expected,
                None,
            )?;
        }
    }
    Ok(())
}
