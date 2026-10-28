use super::*;

#[test]
fn wide_scatter_replays_all_ops_with_changed_unsigned_indices_and_strides() {
    let Some(b) = backend() else { return };
    for unsigned in [false, true] {
        let mut g = b.program();
        let base = if unsigned {
            g.input_u32(shape(&[2, 5]))
        } else {
            g.input(shape(&[2, 5]))
        }
        .unwrap();
        let idx = g.input_u32(shape(&[2, 3])).unwrap();
        let updates = if unsigned {
            g.input_u32(shape(&[2, 2, 3]))
        } else {
            g.input(shape(&[2, 2, 3]))
        }
        .unwrap();
        let mut outputs = vec![];
        for op in OPS {
            let r = if unsigned {
                g.scatter_u32(base, idx, updates, op, 1)
            } else {
                g.scatter(base, idx, updates, op, 1)
            }
            .unwrap();
            outputs.extend([r.values, r.invalid_count]);
        }
        let p = g.compile(&outputs).unwrap();
        for run in 0..4 {
            let iv = index_values(run);
            let ix = indices(&b, &iv, run % 2 == 1);
            let bad = iv.iter().filter(|&&i| i >= 5).count() as u32;
            let floats: Vec<f32> = (0..10).map(|i| (i + 1) as f32 * 0.5).collect();
            let fu: Vec<f32> = (0..12).map(|i| [0.5, 1., 2.][(i + run) % 3]).collect();
            let ints: Vec<u32> = (0..10).map(|i| u32::MAX - i).collect();
            let iu: Vec<u32> = (0..12)
                .map(|i| [2, 0x0100_0001, 0x8000_0001][(i + run) % 3])
                .collect();
            let x = if unsigned {
                b.upload_u32(shape(&[2, 5]), &ints)
            } else {
                b.upload_f32(shape(&[2, 5]), &floats)
            }
            .unwrap();
            let u = if unsigned {
                b.upload_u32(shape(&[2, 2, 3]), &iu)
            } else {
                b.upload_f32(shape(&[2, 2, 3]), &fu)
            }
            .unwrap();
            let u = if run % 2 == 1 {
                let t = b.permute(&u, &[2, 1, 0]).unwrap();
                b.permute(&b.materialize(&t).unwrap(), &[2, 1, 0]).unwrap()
            } else {
                u
            };
            let result = p
                .run_typed(&[(&x).into(), (&ix).into(), (&u).into()])
                .unwrap();
            for (j, op) in OPS.into_iter().enumerate() {
                assert_eq!(result[j * 2].shape(), &shape(&[2, 5]));
                if unsigned {
                    assert_eq!(
                        b.read_u32(result[j * 2].as_tensor().unwrap()).unwrap(),
                        reference(&ints, &iu, &iv, op, unsigned_fold)
                    );
                } else {
                    assert_eq!(
                        b.read_f32(result[j * 2].as_tensor().unwrap()).unwrap(),
                        reference(&floats, &fu, &iv, op, float_fold)
                    );
                }
                assert_eq!(
                    b.read_u32(result[j * 2 + 1].as_tensor().unwrap()).unwrap(),
                    [bad]
                );
            }
            traces(&p, run + 1);
        }
    }
}

#[test]
fn low_atomic_lists_and_owners_reset_on_all_few_none_all_replay() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let mut g = b.program();
        let x = g.input_low(dtype, shape(&[2, 5])).unwrap();
        let ix = g.input_u32(shape(&[2, 3])).unwrap();
        let u = g.input_low(dtype, shape(&[2, 2, 3])).unwrap();
        let mut outputs = vec![];
        for op in OPS {
            let low = g.scatter_low(x, ix, u, op, 1).unwrap();
            let wide = g.scatter_low_f32(x, ix, u, op, 1).unwrap();
            outputs.extend([
                low.values,
                wide.values,
                low.invalid_count,
                wide.invalid_count,
            ]);
        }
        let p = g.compile(&outputs).unwrap();
        for run in 0..4 {
            let iv = index_values(run);
            let indices = indices(&b, &iv, run % 2 == 1);
            let base: Vec<f32> = (0..10).map(|i| (i + 1) as f32 * 0.5).collect();
            let updates: Vec<f32> = (0..12).map(|i| [0.5, 1., 2.][(i + run) % 3]).collect();
            let x = low_view(
                &b,
                dtype,
                &[2, 5],
                &base.iter().map(|&x| encode(dtype, x)).collect::<Vec<_>>(),
                run % 2 == 1,
            );
            let u = low_view(
                &b,
                dtype,
                &[2, 2, 3],
                &updates
                    .iter()
                    .map(|&x| encode(dtype, x))
                    .collect::<Vec<_>>(),
                run % 2 == 1,
            );
            let result = p
                .run_typed(&[(&x).into(), (&indices).into(), (&u).into()])
                .unwrap();
            let bad = iv.iter().filter(|&&i| i >= 5).count() as u32;
            for (j, op) in OPS.into_iter().enumerate() {
                let expected = reference(&base, &updates, &iv, op, float_fold);
                assert_eq!(
                    b.read_low_bits(result[4 * j].as_low().unwrap()).unwrap(),
                    expected
                        .iter()
                        .map(|&x| encode(dtype, x))
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    b.read_f32(result[4 * j + 1].as_tensor().unwrap()).unwrap(),
                    expected
                );
                assert_eq!(
                    b.read_u32(result[4 * j + 2].as_tensor().unwrap()).unwrap(),
                    [bad]
                );
                assert_eq!(
                    b.read_u32(result[4 * j + 3].as_tensor().unwrap()).unwrap(),
                    [bad]
                );
            }
            traces(&p, run + 1);
        }
    }
}

#[test]
fn low_fold_contention_rounds_only_after_f32_accumulation_and_retains_lazy_inputs() {
    let Some(b) = backend() else { return };
    for dtype in [LowDtype::F16, LowDtype::Bf16] {
        let n = 65539;
        let mut g = b.program();
        let x = g.input_low(dtype, shape(&[17])).unwrap();
        let ix = g.input_u32(shape(&[n])).unwrap();
        let u = g.input_low(dtype, shape(&[])).unwrap();
        let a = g.scatter_low_f32(x, ix, u, ScatterOp::Add, 0).unwrap();
        let z = g.scatter_low(x, ix, u, ScatterOp::Add, 0).unwrap();
        let p = g.compile(&[a.values, z.values, a.invalid_count]).unwrap();
        let raw = b
            .upload_low(dtype, shape(&[17]), &[encode(dtype, 0.5); 17])
            .unwrap();
        let idx: Vec<_> = (0..n)
            .map(|i| {
                if i % 11 == 0 {
                    u32::MAX
                } else {
                    (i % 17) as u32
                }
            })
            .collect();
        let ix = b.upload_u32(shape(&[n]), &idx).unwrap();
        let u = b
            .upload_low(dtype, shape(&[]), &[encode(dtype, 0.125)])
            .unwrap();
        let result = p
            .run_typed(&[(&raw).into(), (&ix).into(), (&u).into()])
            .unwrap();
        drop(p);
        drop(raw);
        drop(ix);
        drop(u);
        let mut expected = vec![0.5; 17];
        let mut invalid = 0;
        for i in idx {
            if i < 17 {
                expected[i as usize] += 0.125
            } else {
                invalid += 1
            }
        }
        assert_eq!(
            b.read_f32(result[0].as_tensor().unwrap()).unwrap(),
            expected
        );
        assert_eq!(
            b.read_low_bits(result[1].as_low().unwrap()).unwrap(),
            expected
                .iter()
                .map(|&x| encode(dtype, x))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            b.read_u32(result[2].as_tensor().unwrap()).unwrap(),
            [invalid]
        );
        assert!(
            expected
                .iter()
                .any(|&x| decode(dtype, encode(dtype, x)) != x)
        );
    }
}
