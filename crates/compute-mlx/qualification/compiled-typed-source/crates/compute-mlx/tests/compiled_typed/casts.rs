use super::*;

#[test]
fn all_raw_low_payloads_survive_views_and_changed_strides_without_retracing() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let input = graph.input_low(dtype, shape(&[256, 256])).unwrap();
        let transposed = graph.permute(input, &[1, 0]).unwrap();
        let flat = graph.reshape(transposed, shape(&[65536])).unwrap();
        let repeated = graph.broadcast_to(flat, shape(&[2, 65536])).unwrap();
        let wide = graph.cast_to_f32(input).unwrap();
        let program = graph.compile(&[input, flat, flat, repeated, wide]).unwrap();
        let bits: Vec<_> = (0..=u16::MAX).collect();
        let original = b.upload_low(dtype, shape(&[256, 256]), &bits).unwrap();
        let changed_bits: Vec<_> = bits.iter().map(|x| x ^ 0x8000).collect();
        let changed = b
            .upload_low(dtype, shape(&[256, 256]), &changed_bits)
            .unwrap();
        let changed = b.permute_low(&changed, &[1, 0]).unwrap();
        let mut old = None;
        for (run, input) in [&original, &changed, &original].into_iter().enumerate() {
            let logical: Vec<_> = (0..65536)
                .map(|i| {
                    if run == 1 {
                        changed_bits[(i % 256) * 256 + i / 256]
                    } else {
                        bits[i]
                    }
                })
                .collect();
            let expected: Vec<_> = (0..65536)
                .map(|i| logical[(i % 256) * 256 + i / 256])
                .collect();
            let outputs = program.run_typed(&[input.into()]).unwrap();
            traces(&program, run + 1);
            low_values(&b, &outputs[0], &[256, 256], dtype, &logical);
            low_values(&b, &outputs[1], &[65536], dtype, &expected);
            low_values(&b, &outputs[2], &[65536], dtype, &expected);
            low_values(&b, &outputs[3], &[2, 65536], dtype, &expected.repeat(2));
            floats(
                &b,
                &outputs[4],
                &[256, 256],
                &logical
                    .iter()
                    .map(|&x| decode(dtype, x))
                    .collect::<Vec<_>>(),
            );
            if run == 0 {
                old = Some((outputs, expected));
            }
        }
        let (old, expected) = old.unwrap();
        low_values(&b, &old[1], &[65536], dtype, &expected);
    }
}

#[test]
fn compiled_f32_low_f32_keeps_every_rounding_boundary_inside_the_graph() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let last = if dtype == LowDtype::F16 {
            0x7bff
        } else {
            0x7f7f
        };
        let mut input = Vec::new();
        let mut expected = Vec::new();
        // Midpoints are constructed in f64 from adjacent mathematical values.
        // Tie parity supplies an independent integer rounding oracle.
        for lower in 0..=last {
            let lo = f64::from(decode(dtype, lower));
            let hi = if lower == last {
                lo + (lo - f64::from(decode(dtype, lower - 1)))
            } else {
                f64::from(decode(dtype, lower + 1))
            };
            let midpoint = ((lo + hi) / 2.) as f32;
            let tie = if lower & 1 == 0 { lower } else { lower + 1 };
            for (x, bits) in [
                (midpoint.next_down(), lower),
                (midpoint, tie),
                (midpoint.next_up(), lower + 1),
            ] {
                input.extend([x, -x]);
                expected.extend([decode(dtype, bits), decode(dtype, bits | 0x8000)]);
            }
        }
        input.extend([0., -0., f32::from_bits(1), -f32::from_bits(1), f32::NAN]);
        expected.extend([0., -0., 0., -0., f32::NAN]);
        let mut graph = b.program();
        let x = graph.input(shape(&[input.len()])).unwrap();
        let low = graph.cast_to_low(x, dtype).unwrap();
        let output = graph.cast_to_f32(low).unwrap();
        // Only expose the widened result, so an optimizer cannot rely on an
        // externally requested low output to preserve the rounding barrier.
        let program = graph.compile(&[output]).unwrap();
        let x = b.upload_f32(shape(&[input.len()]), &input).unwrap();
        let result = program.run_typed(&[(&x).into()]).unwrap();
        floats(&b, &result[0], &[input.len()], &expected);
        let changed = b
            .upload_f32(
                shape(&[input.len()]),
                &input.iter().map(|x| -x).collect::<Vec<_>>(),
            )
            .unwrap();
        let result = program.run_typed(&[(&changed).into()]).unwrap();
        floats(
            &b,
            &result[0],
            &[input.len()],
            &expected.iter().map(|x| -x).collect::<Vec<_>>(),
        );
        traces(&program, 2);
    }
}

#[test]
fn scalar_zero_stride_and_empty_low_views_keep_bits() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[])).unwrap();
        let repeated = graph.broadcast_to(x, shape(&[3, 5])).unwrap();
        let transposed = graph.permute(repeated, &[1, 0]).unwrap();
        let flat = graph.reshape(transposed, shape(&[15])).unwrap();
        let empty = graph.broadcast_to(x, shape(&[0, 5])).unwrap();
        let program = graph.compile(&[x, flat, empty]).unwrap();
        for (run, bits) in [0x8001, 0xffff, 0x8000].into_iter().enumerate() {
            let x = b.upload_low(dtype, shape(&[]), &[bits]).unwrap();
            let out = program.run_typed(&[(&x).into()]).unwrap();
            traces(&program, run + 1);
            low_values(&b, &out[0], &[], dtype, &[bits]);
            low_values(&b, &out[1], &[15], dtype, &[bits; 15]);
            low_values(&b, &out[2], &[0, 5], dtype, &[]);
        }
    }
}
