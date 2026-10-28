//! Direct packed selection/indexing/scans versus resident f32 expansion.
use compute_core::{
    ComputeRuntime,
    gpu_compute::{GpuContext, GpuTimer},
    tensor_core::{LowDtype, ScanOptions, Shape, TensorIndexBackend, TensorLowBackend},
};
#[path = "support/low_bench.rs"]
mod low_bench;
use low_bench::{Result, measure};

fn pack(bits: impl IntoIterator<Item = u16>) -> Vec<u32> {
    let bits: Vec<_> = bits.into_iter().collect();
    bits.chunks(2)
        .map(|p| u32::from(p[0]) | (u32::from(p.get(1).copied().unwrap_or(0)) << 16))
        .collect()
}

fn main() -> Result<()> {
    let context = GpuContext::with_timestamps()?;
    let rt = ComputeRuntime::new(&context)?;
    let timer = GpuTimer::new(&context)?;
    println!(
        "backend={};31 rotated samples;200ms warmup;resident low inputs and u32 masks/indices;reused programs;separate unprofiled host+readback and GPU shared-pass timings;exact values and resident counts checked every run",
        context.backend_label()
    );
    for (name, rows, cols, transpose) in [
        ("small", 17, 19, false),
        ("long", 1, 131_077, false),
        ("strided", 257, 1025, true),
    ] {
        let n = rows * cols;
        let mask_values: Vec<u32> = (0..n)
            .map(|i| if i % 5 < 2 { 0 } else { u32::MAX - i as u32 })
            .collect();
        let mask = rt.upload_u32(Shape::new(vec![rows, cols])?, &mask_values)?;
        let indices: Vec<_> = (0..cols.div_ceil(4))
            .map(|i| ((i * 7 + 3) % cols) as u32)
            .chain([cols as u32, u32::MAX])
            .collect();
        let index_tensor = rt.upload_u32(Shape::new(vec![indices.len()])?, &indices)?;
        for dtype in [LowDtype::F16, LowDtype::Bf16] {
            // Finite exactly representable inputs keep the cast baseline
            // numerically equivalent. Full NaN/subnormal payload behavior is
            // verified separately in conformance, not implied by this timing.
            let encodings: [u16; 9] = match dtype {
                LowDtype::F16 => [
                    0, 0x3400, 0x3800, 0x3a00, 0x3c00, 0x3d00, 0x3e00, 0x3f00, 0x4000,
                ],
                LowDtype::Bf16 => [
                    0, 0x3e80, 0x3f00, 0x3f40, 0x3f80, 0x3fa0, 0x3fc0, 0x3fe0, 0x4000,
                ],
            };
            let physical: Vec<_> = (0..n)
                .map(|i| {
                    let logical = if transpose {
                        (i % rows) * cols + i / rows
                    } else {
                        i
                    };
                    encodings[logical % 9]
                })
                .collect();
            let mut input = rt.upload_low(
                dtype,
                Shape::new(if transpose {
                    vec![cols, rows]
                } else {
                    vec![rows, cols]
                })?,
                &physical,
            )?;
            if transpose {
                input = input.permute(&[1, 0])?;
            }
            let other_bits: Vec<_> = (0..n).map(|i| encodings[(i * 7 + 1) % 9]).collect();
            let other = rt.upload_low(dtype, Shape::new(vec![rows, cols])?, &other_bits)?;
            println!(
                "case={name} dtype={dtype:?} rows={rows} cols={cols} transpose={transpose} packed_input_bytes={} index_count={} mask_bytes={} baseline_one_expanded_input_bytes={}",
                input.allocation_bytes(),
                indices.len(),
                n * 4,
                n * 4
            );

            let mut baseline = rt.program();
            let yes = baseline.tensor_cast_to_f32(&input)?;
            let no = baseline.tensor_cast_to_f32(&other)?;
            let chosen = baseline.tensor_select(&mask, &yes, &no)?;
            let old = baseline.tensor_cast_to_low(&chosen, dtype)?;
            let mut direct = rt.program();
            let new = direct.tensor_select_low(&mask, &input, &other)?;
            let expected = pack((0..n).map(|i| {
                if mask_values[i] != 0 {
                    encodings[i % 9]
                } else {
                    other_bits[i]
                }
            }));
            measure(
                &rt,
                &timer,
                &format!("{name}_{dtype:?}_select"),
                &[baseline, direct],
                &[old.packed_words().clone(), new.packed_words().clone()],
                &expected,
                None,
            )?;

            let mut baseline = rt.program();
            let expanded = baseline.tensor_cast_to_f32(&input)?;
            let gathered = baseline.tensor_gather(&expanded, &index_tensor, 1)?;
            let old = baseline.tensor_cast_to_low(&gathered.values, dtype)?;
            let mut direct = rt.program();
            let new = direct.tensor_gather_low(&input, &index_tensor, 1)?;
            let expected = pack((0..rows).flat_map(|row| {
                indices.iter().map(move |&i| {
                    if i < cols as u32 {
                        encodings[(row * cols + i as usize) % 9]
                    } else {
                        0
                    }
                })
            }));
            measure(
                &rt,
                &timer,
                &format!("{name}_{dtype:?}_gather"),
                &[baseline, direct],
                &[
                    old.packed_words().clone(),
                    new.values.packed_words().clone(),
                ],
                &expected,
                Some((
                    &[
                        gathered.invalid_count.values().clone(),
                        new.invalid_count.values().clone(),
                    ],
                    2,
                )),
            )?;

            let mut baseline = rt.program();
            let expanded = baseline.tensor_cast_to_f32(&input)?;
            let compacted = baseline.tensor_compact(&expanded, &mask)?;
            let old = baseline.tensor_cast_to_low(&compacted.values, dtype)?;
            let mut direct = rt.program();
            let new = direct.tensor_compact_low(&input, &mask)?;
            let mut expected: Vec<_> = (0..n)
                .filter(|&i| mask_values[i] != 0)
                .map(|i| encodings[i % 9])
                .collect();
            let count = expected.len() as u32;
            expected.resize(n, 0);
            measure(
                &rt,
                &timer,
                &format!("{name}_{dtype:?}_compact"),
                &[baseline, direct],
                &[
                    old.packed_words().clone(),
                    new.values.packed_words().clone(),
                ],
                &pack(expected),
                Some((
                    &[compacted.count.values().clone(), new.count.values().clone()],
                    count,
                )),
            )?;

            let mut baseline = rt.program();
            let expanded = baseline.tensor_cast_to_f32(&input)?;
            let old = baseline.tensor_scan(&expanded, 1, ScanOptions::default())?;
            let mut direct = rt.program();
            let new = direct.tensor_scan_low_f32(&input, 1, ScanOptions::default())?;
            let expected: Vec<f32> = (0..rows)
                .flat_map(|row| {
                    let mut sum = 0f64;
                    (0..cols).map(move |col| {
                        sum += ((row * cols + col) % 9) as f64 / 4.;
                        sum as f32
                    })
                })
                .collect();
            measure(
                &rt,
                &timer,
                &format!("{name}_{dtype:?}_scan"),
                &[baseline, direct],
                &[old.values().clone(), new.values().clone()],
                &expected,
                None,
            )?;
        }
    }
    Ok(())
}
