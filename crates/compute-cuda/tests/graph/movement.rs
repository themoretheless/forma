use super::*;

pub fn replay_and_counters(rt: &CudaRuntime, dtype: CudaDtype) -> Result {
    let word = |i: usize, version: usize| -> u32 {
        match dtype {
            CudaDtype::F32 => ((i as f32 - version as f32) * 0.25 - 1.).to_bits(),
            CudaDtype::U32 => u32::MAX.wrapping_sub((i + version * 3) as u32),
            _ => u32::from(
                [0x8000u16, 1, 0x8001, 0x7fc1, 0xffc3, 0, 0x1234, 0x7fff][(i + version) % 8],
            ),
        }
    };
    let source = upload(
        rt,
        dtype,
        &[7],
        &(0..7).map(|i| word(i, 0)).collect::<Vec<_>>(),
    )?;
    let narrow = |x: &CudaProgramOutput| -> Result<CudaProgramOutput> {
        Ok(match x {
            CudaProgramOutput::F32(t) => CudaProgramOutput::F32(rt.narrow(t, 0, 1, 5)?),
            CudaProgramOutput::U32(t) => CudaProgramOutput::U32(rt.narrow(t, 0, 1, 5)?),
            CudaProgramOutput::Low(t) => CudaProgramOutput::Low(rt.narrow_low(t, 0, 1, 5)?),
        })
    };
    let input = narrow(&source)?;
    let update = upload(
        rt,
        dtype,
        &[7],
        &(0..7).map(|i| word(i, 1)).collect::<Vec<_>>(),
    )?;
    let mask = CudaProgramOutput::U32(rt.upload_u32(shape(&[5]), &[1; 5])?);
    let index = CudaProgramOutput::U32(rt.upload_u32(shape(&[7]), &[2, 2, 0, u32::MAX, 5, 4, 1])?);
    let mut b = rt.program();
    let x = record(&mut b, &input)?;
    let u = record(&mut b, &update)?;
    let m = record(&mut b, &mask)?;
    let i = record(&mut b, &index)?;
    let (g, c, s) = match dtype {
        CudaDtype::F32 => (
            b.gather(x, i, 0)?,
            b.compact(x, m)?,
            b.scatter(x, i, u, ScatterOp::Replace, 0)?,
        ),
        CudaDtype::U32 => (
            b.gather_u32(x, i, 0)?,
            b.compact_u32(x, m)?,
            b.scatter_u32(x, i, u, ScatterOp::Replace, 0)?,
        ),
        _ => (
            b.gather_low(x, i, 0)?,
            b.compact_low(x, m)?,
            b.scatter_low(x, i, u, ScatterOp::Replace, 0)?,
        ),
    };
    let prefix = b.scan_u32(
        m,
        0,
        ScanOptions {
            inclusive: false,
            reverse: true,
        },
    )?;
    let count_use = match dtype {
        CudaDtype::F32 => b.gather(c.values, c.count, 0)?,
        CudaDtype::U32 => b.gather_u32(c.values, c.count, 0)?,
        _ => b.gather_low(c.values, c.count, 0)?,
    };
    let values = [
        g.values,
        g.invalid_count,
        c.values,
        c.count,
        s.values,
        s.invalid_count,
        prefix,
        count_use.values,
        count_use.invalid_count,
        x,
        s.values,
    ];
    let mut graph = b
        .prepare(&values, CudaPrepareOptions::default())?
        .capture_owned(CudaCaptureOptions::default())?;
    assert!(graph.stats().program.memset_calls >= 4);
    assert_eq!(graph.stats().workspace_bytes, 0);
    let first = graph.run_typed(&[
        input.as_input(),
        update.as_input(),
        mask.as_input(),
        index.as_input(),
    ])?;
    let first_data = read(rt, &first[4])?;
    let mut out = graph.run_typed(&[
        input.as_input(),
        update.as_input(),
        mask.as_input(),
        index.as_input(),
    ])?;
    for version in 0..4 {
        let raw: Vec<_> = (0..7).map(|i| word(i, version)).collect();
        let input = narrow(&upload(rt, dtype, &[7], &raw)?)?;
        let updates: Vec<_> = (0..7).map(|i| word(i, version + 1)).collect();
        let update = upload(rt, dtype, &[7], &updates)?;
        let flags = match version {
            0 | 3 => [u32::MAX; 5],
            1 => [0; 5],
            _ => [0, 2, 0, u32::MAX, 1],
        };
        let mask = CudaProgramOutput::U32(rt.upload_u32(shape(&[5]), &flags)?);
        let indices = if version % 2 == 0 {
            [2, 2, 0, u32::MAX, 5, 4, 1]
        } else {
            [4, 1, 0, 4, 2, 3, 4]
        };
        let index = CudaProgramOutput::U32(rt.upload_u32(shape(&[7]), &indices)?);
        replay(&mut graph, &[&input, &update, &mask, &index], &mut out)?;
        let data = &raw[1..6];
        let gathered: Vec<_> = indices
            .iter()
            .map(|&i| if i < 5 { data[i as usize] } else { 0 })
            .collect();
        let mut compacted: Vec<_> = data
            .iter()
            .zip(flags)
            .filter(|&(_, flag)| flag != 0)
            .map(|(&v, _)| v)
            .collect();
        let count = compacted.len();
        compacted.resize(5, 0);
        let mut scattered = data.to_vec();
        for (&index, &update) in indices.iter().zip(&updates) {
            if index < 5 {
                scattered[index as usize] = update;
            }
        }
        let invalid = indices.iter().filter(|&&i| i >= 5).count() as u32;
        assert_eq!(read(rt, &out[0])?, gathered);
        assert_eq!(read(rt, &out[1])?, [invalid]);
        assert_eq!(read(rt, &out[2])?, compacted);
        assert_eq!(read(rt, &out[3])?, [count as u32]);
        assert_eq!(read(rt, &out[4])?, scattered);
        assert_eq!(read(rt, &out[5])?, [invalid]);
        let mut total = 0u32;
        let mut scan = [0; 5];
        for j in (0..5).rev() {
            scan[j] = total;
            total = total.wrapping_add(flags[j]);
        }
        assert_eq!(read(rt, &out[6])?, scan);
        assert_eq!(read(rt, &out[7])?, [0]);
        assert_eq!(read(rt, &out[8])?, [u32::from(count == 5)]);
        assert_eq!(read(rt, &out[9])?, data);
        assert_eq!(read(rt, &out[10])?, scattered);
        assert_eq!(read(rt, &first[4])?, first_data);
    }
    graph.synchronize()?;
    drop(graph);
    assert_eq!(read(rt, &first[4])?, first_data);
    Ok(())
}
