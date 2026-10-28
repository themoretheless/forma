use super::*;

#[test]
fn compiled_f32_u32_branching_and_gather_use_changed_strided_resident_values() {
    let Some(b) = backend() else { return };
    for unsigned in [false, true] {
        let mut graph = b.program();
        let x = if unsigned {
            graph.input_u32(shape(&[3, 4]))
        } else {
            graph.input(shape(&[3, 4]))
        }
        .unwrap();
        let row = if unsigned {
            graph.input_u32(shape(&[4]))
        } else {
            graph.input(shape(&[4]))
        }
        .unwrap();
        let mask = graph.input_u32(shape(&[3, 1])).unwrap();
        let index = graph.input_u32(shape(&[2, 2])).unwrap();
        let scalar = graph.input_u32(shape(&[])).unwrap();
        let table = graph.input_u32(shape(&[5])).unwrap();
        let mut values: Vec<_> = OPS
            .into_iter()
            .map(|op| {
                if unsigned {
                    graph.compare_u32(x, row, op)
                } else {
                    graph.compare(x, row, op)
                }
                .unwrap()
            })
            .collect();
        let selected = if unsigned {
            graph.select_u32(mask, x, row)
        } else {
            graph.select(mask, x, row)
        }
        .unwrap();
        let gathered = if unsigned {
            graph.gather_u32(selected, index, 1)
        } else {
            graph.gather(selected, index, 1)
        }
        .unwrap();
        let outer = if unsigned {
            graph.gather_u32(x, index, 0)
        } else {
            graph.gather(x, index, 0)
        }
        .unwrap();
        let single = if unsigned {
            graph.gather_u32(x, scalar, 1)
        } else {
            graph.gather(x, scalar, 1)
        }
        .unwrap();
        let count_consumer = graph.gather_u32(table, gathered.invalid_count, 0).unwrap();
        values.extend([
            selected,
            gathered.values,
            gathered.invalid_count,
            outer.values,
            outer.invalid_count,
            single.values,
            single.invalid_count,
            count_consumer.values,
        ]);
        let program = graph.compile(&values).unwrap();
        let mut old = None;
        for replay in 0..2 {
            let raw: Vec<u32> = (0..12)
                .map(|i| {
                    if unsigned {
                        [0, u32::MAX, 0x8000_0000, 17][(i + replay) % 4].wrapping_add(i as u32)
                    } else {
                        ((i as f32 - 5.0) * 0.25 + replay as f32).to_bits()
                    }
                })
                .collect();
            let rows: Vec<u32> = if unsigned {
                vec![u32::MAX, 1, 0x8000_0000, 19]
            } else {
                [-1.0f32, 0.0, 1.0, 2.0].map(f32::to_bits).to_vec()
            };
            let physical = if replay == 0 {
                raw.clone()
            } else {
                (0..4)
                    .flat_map(|c| {
                        let raw = &raw;
                        (0..3).map(move |r| raw[r * 4 + c])
                    })
                    .collect()
            };
            let upload = |dims: &[usize], data: &[u32]| {
                if unsigned {
                    b.upload_u32(shape(dims), data)
                } else {
                    b.upload_f32(
                        shape(dims),
                        &data.iter().copied().map(f32::from_bits).collect::<Vec<_>>(),
                    )
                }
                .unwrap()
            };
            let source = upload(if replay == 0 { &[3, 4] } else { &[4, 3] }, &physical);
            let input = if replay == 0 {
                source
            } else {
                b.permute(&source, &[1, 0]).unwrap()
            };
            let row = upload(&[4], &rows);
            let flags = if replay == 0 {
                [0, 2, u32::MAX]
            } else {
                [1, 0, 3]
            };
            let mask = b.upload_u32(shape(&[3, 1]), &flags).unwrap();
            let indices = if replay == 0 {
                [3, 0, u32::MAX, 9]
            } else {
                [1, 2, 0, 1]
            };
            let physical_index = [indices[0], indices[2], indices[1], indices[3]];
            let source_index = b.upload_u32(shape(&[2, 2]), &physical_index).unwrap();
            let index = b.permute(&source_index, &[1, 0]).unwrap();
            let scalar_value = if replay == 0 { 2 } else { u32::MAX };
            let scalar = b.upload_u32(shape(&[]), &[scalar_value]).unwrap();
            let table = b
                .upload_u32(shape(&[5]), &[100, 101, 102, 103, 104])
                .unwrap();
            let out = program
                .run_typed(&[
                    (&input).into(),
                    (&row).into(),
                    (&mask).into(),
                    (&index).into(),
                    (&scalar).into(),
                    (&table).into(),
                ])
                .unwrap();
            for (value, op) in out.iter().zip(OPS) {
                let expected: Vec<_> = raw
                    .iter()
                    .enumerate()
                    .map(|(i, &a)| {
                        if unsigned {
                            compare(a, rows[i % 4], op)
                        } else {
                            compare(f32::from_bits(a), f32::from_bits(rows[i % 4]), op)
                        }
                    })
                    .collect();
                uints(&b, value, &[3, 4], &expected);
            }
            let selected: Vec<_> = raw
                .iter()
                .enumerate()
                .map(|(i, &x)| if flags[i / 4] != 0 { x } else { rows[i % 4] })
                .collect();
            let inner: Vec<_> = (0..3)
                .flat_map(|r| {
                    let selected = &selected;
                    indices.into_iter().map(move |c| {
                        if c < 4 {
                            selected[r * 4 + c as usize]
                        } else {
                            0
                        }
                    })
                })
                .collect();
            let outer: Vec<_> = indices
                .into_iter()
                .flat_map(|r| {
                    let raw = &raw;
                    (0..4).map(move |c| if r < 3 { raw[r as usize * 4 + c] } else { 0 })
                })
                .collect();
            let single: Vec<_> = (0..3)
                .map(|r| {
                    if scalar_value < 4 {
                        raw[r * 4 + scalar_value as usize]
                    } else {
                        0
                    }
                })
                .collect();
            let read = |v: &MlxProgramOutput| {
                if unsigned {
                    b.read_u32(v.as_tensor().unwrap()).unwrap()
                } else {
                    b.read_f32(v.as_tensor().unwrap())
                        .unwrap()
                        .into_iter()
                        .map(f32::to_bits)
                        .collect()
                }
            };
            assert_eq!(out[7].shape(), &shape(&[3, 2, 2]));
            assert_eq!(out[9].shape(), &shape(&[2, 2, 4]));
            assert_eq!(out[11].shape(), &shape(&[3]));
            for (i, expected) in [(6, selected), (7, inner.clone()), (9, outer), (11, single)] {
                assert_eq!(read(&out[i]), expected);
            }
            let invalid = indices.iter().filter(|&&v| v >= 4).count() as u32;
            uints(&b, &out[8], &[], &[invalid]);
            uints(
                &b,
                &out[10],
                &[],
                &[indices.iter().filter(|&&v| v >= 3).count() as u32],
            );
            uints(&b, &out[12], &[], &[u32::from(scalar_value >= 4)]);
            uints(&b, &out[13], &[], &[100 + invalid]);
            if let Some((output, expected)) = &old {
                assert_eq!(read(output), *expected);
            }
            old = Some((out[7].clone(), inner));
            traces(&program, replay + 1);
        }
    }
}
