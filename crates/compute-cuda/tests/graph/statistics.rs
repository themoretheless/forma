use super::*;

pub fn resident_statistics_attention(rt: &CudaRuntime, dtype: Option<LowDtype>) -> Result {
    let qdata = |version: usize| -> Vec<f32> {
        (0..12)
            .map(|i| ((i + version) % 5) as f32 * 0.25 - 0.5)
            .collect()
    };
    let kdata: Vec<_> = (0..12).map(|i| (i % 7) as f32 * 0.25 - 0.75).collect();
    let vdata = vec![1., -1., 0.5, 2., -0.5, 1., 3., 0.25];
    let q = upload_float(rt, dtype, &[2, 2, 3], &qdata(0))?;
    let k = upload_float(rt, dtype, &[1, 4, 3], &kdata)?;
    let v = upload_float(rt, dtype, &[1, 4, 2], &vdata)?;
    let keep = CudaProgramOutput::U32(rt.upload_u32(shape(&[2, 4]), &[1; 8])?);
    let additive = CudaProgramOutput::F32(rt.upload_f32(shape(&[4]), &[0.; 4])?);
    let mut b = rt.program();
    let x = record(&mut b, &q)?;
    let key = record(&mut b, &k)?;
    let value = record(&mut b, &v)?;
    let mask = record(&mut b, &keep)?;
    let add = record(&mut b, &additive)?;
    let (soft, log, lse, moments, norm) = if dtype.is_some() {
        (
            b.softmax_low_f32(x, &[2])?,
            b.log_softmax_low_f32(x, &[2])?,
            b.logsumexp_low_f32(x, &[2], false)?,
            b.moments_low_f32(x, &[2], false)?,
            b.layer_norm_low_f32(x, &[2], 0.25)?,
        )
    } else {
        (
            b.softmax(x, &[2])?,
            b.log_softmax(x, &[2])?,
            b.logsumexp(x, &[2], false)?,
            b.moments(x, &[2], false)?,
            b.layer_norm(x, &[2], 0.25)?,
        )
    };
    let options = AttentionOptions {
        scale: Some(-0.5),
        causal: Some(1),
    };
    let attention = if dtype.is_some() {
        b.attention_low_f32(x, key, value, AttentionMask::Keep(&mask), options)?
    } else {
        b.attention(x, key, value, AttentionMask::Keep(&mask), options)?
    };
    let added = if dtype.is_some() {
        b.attention_low_f32(
            x,
            key,
            value,
            AttentionMask::Additive(&add),
            AttentionOptions::default(),
        )?
    } else {
        b.attention(
            x,
            key,
            value,
            AttentionMask::Additive(&add),
            AttentionOptions::default(),
        )?
    };
    let sums = b.sum_axes(soft, &[2], false)?;
    let mut values = vec![
        soft,
        log,
        lse,
        moments.mean,
        moments.variance,
        norm,
        attention,
        added,
        sums,
    ];
    if let Some(d) = dtype {
        values.push(b.cast_to_low(soft, d)?);
        values.push(b.cast_to_low(attention, d)?);
    }
    let mut graph = b
        .prepare(&values, CudaPrepareOptions::default())?
        .capture_owned(CudaCaptureOptions::default())?;
    assert_eq!(graph.stats().workspace_bytes, 0);
    let mut out = graph.run_typed(&[
        q.as_input(),
        k.as_input(),
        v.as_input(),
        keep.as_input(),
        additive.as_input(),
    ])?;
    for version in [1, 2, 0] {
        let data = qdata(version);
        let q = upload_float(rt, dtype, &[2, 2, 3], &data)?;
        let flags = match version {
            0 => [1; 8],
            1 => [1, 0, 2, 1, 0, 0, 0, 0],
            _ => [0; 8],
        };
        let adds = match version {
            0 => [0.; 4],
            1 => [0., f32::NEG_INFINITY, 0.25, -0.5],
            _ => [f32::NEG_INFINITY; 4],
        };
        let mask = CudaProgramOutput::U32(rt.upload_u32(shape(&[2, 4]), &flags)?);
        let additive = CudaProgramOutput::F32(rt.upload_f32(shape(&[4]), &adds)?);
        replay(&mut graph, &[&q, &k, &v, &mask, &additive], &mut out)?;
        let references: Vec<_> = data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|row| stats(&row.map(f64::from), 0.25))
            .collect();
        for (index, expected) in [
            (
                0,
                references
                    .iter()
                    .flat_map(|r| r.soft.clone())
                    .collect::<Vec<_>>(),
            ),
            (1, references.iter().flat_map(|r| r.log.clone()).collect()),
            (2, references.iter().map(|r| r.lse).collect()),
            (3, references.iter().map(|r| r.mean).collect()),
            (4, references.iter().map(|r| r.variance).collect()),
            (5, references.iter().flat_map(|r| r.norm.clone()).collect()),
        ] {
            close(&rt.read_f32(out[index].as_f32()?)?, &expected);
        }
        // Independent dense reference: two query heads share the single K/V
        // head. No backend AttentionPlan/Layout participates in this oracle.
        for (slot, keep_mask, scale, causal) in [
            (6, true, -0.5f64, Some(1i64)),
            (7, false, f64::from(1f32 / 3f32.sqrt()), None),
        ] {
            let mut expected = Vec::new();
            for h in 0..2 {
                for row in 0..2 {
                    let logits: Vec<_> = (0..4)
                        .map(|key| {
                            let allowed = if keep_mask {
                                flags[row * 4 + key] != 0
                                    && key as i64 <= row as i64 + causal.unwrap()
                            } else {
                                adds[key] != f32::NEG_INFINITY
                            };
                            if !allowed {
                                return f64::NEG_INFINITY;
                            }
                            let dot = (0..3)
                                .map(|d| {
                                    f64::from(data[(h * 2 + row) * 3 + d])
                                        * f64::from(kdata[key * 3 + d])
                                })
                                .sum::<f64>();
                            dot * scale + if keep_mask { 0. } else { f64::from(adds[key]) }
                        })
                        .collect();
                    let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let weights: Vec<_> = logits
                        .iter()
                        .map(|&x| if x.is_finite() { (x - max).exp() } else { 0. })
                        .collect();
                    let sum = weights.iter().sum::<f64>();
                    for col in 0..2 {
                        expected.push(if sum == 0. {
                            0.
                        } else {
                            (0..4)
                                .map(|j| weights[j] * f64::from(vdata[j * 2 + col]))
                                .sum::<f64>()
                                / sum
                        });
                    }
                }
            }
            close(&rt.read_f32(out[slot].as_f32()?)?, &expected);
        }
        close(&rt.read_f32(out[8].as_f32()?)?, &[1.; 4]);
        if let Some(d) = dtype {
            check_low_round(rt, d, &out[0], &out[9])?;
            check_low_round(rt, d, &out[6], &out[10])?;
        }
    }
    graph.synchronize()?;
    zero_keys(rt, dtype)?;
    Ok(())
}

fn zero_keys(rt: &CudaRuntime, dtype: Option<LowDtype>) -> Result {
    let q = upload_float(rt, dtype, &[2, 3], &[1.; 6])?;
    let k = upload_float(rt, dtype, &[0, 3], &[])?;
    let v = upload_float(rt, dtype, &[0, 2], &[])?;
    let mut b = rt.program();
    let query = record(&mut b, &q)?;
    let key = record(&mut b, &k)?;
    let value = record(&mut b, &v)?;
    let output = if dtype.is_some() {
        b.attention_low_f32(
            query,
            key,
            value,
            AttentionMask::None,
            AttentionOptions::default(),
        )?
    } else {
        b.attention(
            query,
            key,
            value,
            AttentionMask::None,
            AttentionOptions::default(),
        )?
    };
    let mut graph = b
        .prepare(&[output], CudaPrepareOptions::default())?
        .capture_owned(CudaCaptureOptions::default())?;
    for _ in 0..2 {
        let mut out = [CudaProgramOutput::F32(
            rt.upload_f32(shape(&[2, 2]), &[19.; 4])?,
        )];
        replay(&mut graph, &[&q, &k, &v], &mut out)?;
        assert_eq!(rt.read_f32(out[0].as_f32()?)?, [0.; 4]);
    }
    graph.synchronize()
}
