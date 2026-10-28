use super::*;
fn transpose_last(values: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    values
        .chunks(rows * cols)
        .flat_map(|tile| (0..cols).flat_map(move |c| (0..rows).map(move |r| tile[r * cols + c])))
        .collect()
}
struct Host {
    q: Vec<f32>,
    k: Vec<f32>,
    v: Vec<f32>,
    keep: Vec<u32>,
    add: Vec<f32>,
}
impl Host {
    fn new(version: usize) -> Self {
        Self {
            q: (0..48)
                .map(|i| ((i % 11) as f32 - 5.) / 8. + version as f32 / 4.)
                .collect(),
            k: (0..20).map(|i| ((i % 7) as f32 - 3.) / 4.).collect(),
            v: (0..60)
                .map(|i| ((i % 13) as f32 - 6.) / 4. + version as f32 / 2.)
                .collect(),
            keep: (0..15)
                .map(|i| {
                    if (version == 1 && i / 5 == 1) || i % 4 == 0 {
                        0
                    } else {
                        7
                    }
                })
                .collect(),
            add: if version == 0 {
                vec![0., f32::NEG_INFINITY, 0.25, 0., -0.5]
            } else {
                vec![
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                    0.5,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ]
            },
        }
    }
    /// Independent dense f64 loops: B2,Hq4,Hkv2,Lq3,Lk5,D2,Dv3.
    /// GQA assigns two consecutive query heads to one key/value head.
    fn reference(&self, mask: usize, scale: f32, causal: i32) -> Vec<f64> {
        let mut out = Vec::new();
        for batch in 0..2 {
            for head in 0..4 {
                for query in 0..3 {
                    let mut logits = [f64::NEG_INFINITY; 5];
                    for (key, logit) in logits.iter_mut().enumerate() {
                        if key as i64 > query as i64 + i64::from(causal)
                            || (mask == 1 && self.keep[query * 5 + key] == 0)
                            || (mask == 2 && self.add[key] == f32::NEG_INFINITY)
                        {
                            continue;
                        }
                        let dot = (0..2)
                            .map(|d| {
                                f64::from(self.q[((batch * 4 + head) * 3 + query) * 2 + d])
                                    * f64::from(self.k[((head / 2) * 5 + key) * 2 + d])
                            })
                            .sum::<f64>();
                        *logit = dot * f64::from(scale)
                            + if mask == 2 {
                                f64::from(self.add[key])
                            } else {
                                0.
                            };
                    }
                    let max = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    if max == f64::NEG_INFINITY {
                        out.extend([0.; 3]);
                        continue;
                    }
                    let weights = logits.map(|l| (l - max).exp());
                    let sum = weights.iter().sum::<f64>();
                    for d in 0..3 {
                        out.push(
                            (0..5)
                                .map(|key| {
                                    weights[key] / sum
                                        * f64::from(
                                            self.v[((batch * 2 + head / 2) * 5 + key) * 3 + d],
                                        )
                                })
                                .sum(),
                        );
                    }
                }
            }
        }
        out
    }
}
pub fn replay(rt: &CudaRuntime, dtype: Option<LowDtype>) -> Result {
    let hosts = [Host::new(0), Host::new(1)];
    let mut inputs = Vec::new();
    for h in &hosts {
        let q = permute(
            rt,
            &upload(rt, dtype, &[2, 4, 2, 3], &transpose_last(&h.q, 3, 2))?,
            &[0, 1, 3, 2],
        )?;
        let k = permute(
            rt,
            &upload(rt, dtype, &[1, 2, 2, 5], &transpose_last(&h.k, 5, 2))?,
            &[0, 1, 3, 2],
        )?;
        let v = permute(
            rt,
            &upload(rt, dtype, &[2, 2, 3, 5], &transpose_last(&h.v, 5, 3))?,
            &[0, 1, 3, 2],
        )?;
        let keep = (0..5)
            .flat_map(|key| (0..3).map(move |q| h.keep[q * 5 + key]))
            .collect::<Vec<_>>();
        let keep = rt.permute_u32(&rt.upload_u32(shape(&[5, 3]), &keep)?, &[1, 0])?;
        let add = rt.upload_f32(shape(&[5]), &h.add)?;
        inputs.push(vec![
            q,
            k,
            v,
            CudaProgramOutput::U32(keep),
            CudaProgramOutput::F32(add),
        ]);
    }
    let mut b = rt.program();
    let values = inputs[0]
        .iter()
        .map(|x| record(&mut b, x))
        .collect::<Result<Vec<_>>>()?;
    let (q, k, v, m, a) = (values[0], values[1], values[2], values[3], values[4]);
    let configs = [
        (1, None, 1),
        (2, Some(-0.5), -1),
        (1, None, i32::MIN),
        (0, Some(0.), i32::MAX),
    ];
    let mut out = Vec::new();
    for (mask, scale, offset) in configs {
        let mask = match mask {
            1 => AttentionMask::Keep(&m),
            2 => AttentionMask::Additive(&a),
            _ => AttentionMask::None,
        };
        let opts = AttentionOptions {
            scale,
            causal: Some(offset),
        };
        out.push(if dtype.is_some() {
            b.attention_low_f32(q, k, v, mask, opts)?
        } else {
            b.attention(q, k, v, mask, opts)?
        });
    }
    out.push(b.sum_axes(out[0], &[3], false)?);
    if dtype.is_some() {
        out.push(b.attention_low(
            q,
            k,
            v,
            AttentionMask::Keep(&m),
            AttentionOptions {
                scale: None,
                causal: Some(1),
            },
        )?);
    }
    let mut p = b.prepare(&out, CudaPrepareOptions::default())?;
    let initial = inputs[0].iter().map(|v| v.as_input()).collect::<Vec<_>>();
    let old = p.run_typed(&initial)?;
    let old_data = rt.read_f32(old[0].as_f32()?)?;
    let mut result = p.run_typed(&initial)?;
    for version in [1, 0, 1] {
        p.run_typed_into(
            &inputs[version]
                .iter()
                .map(|v| v.as_input())
                .collect::<Vec<_>>(),
            &mut result
                .iter_mut()
                .map(|v| v.as_output_mut())
                .collect::<Vec<_>>(),
        )?;
        for (index, (mask, scale, offset)) in configs.into_iter().enumerate() {
            let expected =
                hosts[version].reference(mask, scale.unwrap_or(1. / 2f32.sqrt()), offset);
            close(&rt.read_f32(result[index].as_f32()?)?, &expected);
            if index == 0 {
                close(
                    &rt.read_f32(result[4].as_f32()?)?,
                    &expected
                        .chunks(3)
                        .map(|row| row.iter().sum())
                        .collect::<Vec<_>>(),
                );
            }
        }
        if let Some(dtype) = dtype {
            check_low_round(rt, dtype, &result[0], &result[5])?;
        }
    }
    assert_eq!(rt.read_f32(old[0].as_f32()?)?, old_data);
    Ok(())
}
pub fn zero_keys(rt: &CudaRuntime, dtype: Option<LowDtype>) -> Result {
    let q = upload(rt, dtype, &[3, 2], &[1.; 6])?;
    let k = upload(rt, dtype, &[0, 2], &[])?;
    let v = upload(rt, dtype, &[0, 3], &[])?;
    let mut b = rt.program();
    let qv = record(&mut b, &q)?;
    let kv = record(&mut b, &k)?;
    let vv = record(&mut b, &v)?;
    let y = if dtype.is_some() {
        b.attention_low_f32(qv, kv, vv, AttentionMask::None, AttentionOptions::default())?
    } else {
        b.attention(qv, kv, vv, AttentionMask::None, AttentionOptions::default())?
    };
    let mut p = b.prepare(&[y], CudaPrepareOptions::default())?;
    let inputs = [q.as_input(), k.as_input(), v.as_input()];
    let mut result = p.run_typed(&inputs)?;
    for _ in 0..3 {
        let CudaProgramOutput::F32(t) = &mut result[0] else {
            panic!()
        };
        rt.write_f32(t, &[19.; 9])?;
        p.run_typed_into(
            &inputs,
            &mut result
                .iter_mut()
                .map(|v| v.as_output_mut())
                .collect::<Vec<_>>(),
        )?;
        assert_eq!(rt.read_f32(result[0].as_f32()?)?, [0.; 9]);
    }
    Ok(())
}

pub fn tiny_products(rt: &CudaRuntime) -> Result {
    for tiny in [1u16, 0x7f, 0x8001] {
        for swapped in [false, true] {
            let large = 0x7f7f;
            let (qbits, kbits) = if swapped {
                (large, [tiny, 0])
            } else {
                (tiny, [large, 0])
            };
            let q = rt.upload_low(LowDtype::Bf16, shape(&[1, 1]), &[qbits])?;
            let k = rt.upload_low(LowDtype::Bf16, shape(&[2, 1]), &kbits)?;
            let v = rt.upload_low(LowDtype::Bf16, shape(&[2, 1]), &[0x3f80, 0])?;
            let mut b = rt.program();
            let qv = b.input_low(LowDtype::Bf16, q.layout().clone())?;
            let kv = b.input_low(LowDtype::Bf16, k.layout().clone())?;
            let vv = b.input_low(LowDtype::Bf16, v.layout().clone())?;
            let y =
                b.attention_low_f32(qv, kv, vv, AttentionMask::None, AttentionOptions::default())?;
            let mut p = b.prepare(&[y], CudaPrepareOptions::default())?;
            let r = p.run_typed(&[(&q).into(), (&k).into(), (&v).into()])?;
            let dot = decode(LowDtype::Bf16, tiny) * decode(LowDtype::Bf16, large);
            assert!(dot.abs() >= f64::from(f32::MIN_POSITIVE));
            close(&rt.read_f32(r[0].as_f32()?)?, &[1. / (1. + (-dot).exp())]);
        }
    }
    Ok(())
}
