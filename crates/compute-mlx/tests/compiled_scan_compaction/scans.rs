use super::*;

#[test]
fn compiled_native_scans_wrap_and_rebind_strided_inputs_in_every_mode() {
    let Some(b) = backend() else { return };
    for count in [0, 1, 255, 256, 257, 513, 131077] {
        let dims = [2, count];
        let mut graph = b.program();
        let f = graph.input(shape(&dims)).unwrap();
        let u = graph.input_u32(shape(&dims)).unwrap();
        let mut values = vec![];
        for opt in options() {
            values.push(graph.scan(f, 1, opt).unwrap());
            values.push(graph.scan_u32(u, 1, opt).unwrap());
        }
        let program = graph.compile(&values).unwrap();
        for replay in 0..2 {
            let fs: Vec<_> = (0..2 * count)
                .map(|i| ((i + replay) % 7) as f32 * 0.25 - 0.75)
                .collect();
            let us: Vec<_> = (0..2 * count)
                .map(|i| match (i + replay) % 4 {
                    0 => u32::MAX,
                    1 => 1,
                    2 => 0x8000_0000,
                    _ => 17,
                })
                .collect();
            let (f, u) = if replay == 0 {
                (
                    b.upload_f32(shape(&dims), &fs).unwrap(),
                    b.upload_u32(shape(&dims), &us).unwrap(),
                )
            } else {
                let fp: Vec<_> = (0..count).flat_map(|i| [fs[i], fs[count + i]]).collect();
                let up: Vec<_> = (0..count).flat_map(|i| [us[i], us[count + i]]).collect();
                let f = b.upload_f32(shape(&[count, 2]), &fp).unwrap();
                let u = b.upload_u32(shape(&[count, 2]), &up).unwrap();
                (
                    b.permute(&f, &[1, 0]).unwrap(),
                    b.permute(&u, &[1, 0]).unwrap(),
                )
            };
            let out = program.run_typed(&[(&f).into(), (&u).into()]).unwrap();
            for (i, opt) in options().iter().enumerate() {
                assert_eq!(out[2 * i].shape(), &shape(&dims));
                assert_eq!(out[2 * i + 1].shape(), &shape(&dims));
                floats(
                    &b,
                    &out[2 * i],
                    &scan_reference(&fs, &dims, 1, *opt, 0., |a, c| a + c),
                );
                assert_eq!(
                    b.read_u32(out[2 * i + 1].as_tensor().unwrap()).unwrap(),
                    scan_reference(&us, &dims, 1, *opt, 0, u32::wrapping_add)
                );
            }
            traces(&program, replay + 1);
        }
    }
}

#[test]
fn compiled_low_scans_keep_f32_prefixes_and_round_each_final_prefix_once() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        for count in [1, 255, 256, 257, 513, 4097, 131077] {
            let mut graph = b.program();
            let x = graph.input_low(dtype, shape(&[count])).unwrap();
            let mut values = vec![];
            for opt in options() {
                values.push(graph.scan_low_f32(x, 0, opt).unwrap());
                values.push(graph.scan_low(x, 0, opt).unwrap());
            }
            let program = graph.compile(&values).unwrap();
            for replay in 0..2 {
                let data: Vec<_> = (0..count)
                    .map(|i| {
                        if count == 4097 || (i + replay) % 2 == 0 {
                            1.
                        } else {
                            -1.
                        }
                    })
                    .collect();
                let bits: Vec<_> = data.iter().map(|&x| encode(dtype, x)).collect();
                let x = b.upload_low(dtype, shape(&[count]), &bits).unwrap();
                let out = program.run_typed(&[(&x).into()]).unwrap();
                for (i, opt) in options().iter().enumerate() {
                    let expected = scan_reference(&data, &[count], 0, *opt, 0., |a, c| a + c);
                    floats(&b, &out[2 * i], &expected);
                    assert_eq!(
                        b.read_low_bits(out[2 * i + 1].as_low().unwrap()).unwrap(),
                        expected
                            .iter()
                            .map(|&x| encode(dtype, x))
                            .collect::<Vec<_>>()
                    );
                }
                if count == 4097 {
                    assert_eq!(
                        *b.read_f32(out[0].as_tensor().unwrap())
                            .unwrap()
                            .last()
                            .unwrap(),
                        4097.
                    );
                    assert_eq!(
                        *b.read_low_bits(out[1].as_low().unwrap())
                            .unwrap()
                            .last()
                            .unwrap(),
                        if dtype == LowDtype::F16 {
                            0x6c00
                        } else {
                            0x4580
                        }
                    );
                }
                traces(&program, replay + 1);
            }
        }
    }
}

#[test]
fn compiled_low_scans_restore_arbitrary_axis_coordinates_and_broadcast_strides() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let dims = [2, 513, 3];
        let mut graph = b.program();
        let input = graph.input_low(dtype, shape(&dims)).unwrap();
        let mut values = vec![];
        for opt in options() {
            values.push(graph.scan_low_f32(input, 1, opt).unwrap());
        }
        let program = graph.compile(&values).unwrap();
        for replay in 0..2 {
            let logical: Vec<_> = (0..2)
                .flat_map(|outer| {
                    (0..513).flat_map(move |j| {
                        (0..3).map(move |inner| {
                            if replay == 0 {
                                ((outer + j + inner) % 5) as f32 - 2.
                            } else {
                                inner as f32 - 1.
                            }
                        })
                    })
                })
                .collect();
            let input = if replay == 0 {
                let physical: Vec<_> = (0..513)
                    .flat_map(|j| {
                        [
                            logical[j * 3],
                            logical[j * 3 + 1],
                            logical[j * 3 + 2],
                            logical[(513 + j) * 3],
                            logical[(513 + j) * 3 + 1],
                            logical[(513 + j) * 3 + 2],
                        ]
                    })
                    .map(|x| encode(dtype, x))
                    .collect();
                let source = b.upload_low(dtype, shape(&[513, 2, 3]), &physical).unwrap();
                b.permute_low(&source, &[1, 0, 2]).unwrap()
            } else {
                let source = b
                    .upload_low(
                        dtype,
                        shape(&[1, 1, 3]),
                        &[encode(dtype, -1.), 0, encode(dtype, 1.)],
                    )
                    .unwrap();
                b.broadcast_low(&source, shape(&dims)).unwrap()
            };
            let out = program.run_typed(&[(&input).into()]).unwrap();
            for (i, opt) in options().iter().enumerate() {
                assert_eq!(out[i].shape(), &shape(&dims));
                floats(
                    &b,
                    &out[i],
                    &scan_reference(&logical, &dims, 1, *opt, 0., |a, c| a + c),
                );
            }
            traces(&program, replay + 1);
        }
    }
}
