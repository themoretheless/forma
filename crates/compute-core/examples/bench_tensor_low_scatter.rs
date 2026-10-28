//! Resident low scatter versus explicit f32 expansion, with matched outputs.
use compute_core::{
    ComputeRuntime, GpuLowTensor,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{LowDtype, ScatterOp, Shape, TensorIndexBackend, TensorLowBackend},
};
#[path = "support/low_bench.rs"]
mod low_bench;
use low_bench::{Result, measure};

// Numerical rounding for this benchmark's finite normal dyadic values. This
// oracle uses arithmetic on significands, independently of device bit codecs.
fn encode(dtype: LowDtype, value: f64) -> u16 {
    let sign = if value.is_sign_negative() { 0x8000 } else { 0 };
    let value = value.abs();
    if value == 0. {
        return sign;
    }
    let (precision, bias, max_exponent) = match dtype {
        LowDtype::F16 => (10, 15, 31),
        LowDtype::Bf16 => (7, 127, 255),
    };
    let mut exponent = value.log2().floor() as i32;
    assert!(exponent >= 1 - bias);
    let mut significand = (value * 2f64.powi(precision - exponent)).round_ties_even() as u16;
    if significand == 1 << (precision + 1) {
        exponent += 1;
        significand >>= 1;
    }
    if exponent + bias >= max_exponent {
        sign | ((max_exponent as u16) << precision)
    } else {
        sign | (((exponent + bias) as u16) << precision) | (significand - (1 << precision))
    }
}

fn pack(bits: &[u16]) -> Vec<u32> {
    bits.chunks(2)
        .map(|pair| u32::from(pair[0]) | (u32::from(pair.get(1).copied().unwrap_or(0)) << 16))
        .collect()
}

fn upload(
    rt: &ComputeRuntime,
    dtype: LowDtype,
    rows: usize,
    cols: usize,
    transposed: bool,
    values: &[f64],
) -> Result<GpuLowTensor> {
    let physical = (0..values.len())
        .map(|i| {
            let logical = if transposed {
                (i % rows) * cols + i / rows
            } else {
                i
            };
            encode(dtype, values[logical])
        })
        .collect::<Vec<_>>();
    let shape = Shape::new(if transposed {
        vec![cols, rows]
    } else {
        vec![rows, cols]
    })?;
    let tensor = rt.upload_low(dtype, shape, &physical)?;
    Ok(if transposed {
        tensor.permute(&[1, 0])?
    } else {
        tensor
    })
}

fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={};31 rotated samples;200ms warmup;resident low base/updates and u32 indices;reused programs;separate unprofiled host+readback and GPU shared-pass timings;all low values and invalid counts checked every run",
        context.backend_label()
    );
    for (name, rows, cols, valid_indices, destinations, transpose) in [
        ("small", 17usize, 19usize, 65usize, 19usize, false),
        ("unique", 257, 1025, 1025, 1025, false),
        ("spread", 129, 1025, 2051, 1025, false),
        ("hot", 17, 19, 4099, 1, false),
        ("strided", 257, 1025, 513, 1025, true),
    ] {
        // 7 is coprime to both destination extents. Two invalid indices are
        // always present and must each be counted once, independently of rows.
        let indices = (0..valid_indices)
            .map(|j| ((j * 7 + 3) % destinations) as u32)
            .chain([cols as u32, u32::MAX])
            .collect::<Vec<_>>();
        let count = indices.len();
        let index_tensor = rt.upload_u32(Shape::new(vec![count])?, &indices)?;
        let base = (0..rows * cols)
            .map(|i| ((i % 8) + 1) as f64 / 4.)
            .collect::<Vec<_>>();
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            let input = upload(&rt, dtype, rows, cols, transpose, &base)?;
            println!(
                "case={name} dtype={dtype:?} rows={rows} cols={cols} indices={count} destinations={destinations} transpose={transpose} base_numel={} update_numel={} removed_f32_base_bytes={} removed_f32_updates_bytes={}",
                base.len(),
                rows * count,
                4 * base.len(),
                4 * rows * count
            );
            for op in [
                ScatterOp::Replace,
                ScatterOp::Add,
                ScatterOp::Multiply,
                ScatterOp::Min,
                ScatterOp::Max,
            ] {
                // Quarter-integer sums are exactly representable in f32 even
                // under contention. Products of +/-1 cannot under/overflow.
                let updates = (0..rows * count)
                    .map(|i| {
                        if op == ScatterOp::Multiply {
                            if i % 3 == 0 { -1. } else { 1. }
                        } else {
                            ((i % 8) + 1) as f64 / 4.
                        }
                    })
                    .collect::<Vec<_>>();
                let update_tensor = upload(&rt, dtype, rows, count, transpose, &updates)?;
                let mut expected = base.clone();
                for row in 0..rows {
                    for (j, &destination) in indices.iter().enumerate() {
                        if (destination as usize) < cols {
                            let old = &mut expected[row * cols + destination as usize];
                            let value = updates[row * count + j];
                            *old = match op {
                                ScatterOp::Replace => value,
                                ScatterOp::Add => *old + value,
                                ScatterOp::Multiply => *old * value,
                                ScatterOp::Min => old.min(value),
                                ScatterOp::Max => old.max(value),
                            };
                        }
                    }
                }
                let expected = pack(
                    &expected
                        .iter()
                        .map(|&v| encode(dtype, v))
                        .collect::<Vec<_>>(),
                );
                let mut baseline = rt.program();
                let expanded_base = baseline.tensor_cast_to_f32(&input)?;
                let expanded_updates = baseline.tensor_cast_to_f32(&update_tensor)?;
                let scattered = baseline.tensor_scatter(
                    op,
                    &expanded_base,
                    &index_tensor,
                    &expanded_updates,
                    1,
                )?;
                let old = baseline.tensor_cast_to_low(&scattered.values, dtype)?;
                let mut direct = rt.program();
                let new =
                    direct.tensor_scatter_low(op, &input, &index_tensor, &update_tensor, 1)?;
                measure(
                    &rt,
                    &timer,
                    &format!("{name}_{dtype:?}_{op:?}"),
                    &[baseline, direct],
                    &[
                        old.packed_words().clone(),
                        new.values.packed_words().clone(),
                    ],
                    &expected,
                    Some((
                        &[
                            scattered.invalid_count.values().clone(),
                            new.invalid_count.values().clone(),
                        ],
                        2,
                    )),
                )?;
            }
        }
    }
    Ok(())
}
