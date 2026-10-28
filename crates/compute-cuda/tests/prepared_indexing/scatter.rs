use super::*;

fn combine(dtype: CudaDtype, op: ScatterOp, a: u32, b: u32) -> u32 {
    if op == ScatterOp::Replace {
        return b;
    }
    match dtype {
        CudaDtype::F32 => {
            let (a, b) = (f32::from_bits(a), f32::from_bits(b));
            match op {
                ScatterOp::Add => a + b,
                ScatterOp::Multiply => a * b,
                ScatterOp::Min => a.min(b),
                ScatterOp::Max => a.max(b),
                _ => unreachable!(),
            }
            .to_bits()
        }
        CudaDtype::U32 => match op {
            ScatterOp::Add => a.wrapping_add(b),
            ScatterOp::Multiply => a.wrapping_mul(b),
            ScatterOp::Min => a.min(b),
            ScatterOp::Max => a.max(b),
            _ => unreachable!(),
        },
        _ => {
            let key = |x: u32| {
                if x & 0x8000 != 0 {
                    (!x) & 0xffff
                } else {
                    x ^ 0x8000
                }
            };
            match op {
                ScatterOp::Min => {
                    if key(a) < key(b) {
                        a
                    } else {
                        b
                    }
                }
                ScatterOp::Max => {
                    if key(a) > key(b) {
                        a
                    } else {
                        b
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}
pub fn replay(rt: &CudaRuntime, dtype: CudaDtype) -> Result {
    let (base, updates, ops) = match dtype {
        CudaDtype::F32 => (
            [1f32, 2., 3., 4., 5.].map(f32::to_bits).to_vec(),
            [1f32, 2., 3., 4., 1., 2., 9.].map(f32::to_bits).to_vec(),
            vec![
                ScatterOp::Replace,
                ScatterOp::Add,
                ScatterOp::Multiply,
                ScatterOp::Min,
                ScatterOp::Max,
            ],
        ),
        CudaDtype::U32 => (
            vec![u32::MAX, u32::MAX - 1, 3, 4, u32::MAX],
            vec![2, 3, 4, 5, 2, 3, 9],
            vec![
                ScatterOp::Replace,
                ScatterOp::Add,
                ScatterOp::Multiply,
                ScatterOp::Min,
                ScatterOp::Max,
            ],
        ),
        _ => (
            vec![0, 0x8000, 2, 0x8001, 0x8002],
            vec![0, 0x8000, 0x8001, 1, 0x8003, 3, 0],
            vec![ScatterOp::Replace, ScatterOp::Min, ScatterOp::Max],
        ),
    };
    let mut padded = vec![0];
    padded.extend(&base);
    padded.push(0);
    let input = narrow(rt, &upload_bits(rt, dtype, &[1, 7], &padded)?, 1, 1, 5)?;
    let update_bits = updates.iter().flat_map(|&x| [0, x]).collect::<Vec<_>>();
    let updates_view = narrow(
        rt,
        &transpose(rt, &upload_bits(rt, dtype, &[7, 2], &update_bits)?)?,
        0,
        1,
        1,
    )?;
    let versions = [
        [0, 0, 1, 1, 4, 4, 8],
        [0, 8, 1, 8, 4, 8, 8],
        [u32::MAX; 7],
        [0, 0, 1, 1, 4, 4, 8],
    ];
    let indices = versions
        .iter()
        .map(|v| rt.upload_u32(shape(&[7]), v))
        .collect::<Result<Vec<_>>>()?;
    let mut graph = rt.program();
    let x = record_input(&mut graph, &input)?;
    let i = graph.input_u32(indices[0].layout().clone())?;
    let u = record_input(&mut graph, &updates_view)?;
    let mut outputs = Vec::new();
    for &op in &ops {
        let result = match dtype {
            CudaDtype::F32 => graph.scatter(x, i, u, op, 1)?,
            CudaDtype::U32 => graph.scatter_u32(x, i, u, op, 1)?,
            _ => graph.scatter_low(x, i, u, op, 1)?,
        };
        outputs.extend([result.values, result.invalid_count]);
    }
    let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
    let mut result = program.run_typed(&[
        input.as_input(),
        (&indices[0]).into(),
        updates_view.as_input(),
    ])?;
    for version in 0..4 {
        super::replay(
            &mut program,
            &[
                input.as_input(),
                (&indices[version]).into(),
                updates_view.as_input(),
            ],
            &mut result,
        )?;
        for (j, &op) in ops.iter().enumerate() {
            let mut expected = base.clone();
            for (&index, &update) in versions[version].iter().zip(&updates) {
                if index < 5 {
                    expected[index as usize] = combine(dtype, op, expected[index as usize], update);
                }
            }
            assert_eq!(
                read_bits(rt, &result[j * 2])?,
                expected,
                "{dtype:?} {op:?} replay {version}"
            );
            assert_eq!(
                rt.read_u32(result[j * 2 + 1].as_u32()?)?,
                [versions[version].iter().filter(|&&x| x >= 5).count() as u32]
            );
        }
    }
    // A long owner race must choose the greatest logical index, including raw
    // NaN payloads for low Replace. Later sparse indices have smaller winners.
    let n = 65539;
    let update_bits = (0..n)
        .map(|i| match dtype {
            CudaDtype::F32 => (i as f32).to_bits(),
            CudaDtype::U32 => u32::MAX - i as u32,
            _ => [0x7e01, 0xffff, 0x8000, 1][i % 4],
        })
        .collect::<Vec<_>>();
    let update = upload_bits(rt, dtype, &[n], &update_bits)?;
    let input = upload_bits(rt, dtype, &[5], &base)?;
    let versions = [
        (0..n)
            .map(|i| if i % 17 == 0 { 9 } else { (i % 5) as u32 })
            .collect::<Vec<_>>(),
        (0..n)
            .map(|i| if i < 5 { i as u32 } else { u32::MAX })
            .collect(),
        vec![u32::MAX; n],
    ];
    let indices = versions
        .iter()
        .map(|v| rt.upload_u32(shape(&[n]), v))
        .collect::<Result<Vec<_>>>()?;
    let mut graph = rt.program();
    let x = record_input(&mut graph, &input)?;
    let i = graph.input_u32(indices[0].layout().clone())?;
    let u = record_input(&mut graph, &update)?;
    let output = match dtype {
        CudaDtype::F32 => graph.scatter(x, i, u, ScatterOp::Replace, 0)?,
        CudaDtype::U32 => graph.scatter_u32(x, i, u, ScatterOp::Replace, 0)?,
        _ => graph.scatter_low(x, i, u, ScatterOp::Replace, 0)?,
    };
    let mut program = graph.prepare(
        &[output.values, output.invalid_count],
        CudaPrepareOptions::default(),
    )?;
    let mut result =
        program.run_typed(&[input.as_input(), (&indices[0]).into(), update.as_input()])?;
    for version in [0, 1, 2, 0] {
        super::replay(
            &mut program,
            &[
                input.as_input(),
                (&indices[version]).into(),
                update.as_input(),
            ],
            &mut result,
        )?;
        let mut expected = base.clone();
        for (j, &index) in versions[version].iter().enumerate() {
            if index < 5 {
                expected[index as usize] = update_bits[j];
            }
        }
        assert_eq!(
            read_bits(rt, &result[0])?,
            expected,
            "last-index Replace {dtype:?} replay {version}"
        );
        assert_eq!(
            rt.read_u32(result[1].as_u32()?)?,
            [versions[version].iter().filter(|&&x| x >= 5).count() as u32]
        );
    }
    // Invalid counts are meaningful even when no output/update slice exists.
    for dims in [[0, 5], [2, 0]] {
        let input = upload_bits(rt, dtype, &dims, &[])?;
        let update = upload_bits(rt, dtype, &[], &[0])?;
        let indices = rt.upload_u32(shape(&[3]), &[0, 4, u32::MAX])?;
        let mut graph = rt.program();
        let x = record_input(&mut graph, &input)?;
        let i = graph.input_u32(indices.layout().clone())?;
        let u = record_input(&mut graph, &update)?;
        let output = match dtype {
            CudaDtype::F32 => graph.scatter(x, i, u, ScatterOp::Replace, 1)?,
            CudaDtype::U32 => graph.scatter_u32(x, i, u, ScatterOp::Replace, 1)?,
            _ => graph.scatter_low(x, i, u, ScatterOp::Replace, 1)?,
        };
        let mut program = graph.prepare(
            &[output.values, output.invalid_count],
            CudaPrepareOptions::default(),
        )?;
        let inputs = [input.as_input(), (&indices).into(), update.as_input()];
        let mut result = program.run_typed(&inputs)?;
        for _ in 0..3 {
            super::replay(&mut program, &inputs, &mut result)?;
            assert!(read_bits(rt, &result[0])?.is_empty());
            assert_eq!(
                rt.read_u32(result[1].as_u32()?)?,
                [if dims[1] == 0 { 3 } else { 1 }]
            );
        }
    }
    Ok(())
}
pub fn low_arithmetic(rt: &CudaRuntime, dtype: LowDtype) -> Result {
    let (one, half_ulp, ulp) = match dtype {
        LowDtype::F16 => (0x3c00u16, 0x1000u16, 2f32.powi(-10)),
        LowDtype::Bf16 => (0x3f80, 0x3b80, 2f32.powi(-7)),
    };
    let base = rt.upload_low(dtype, shape(&[1]), &[one])?;
    let tiny = rt.upload_low(dtype, shape(&[]), &[half_ulp])?;
    let high = rt.upload_low(dtype, shape(&[1]), &[one + 1])?;
    let add_indices = [
        rt.upload_u32(shape(&[3]), &[0; 3])?,
        rt.upload_u32(shape(&[3]), &[1; 3])?,
    ];
    let mul_indices = [
        rt.upload_u32(shape(&[2]), &[0; 2])?,
        rt.upload_u32(shape(&[2]), &[1; 2])?,
    ];
    let mut graph = rt.program();
    let a = graph.input_low(dtype, base.layout().clone())?;
    let t = graph.input_low(dtype, tiny.layout().clone())?;
    let h = graph.input_low(dtype, high.layout().clone())?;
    let ai = graph.input_u32(add_indices[0].layout().clone())?;
    let mi = graph.input_u32(mul_indices[0].layout().clone())?;
    let af = graph.scatter_low_f32(a, ai, t, ScatterOp::Add, 0)?;
    let al = graph.scatter_low(a, ai, t, ScatterOp::Add, 0)?;
    let mf = graph.scatter_low_f32(h, mi, h, ScatterOp::Multiply, 0)?;
    let ml = graph.scatter_low(h, mi, h, ScatterOp::Multiply, 0)?;
    let mut program = graph.prepare(
        &[
            af.values,
            af.invalid_count,
            al.values,
            al.invalid_count,
            mf.values,
            mf.invalid_count,
            ml.values,
            ml.invalid_count,
        ],
        CudaPrepareOptions::default(),
    )?;
    let mut result = program.run_typed(&[
        (&base).into(),
        (&tiny).into(),
        (&high).into(),
        (&add_indices[0]).into(),
        (&mul_indices[0]).into(),
    ])?;
    for version in [0, 1, 0] {
        super::replay(
            &mut program,
            &[
                (&base).into(),
                (&tiny).into(),
                (&high).into(),
                (&add_indices[version]).into(),
                (&mul_indices[version]).into(),
            ],
            &mut result,
        )?;
        let add = if version == 0 { 1. + 3. * ulp / 2. } else { 1. };
        let mul = if version == 0 {
            ((1. + f64::from(ulp)).powi(3)) as f32
        } else {
            1. + ulp
        };
        assert_eq!(rt.read_f32(result[0].as_f32()?)?, [add]);
        assert_eq!(
            rt.read_low_bits(result[2].as_low()?)?,
            [if version == 0 { one + 2 } else { one }]
        );
        assert_eq!(rt.read_f32(result[4].as_f32()?)?, [mul]);
        assert_eq!(
            rt.read_low_bits(result[6].as_low()?)?,
            [if version == 0 { one + 3 } else { one + 1 }]
        );
        for index in [1, 3] {
            assert_eq!(
                rt.read_u32(result[index].as_u32()?)?,
                [if version == 0 { 0 } else { 3 }]
            );
        }
        for index in [5, 7] {
            assert_eq!(
                rt.read_u32(result[index].as_u32()?)?,
                [if version == 0 { 0 } else { 2 }]
            );
        }
    }
    Ok(())
}
