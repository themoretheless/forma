use super::*;

pub fn replay_and_empty(rt: &CudaRuntime, dtype: CudaDtype) -> Result {
    let raw: Vec<u32> = match dtype {
        CudaDtype::F32 => (0..10).map(|i| (i as f32 + 1.).to_bits()).collect(),
        CudaDtype::U32 => (0..10).map(|i| 0x8000_0000 + i).collect(),
        _ => vec![
            0x7fff, 0x8000, 1, 0xfc00, 0x7e01, 0, 0xffff, 0x7c00, 0x8001, 0x1234,
        ],
    };
    let input = transpose(rt, &upload_bits(rt, dtype, &[5, 2], &raw)?)?;
    let masks = [[1; 5], [0, u32::MAX, 0, 7, 0], [0; 5], [1; 5]];
    let index_bits = [
        [4, 0, 8, 1, 4, 0],
        [u32::MAX; 6],
        [0; 6],
        [2, 2, 4, 4, 1, 9],
    ];
    let mut mask_tensors = Vec::new();
    let mut index_tensors = Vec::new();
    for version in 0..4 {
        let mask_bits = masks[version]
            .iter()
            .flat_map(|&x| [91, x])
            .collect::<Vec<_>>();
        let mask = rt.upload_u32(shape(&[5, 2]), &mask_bits)?;
        let mask = rt.permute_u32(&mask, &[1, 0])?;
        mask_tensors.push(rt.narrow(&mask, 0, 1, 1)?);
        let indices = rt.upload_u32(shape(&[2, 3]), &index_bits[version])?;
        index_tensors.push(rt.permute_u32(&indices, &[1, 0])?);
    }
    let lookup = rt.upload_u32(
        shape(&[11]),
        &(0..11).map(|i| i * 17 + 3).collect::<Vec<_>>(),
    )?;
    let mut graph = rt.program();
    let x = record_input(&mut graph, &input)?;
    let indices = graph.input_u32(index_tensors[0].layout().clone())?;
    let mask = graph.input_u32(mask_tensors[0].layout().clone())?;
    let lookup_value = graph.input_u32(lookup.layout().clone())?;
    let gathered = match dtype {
        CudaDtype::F32 => graph.gather(x, indices, 1)?,
        CudaDtype::U32 => graph.gather_u32(x, indices, 1)?,
        _ => graph.gather_low(x, indices, 1)?,
    };
    let compacted = match dtype {
        CudaDtype::F32 => graph.compact(x, mask)?,
        CudaDtype::U32 => graph.compact_u32(x, mask)?,
        _ => graph.compact_low(x, mask)?,
    };
    let count_lookup = graph.gather_u32(lookup_value, compacted.count, 0)?;
    let chained = match dtype {
        CudaDtype::F32 => graph.gather(compacted.values, indices, 0)?,
        CudaDtype::U32 => graph.gather_u32(compacted.values, indices, 0)?,
        _ => graph.gather_low(compacted.values, indices, 0)?,
    };
    let mut program = graph.prepare(
        &[
            gathered.values,
            gathered.invalid_count,
            compacted.values,
            compacted.count,
            chained.values,
            chained.invalid_count,
            count_lookup.values,
        ],
        CudaPrepareOptions::default(),
    )?;
    let initial_inputs = [
        input.as_input(),
        (&index_tensors[0]).into(),
        (&mask_tensors[0]).into(),
        (&lookup).into(),
    ];
    let snapshot = program.run_typed(&initial_inputs)?;
    let snapshot_bits = read_bits(rt, &snapshot[0])?;
    let mut result = program.run_typed(&initial_inputs)?;
    for version in [0, 1, 2, 3, 0] {
        super::replay(
            &mut program,
            &[
                input.as_input(),
                (&index_tensors[version]).into(),
                (&mask_tensors[version]).into(),
                (&lookup).into(),
            ],
            &mut result,
        )?;
        let logical_indices = (0..6)
            .map(|i| index_bits[version][(i % 2) * 3 + i / 2])
            .collect::<Vec<_>>();
        let mut expected = Vec::new();
        let mut packed = Vec::new();
        for row in 0..2 {
            for &index in &logical_indices {
                expected.push(if index < 5 {
                    raw[index as usize * 2 + row]
                } else {
                    0
                });
            }
            for column in 0..5 {
                if masks[version][column] != 0 {
                    packed.push(raw[column * 2 + row]);
                }
            }
        }
        let count = packed.len() as u32;
        packed.resize(10, 0);
        assert_eq!(
            read_bits(rt, &result[0])?,
            expected,
            "{dtype:?} gather replay {version}"
        );
        assert_eq!(
            rt.read_u32(result[1].as_u32()?)?,
            [logical_indices.iter().filter(|&&i| i >= 5).count() as u32]
        );
        assert_eq!(
            read_bits(rt, &result[2])?,
            packed,
            "{dtype:?} zero tail replay {version}"
        );
        assert_eq!(rt.read_u32(result[3].as_u32()?)?, [count]);
        assert_eq!(
            read_bits(rt, &result[4])?,
            logical_indices
                .iter()
                .map(|&i| if i < 10 { packed[i as usize] } else { 0 })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            rt.read_u32(result[5].as_u32()?)?,
            [logical_indices.iter().filter(|&&i| i >= 10).count() as u32]
        );
        assert_eq!(
            rt.read_u32(result[6].as_u32()?)?,
            [count * 17 + 3],
            "resident count consumer"
        );
    }
    assert_eq!(
        read_bits(rt, &snapshot[0])?,
        snapshot_bits,
        "later replay changed an earlier owned result"
    );
    // Two scalar result slots sharing storage must be rejected before writes.
    let alias = result[3].clone();
    let previous = std::mem::replace(&mut result[1], alias);
    let before = read_bits(rt, &result[0])?;
    assert!(matches!(
        super::replay(&mut program, &initial_inputs, &mut result),
        Err(CudaError::SharedOutput)
    ));
    assert_eq!(read_bits(rt, &result[0])?, before);
    result[1] = previous;
    assert!(!program.is_poisoned());
    super::replay(&mut program, &initial_inputs, &mut result)?;

    for dims in [[2, 0], [0, 5]] {
        let input = upload_bits(rt, dtype, &dims, &[])?;
        let indices = rt.upload_u32(shape(&[3]), &[0, 4, u32::MAX])?;
        let mask = rt.upload_u32(shape(&[]), &[1])?;
        let mut graph = rt.program();
        let x = record_input(&mut graph, &input)?;
        let i = graph.input_u32(indices.layout().clone())?;
        let m = graph.input_u32(mask.layout().clone())?;
        let g = match dtype {
            CudaDtype::F32 => graph.gather(x, i, 1)?,
            CudaDtype::U32 => graph.gather_u32(x, i, 1)?,
            _ => graph.gather_low(x, i, 1)?,
        };
        let c = match dtype {
            CudaDtype::F32 => graph.compact(x, m)?,
            CudaDtype::U32 => graph.compact_u32(x, m)?,
            _ => graph.compact_low(x, m)?,
        };
        let mut program = graph.prepare(
            &[g.values, g.invalid_count, c.values, c.count],
            CudaPrepareOptions::default(),
        )?;
        let inputs = [input.as_input(), (&indices).into(), (&mask).into()];
        let mut result = program.run_typed(&inputs)?;
        for _ in 0..3 {
            super::replay(&mut program, &inputs, &mut result)?;
            assert_eq!(
                read_bits(rt, &result[0])?,
                vec![0; if dims[0] == 0 { 0 } else { 6 }]
            );
            assert_eq!(
                rt.read_u32(result[1].as_u32()?)?,
                [if dims[1] == 0 { 3 } else { 1 }]
            );
            assert!(read_bits(rt, &result[2])?.is_empty());
            assert_eq!(rt.read_u32(result[3].as_u32()?)?, [0]);
        }
    }
    Ok(())
}

pub fn large_compaction(rt: &CudaRuntime) -> Result {
    let n = 131077;
    let values = (0..n).map(|i| u32::MAX - i as u32).collect::<Vec<_>>();
    let input = rt.upload_u32(shape(&[n]), &values)?;
    let masks = [
        vec![u32::MAX; n],
        (0..n).map(|i| if i % 257 == 0 { 7 } else { 0 }).collect(),
        vec![0; n],
    ];
    let mask_tensors = masks
        .iter()
        .map(|bits| rt.upload_u32(shape(&[n]), bits))
        .collect::<Result<Vec<_>>>()?;
    let mut graph = rt.program();
    let x = graph.input_u32(input.layout().clone())?;
    let m = graph.input_u32(mask_tensors[0].layout().clone())?;
    let compact = graph.compact_u32(x, m)?;
    let mut program = graph.prepare(
        &[compact.values, compact.count],
        CudaPrepareOptions::default(),
    )?;
    let mut result = program.run_typed(&[(&input).into(), (&mask_tensors[0]).into()])?;
    for version in [0, 1, 2, 0] {
        super::replay(
            &mut program,
            &[(&input).into(), (&mask_tensors[version]).into()],
            &mut result,
        )?;
        let mut expected = values
            .iter()
            .zip(&masks[version])
            .filter_map(|(&value, &keep)| (keep != 0).then_some(value))
            .collect::<Vec<_>>();
        let count = expected.len() as u32;
        expected.resize(n, 0);
        assert_eq!(rt.read_u32(result[0].as_u32()?)?, expected);
        assert_eq!(rt.read_u32(result[1].as_u32()?)?, [count]);
    }
    Ok(())
}
