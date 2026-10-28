use super::*;
use compute_mlx::MlxProgramInput;

fn raw(i: usize, replay: usize) -> u16 {
    (i.wrapping_mul(251).wrapping_add(replay * 997)) as u16
}
fn unsigned(i: usize, replay: usize) -> u32 {
    0xffff_ff00u32
        .wrapping_add((i as u32).wrapping_mul(65537))
        .wrapping_add(replay as u32)
}
fn float(i: usize, replay: usize) -> f32 {
    (i % 251) as f32 * 0.25 + replay as f32
}
fn check_values(
    b: &MlxBackend,
    value: &MlxProgramOutput,
    kind: usize,
    selected: &[usize],
    capacity: usize,
    replay: usize,
) {
    assert_eq!(value.shape(), &shape(&[capacity]));
    match kind {
        0 => {
            let mut expected: Vec<_> = selected.iter().map(|&i| float(i, replay)).collect();
            expected.resize(capacity, 0.);
            floats(b, value, &expected);
        }
        1 => {
            let mut expected: Vec<_> = selected.iter().map(|&i| unsigned(i, replay)).collect();
            expected.resize(capacity, 0);
            assert_eq!(b.read_u32(value.as_tensor().unwrap()).unwrap(), expected);
        }
        _ => {
            let mut expected: Vec<_> = selected.iter().map(|&i| raw(i, replay)).collect();
            expected.resize(capacity, 0);
            assert_eq!(b.read_low_bits(value.as_low().unwrap()).unwrap(), expected);
        }
    }
}

#[test]
fn compiled_compaction_replays_all_sparse_none_all_and_consumes_count_on_device() {
    let Some(b) = backend() else { return };
    let columns = 131077;
    let capacity = 2 * columns;
    for kind in 0..4 {
        let mut graph = b.program();
        let input = match kind {
            0 => graph.input(shape(&[2, columns])).unwrap(),
            1 => graph.input_u32(shape(&[2, columns])).unwrap(),
            _ => graph
                .input_low(
                    if kind == 2 {
                        LowDtype::F16
                    } else {
                        LowDtype::Bf16
                    },
                    shape(&[2, columns]),
                )
                .unwrap(),
        };
        let mask = graph.input_u32(shape(&[2, columns])).unwrap();
        let lookup = graph.input_u32(shape(&[capacity + 1])).unwrap();
        let pair = match kind {
            0 => graph.compact(input, mask).unwrap(),
            1 => graph.compact_u32(input, mask).unwrap(),
            _ => graph.compact_low(input, mask).unwrap(),
        };
        let count_use = graph.gather_u32(lookup, pair.count, 0).unwrap();
        let program = graph
            .compile(&[
                pair.values,
                pair.count,
                count_use.values,
                count_use.invalid_count,
            ])
            .unwrap();
        let lookup_data: Vec<_> = (0..=capacity).map(|i| (i as u32) * 3 + 7).collect();
        let lookup = b.upload_u32(shape(&[capacity + 1]), &lookup_data).unwrap();
        let mut first = None;
        for replay in 0..4 {
            let selected: Vec<_> = (0..capacity)
                .filter(|&i| match replay {
                    0 | 3 => true,
                    1 => i % 4093 == 0 || i == capacity - 1,
                    _ => false,
                })
                .collect();
            let masks: Vec<_> = (0..capacity)
                .map(|i| {
                    if selected.binary_search(&i).is_ok() {
                        if i % 2 == 0 { u32::MAX } else { 0x8000_0000 }
                    } else {
                        0
                    }
                })
                .collect();
            let order: Vec<_> = if replay % 2 == 0 {
                (0..capacity).collect()
            } else {
                (0..columns).flat_map(|i| [i, columns + i]).collect()
            };
            let source_shape = if replay % 2 == 0 {
                shape(&[2, columns])
            } else {
                shape(&[columns, 2])
            };
            let physical_mask: Vec<_> = order.iter().map(|&i| masks[i]).collect();
            let mask = b.upload_u32(source_shape.clone(), &physical_mask).unwrap();
            let mask = if replay % 2 == 0 {
                mask
            } else {
                b.permute(&mask, &[1, 0]).unwrap()
            };
            let out = match kind {
                0 => {
                    let data: Vec<_> = order.iter().map(|&i| float(i, replay)).collect();
                    let input = b.upload_f32(source_shape, &data).unwrap();
                    let input = if replay % 2 == 0 {
                        input
                    } else {
                        b.permute(&input, &[1, 0]).unwrap()
                    };
                    program
                        .run_typed(&[
                            MlxProgramInput::Tensor(&input),
                            (&mask).into(),
                            (&lookup).into(),
                        ])
                        .unwrap()
                }
                1 => {
                    let data: Vec<_> = order.iter().map(|&i| unsigned(i, replay)).collect();
                    let input = b.upload_u32(source_shape, &data).unwrap();
                    let input = if replay % 2 == 0 {
                        input
                    } else {
                        b.permute(&input, &[1, 0]).unwrap()
                    };
                    program
                        .run_typed(&[(&input).into(), (&mask).into(), (&lookup).into()])
                        .unwrap()
                }
                _ => {
                    let dtype = if kind == 2 {
                        LowDtype::F16
                    } else {
                        LowDtype::Bf16
                    };
                    let data: Vec<_> = order.iter().map(|&i| raw(i, replay)).collect();
                    let input = b.upload_low(dtype, source_shape, &data).unwrap();
                    let input = if replay % 2 == 0 {
                        input
                    } else {
                        b.permute_low(&input, &[1, 0]).unwrap()
                    };
                    program
                        .run_typed(&[(&input).into(), (&mask).into(), (&lookup).into()])
                        .unwrap()
                }
            };
            check_values(&b, &out[0], kind, &selected, capacity, replay);
            for index in [1, 2, 3] {
                assert_eq!(out[index].shape(), &shape(&[]));
            }
            assert_eq!(
                b.read_u32(out[1].as_tensor().unwrap()).unwrap(),
                [selected.len() as u32]
            );
            assert_eq!(
                b.read_u32(out[2].as_tensor().unwrap()).unwrap(),
                [lookup_data[selected.len()]]
            );
            assert_eq!(b.read_u32(out[3].as_tensor().unwrap()).unwrap(), [0]);
            traces(&program, replay + 1);
            if replay == 0 {
                first = Some(out);
            }
        }
        check_values(
            &b,
            &first.unwrap()[0],
            kind,
            &(0..capacity).collect::<Vec<_>>(),
            capacity,
            0,
        );
    }
}

#[test]
fn compiled_compaction_broadcast_masks_preserve_exact_low_payloads_and_scalar_shapes() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[2, 3])).unwrap();
        let m = graph.input_u32(shape(&[1, 3])).unwrap();
        let pair = graph.compact_low(x, m).unwrap();
        let program = graph.compile(&[pair.values, pair.count]).unwrap();
        // Includes a NaN payload, both zero signs and signed subnormal payloads.
        let bits = [0x7fc1, 0x8000, 1, 0xffc3, 0, 0x8001];
        let input = b.upload_low(dtype, shape(&[2, 3]), &bits).unwrap();
        for (replay, mask) in [[1, 0, u32::MAX], [0, 0, 0], [1, 1, 1]].iter().enumerate() {
            let m = b.upload_u32(shape(&[1, 3]), mask).unwrap();
            let out = program.run_typed(&[(&input).into(), (&m).into()]).unwrap();
            let mut expected: Vec<_> = bits
                .iter()
                .enumerate()
                .filter(|(i, _)| mask[i % 3] != 0)
                .map(|(_, x)| *x)
                .collect();
            let count = expected.len();
            expected.resize(6, 0);
            assert_eq!(b.read_low_bits(out[0].as_low().unwrap()).unwrap(), expected);
            assert_eq!(
                b.read_u32(out[1].as_tensor().unwrap()).unwrap(),
                [count as u32]
            );
            traces(&program, replay + 1);
        }
        let mut graph = b.program();
        let x = graph.input_low(dtype, shape(&[])).unwrap();
        let m = graph.input_u32(shape(&[])).unwrap();
        let pair = graph.compact_low(x, m).unwrap();
        let program = graph.compile(&[pair.values, pair.count]).unwrap();
        for (replay, mask) in [u32::MAX, 0].iter().enumerate() {
            let input = b.upload_low(dtype, shape(&[]), &[0xffd1]).unwrap();
            let mask = b.upload_u32(shape(&[]), &[*mask]).unwrap();
            let out = program
                .run_typed(&[(&input).into(), (&mask).into()])
                .unwrap();
            assert_eq!(out[0].shape(), &shape(&[1]));
            assert_eq!(out[1].shape(), &shape(&[]));
            assert_eq!(
                b.read_low_bits(out[0].as_low().unwrap()).unwrap(),
                [if replay == 0 { 0xffd1 } else { 0 }]
            );
            assert_eq!(
                b.read_u32(out[1].as_tensor().unwrap()).unwrap(),
                [u32::from(replay == 0)]
            );
            traces(&program, replay + 1);
        }
    }
}
