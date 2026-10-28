use super::*;

#[test]
fn compiled_low_compare_exhausts_payloads_with_changed_strided_inputs() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let a = graph.input_low(dtype, shape(&[256, 256])).unwrap();
        let c = graph.input_low(dtype, shape(&[256, 256])).unwrap();
        let values: Vec<_> = OPS
            .into_iter()
            .map(|op| graph.compare_low(a, c, op).unwrap())
            .collect();
        let program = graph.compile(&values).unwrap();
        let mut old: Option<(Vec<MlxProgramOutput>, Vec<Vec<u32>>)> = None;
        for replay in 0..3 {
            let lhs: Vec<_> = (0..=u16::MAX).collect();
            let rhs: Vec<_> = lhs
                .iter()
                .map(|&x| match replay {
                    0 => x,
                    1 => x ^ 0x8000,
                    _ => x.wrapping_mul(40503).wrapping_add(197),
                })
                .collect();
            let physical = |data: &[u16]| -> Vec<u16> {
                if replay == 0 {
                    data.to_vec()
                } else {
                    (0..256)
                        .flat_map(|column| (0..256).map(move |row| data[row * 256 + column]))
                        .collect()
                }
            };
            let a = b
                .upload_low(dtype, shape(&[256, 256]), &physical(&lhs))
                .unwrap();
            let c = b
                .upload_low(dtype, shape(&[256, 256]), &physical(&rhs))
                .unwrap();
            let a = if replay == 0 {
                a
            } else {
                b.permute_low(&a, &[1, 0]).unwrap()
            };
            let c = if replay == 0 {
                c
            } else {
                b.permute_low(&c, &[1, 0]).unwrap()
            };
            let out = program.run_typed(&[(&a).into(), (&c).into()]).unwrap();
            let expected: Vec<Vec<_>> = OPS
                .into_iter()
                .map(|op| {
                    lhs.iter()
                        .zip(&rhs)
                        .map(|(&a, &c)| compare(decode(dtype, a), decode(dtype, c), op))
                        .collect()
                })
                .collect();
            for (value, expected) in out.iter().zip(&expected) {
                uints(&b, value, &[256, 256], expected);
            }
            if let Some((previous, expected)) = &old {
                for (value, expected) in previous.iter().zip(expected) {
                    uints(&b, value, &[256, 256], expected);
                }
            }
            old = Some((out, expected));
            traces(&program, replay + 1);
        }
    }
}

#[test]
fn compiled_low_select_gather_preserve_all_raw_bits_and_survive_program_drop() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[256, 256])).unwrap();
        let mask = graph.input_u32(shape(&[256, 1])).unwrap();
        let fallback = graph.input_low(dtype, shape(&[])).unwrap();
        let index = graph.input_u32(shape(&[257])).unwrap();
        let routed = graph.select_low(mask, x, fallback).unwrap();
        let gathered = graph.gather_low(x, index, 0).unwrap();
        let selected_gather = graph.gather_low(routed, index, 0).unwrap();
        let program = graph
            .compile(&[
                routed,
                gathered.values,
                gathered.invalid_count,
                selected_gather.values,
                selected_gather.invalid_count,
                x,
                gathered.values,
            ])
            .unwrap();
        let mut old: Option<(MlxProgramOutput, Vec<u16>)> = None;
        for replay in 0..2 {
            let raw: Vec<_> = (0..=u16::MAX)
                .map(|x| x.wrapping_add(replay as u16 * 59))
                .collect();
            let physical: Vec<_> = (0..256)
                .flat_map(|c| {
                    let raw = &raw;
                    (0..256).map(move |r| raw[r * 256 + c])
                })
                .collect();
            let source = b.upload_low(dtype, shape(&[256, 256]), &physical).unwrap();
            let input = b.permute_low(&source, &[1, 0]).unwrap();
            let flags: Vec<_> = (0..256)
                .map(|r| if (r + replay) % 3 == 0 { 0 } else { u32::MAX })
                .collect();
            let mask = b.upload_u32(shape(&[256, 1]), &flags).unwrap();
            // A signaling NaN payload must route unchanged, including to both
            // selected paths; no numerical low conversion is part of routing.
            let replacement = 0x7f81;
            let fallback = b.upload_low(dtype, shape(&[]), &[replacement]).unwrap();
            let indices: Vec<_> = (0..256)
                .rev()
                .map(|i| i as u32)
                .chain([if replay == 0 { 256 } else { u32::MAX }])
                .collect();
            let index = b.upload_u32(shape(&[257]), &indices).unwrap();
            let out = program
                .run_typed(&[
                    (&input).into(),
                    (&mask).into(),
                    (&fallback).into(),
                    (&index).into(),
                ])
                .unwrap();
            let selected: Vec<_> = raw
                .iter()
                .enumerate()
                .map(|(i, &v)| if flags[i / 256] != 0 { v } else { replacement })
                .collect();
            let gather = |source: &[u16]| -> Vec<u16> {
                indices
                    .iter()
                    .flat_map(|&r| {
                        (0..256).map(move |c| {
                            if r < 256 {
                                source[r as usize * 256 + c]
                            } else {
                                0
                            }
                        })
                    })
                    .collect()
            };
            bits(&b, &out[0], &[256, 256], &selected);
            bits(&b, &out[1], &[257, 256], &gather(&raw));
            uints(&b, &out[2], &[], &[1]);
            bits(&b, &out[3], &[257, 256], &gather(&selected));
            uints(&b, &out[4], &[], &[1]);
            bits(&b, &out[5], &[256, 256], &raw);
            bits(&b, &out[6], &[257, 256], &gather(&raw));
            if let Some((previous, expected)) = &old {
                bits(&b, previous, &[257, 256], expected);
            }
            old = Some((out[1].clone(), gather(&raw)));
            traces(&program, replay + 1);
        }
        // Earlier results remain readable after closure/kernel release.
        drop(program);
        let (output, expected) = old.unwrap();
        bits(&b, &output, &[257, 256], &expected);
    }
}

#[test]
fn compiled_scalar_low_comparison_and_selection_use_pointer_metadata() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let a = graph.input_low(dtype, shape(&[])).unwrap();
        let c = graph.input_low(dtype, shape(&[])).unwrap();
        let less = graph.compare_low(a, c, CompareOp::Less).unwrap();
        let selected = graph.select_low(less, a, c).unwrap();
        let program = graph.compile(&[less, selected]).unwrap();
        for (replay, (left, right)) in [(0x8001, 1), (0, 0x8000), (0x7fff, 1)]
            .into_iter()
            .enumerate()
        {
            let a = b.upload_low(dtype, shape(&[]), &[left]).unwrap();
            let c = b.upload_low(dtype, shape(&[]), &[right]).unwrap();
            let out = program.run_typed(&[(&a).into(), (&c).into()]).unwrap();
            let flag = compare(decode(dtype, left), decode(dtype, right), CompareOp::Less);
            uints(&b, &out[0], &[], &[flag]);
            bits(&b, &out[1], &[], &[if flag != 0 { left } else { right }]);
            traces(&program, replay + 1);
        }
        // Keep a fresh lazy output: this has not been evaluated while its
        // input handles or compiled closure are alive.
        let pending = {
            let a = b.upload_low(dtype, shape(&[]), &[1]).unwrap();
            let c = b.upload_low(dtype, shape(&[]), &[0x8001]).unwrap();
            program.run_typed(&[(&a).into(), (&c).into()]).unwrap()
        };
        traces(&program, 4);
        drop(program);
        uints(&b, &pending[0], &[], &[0]);
        bits(&b, &pending[1], &[], &[0x8001]);
    }
}
