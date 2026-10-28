use super::*;

pub fn arithmetic_views_and_replay(rt: &CudaRuntime) -> Result {
    let backing = rt.upload_u32(
        shape(&[3, 4]),
        &[99, u32::MAX, 0, 99, 99, 2, 0x8000_0000, 99, 99, 3, 7, 99],
    )?;
    let input = rt.permute_u32(&rt.narrow(&backing, 1, 1, 2)?, &[1, 0])?;
    let mut scalar = rt.upload_u32(shape(&[]), &[2])?;
    let mut graph = rt.program();
    let x = graph.input_u32(input.layout().clone())?;
    let y = graph.input_u32(scalar.layout().clone())?;
    let ops = [
        BinaryOp::Add,
        BinaryOp::Subtract,
        BinaryOp::Multiply,
        BinaryOp::Min,
        BinaryOp::Max,
    ];
    let mut outputs = Vec::new();
    for op in ops {
        outputs.push(graph.binary_u32(x, y, op)?);
    }
    assert!(graph.binary_u32(x, y, BinaryOp::Divide).is_err());
    let mask = graph.compare_u32(x, y, CompareOp::Greater)?;
    outputs.push(mask);
    outputs.push(graph.select_u32(mask, x, y)?);
    let flat = graph.reshape(x, shape(&[6]))?;
    outputs.extend([flat, flat]);
    let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
    let stats = program.stats();
    let values = [u32::MAX, 2, 3, 0, 0x8000_0000, 7];
    let mut retained = None;
    for scalar_value in [2, 3, 0xffff_ffff] {
        rt.write_u32(&mut scalar, &[scalar_value])?;
        let mut actual = program.run_typed(&[(&input).into(), (&scalar).into()])?;
        for (index, op) in ops.into_iter().enumerate() {
            let expected: Vec<_> = values
                .iter()
                .map(|&x| match op {
                    BinaryOp::Add => x.wrapping_add(scalar_value),
                    BinaryOp::Subtract => x.wrapping_sub(scalar_value),
                    BinaryOp::Multiply => x.wrapping_mul(scalar_value),
                    BinaryOp::Min => x.min(scalar_value),
                    BinaryOp::Max => x.max(scalar_value),
                    _ => unreachable!(),
                })
                .collect();
            assert_eq!(rt.read_u32(actual[index].as_u32()?)?, expected, "{op:?}");
            assert_eq!(rt.read_u32(&rt.binary_u32(op, &input, &scalar)?)?, expected);
        }
        assert_eq!(
            rt.read_u32(actual[5].as_u32()?)?,
            values.map(|x| u32::from(x > scalar_value))
        );
        assert_eq!(
            rt.read_u32(actual[6].as_u32()?)?,
            values.map(|x| x.max(scalar_value))
        );
        assert_eq!(rt.read_u32(actual[7].as_u32()?)?, values);
        let mut independent = actual.pop().unwrap().into_u32()?;
        // Duplicate terminal references must still own independent storage.
        rt.write_u32(&mut independent, &[11; 6])?;
        assert_eq!(rt.read_u32(actual[7].as_u32()?)?, values);
        drop(actual);
        if scalar_value == 2 {
            retained = Some(independent);
        }
    }
    assert_eq!(program.stats(), stats);
    assert_eq!(rt.read_u32(&retained.unwrap())?, [11; 6]);
    assert!(rt.binary_u32(BinaryOp::Divide, &input, &scalar).is_err());
    Ok(())
}

pub fn reductions(rt: &CudaRuntime) -> Result {
    let values: Vec<_> = (0..8197)
        .map(|i| {
            if i % 3 == 0 {
                u32::MAX
            } else {
                (i % 17) as u32
            }
        })
        .collect();
    let input = rt.upload_u32(shape(&[values.len()]), &values)?;
    let empty = rt.upload_u32(shape(&[4, 0]), &[])?;
    let mut graph = rt.program();
    let x = graph.input_u32(input.layout().clone())?;
    let e = graph.input_u32(empty.layout().clone())?;
    let mut outputs = Vec::new();
    for op in [
        ReduceOp::Sum,
        ReduceOp::Product,
        ReduceOp::Min,
        ReduceOp::Max,
    ] {
        outputs.push(graph.reduce_u32(x, op, &[0], false)?);
    }
    outputs.push(graph.reduce_u32(e, ReduceOp::Sum, &[1], false)?);
    outputs.push(graph.reduce_u32(e, ReduceOp::Product, &[1], false)?);
    outputs.push(graph.reduce_u32(x, ReduceOp::Sum, &[], false)?);
    let mut program = graph.prepare(&outputs, CudaPrepareOptions::default())?;
    for _ in 0..3 {
        let actual = program.run_typed(&[(&input).into(), (&empty).into()])?;
        for (i, expected) in [
            values.iter().fold(0u32, |a, &b| a.wrapping_add(b)),
            values.iter().fold(1u32, |a, &b| a.wrapping_mul(b)),
            *values.iter().min().unwrap(),
            *values.iter().max().unwrap(),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(rt.read_u32(actual[i].as_u32()?)?, [expected]);
        }
        assert_eq!(rt.read_u32(actual[4].as_u32()?)?, [0; 4]);
        assert_eq!(rt.read_u32(actual[5].as_u32()?)?, [1; 4]);
        assert_eq!(rt.read_u32(actual[6].as_u32()?)?, values);
    }
    Ok(())
}
