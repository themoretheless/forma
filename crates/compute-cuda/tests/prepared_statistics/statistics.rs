use super::*;
fn outputs(
    b: &mut CudaProgramBuilder<'_>,
    x: CudaValue,
    dtype: Option<LowDtype>,
    axes: &[usize],
    epsilon: f32,
) -> Result<Vec<CudaValue>> {
    let mut out = if dtype.is_some() {
        let m = b.moments_low_f32(x, axes, false)?;
        vec![
            b.softmax_low_f32(x, axes)?,
            b.log_softmax_low_f32(x, axes)?,
            b.logsumexp_low_f32(x, axes, true)?,
            m.mean,
            m.variance,
            b.layer_norm_low_f32(x, axes, epsilon)?,
        ]
    } else {
        let m = b.moments(x, axes, false)?;
        vec![
            b.softmax(x, axes)?,
            b.log_softmax(x, axes)?,
            b.logsumexp(x, axes, true)?,
            m.mean,
            m.variance,
            b.layer_norm(x, axes, epsilon)?,
        ]
    };
    if dtype.is_some() {
        let m = b.moments_low(x, axes, false)?;
        out.extend([
            b.softmax_low(x, axes)?,
            b.log_softmax_low(x, axes)?,
            b.logsumexp_low(x, axes, true)?,
            m.mean,
            m.variance,
            b.layer_norm_low(x, axes, epsilon)?,
        ]);
    }
    Ok(out)
}
pub fn replay(rt: &CudaRuntime, dtype: Option<LowDtype>) -> Result {
    let n = 8197;
    let host = [0, 1].map(|version| {
        (0..n * 2)
            .map(|i| ((i % 17) as f32 - 8.) / 8. + version as f32 * 2.)
            .collect::<Vec<_>>()
    });
    let mut tensors = Vec::new();
    for values in &host {
        tensors.push(permute(rt, &upload(rt, dtype, &[n, 2], values)?, &[1, 0])?);
    }
    let mut b = rt.program();
    let x = record(&mut b, &tensors[0])?;
    let mut recorded = outputs(&mut b, x, dtype, &[1], 0.25)?;
    let sums = b.sum_axes(recorded[0], &[1], false)?;
    let prefix = b.scan(recorded[0], 1, ScanOptions::default())?;
    recorded.extend([sums, prefix]);
    let mut p = b.prepare(&recorded, CudaPrepareOptions::default())?;
    let old = p.run_typed(&[tensors[0].as_input()])?;
    let old_bits = rt.read_f32(old[0].as_f32()?)?;
    let mut result = p.run_typed(&[tensors[0].as_input()])?;
    for version in [1, 0, 1] {
        p.run_typed_into(
            &[tensors[version].as_input()],
            &mut result
                .iter_mut()
                .map(|o| o.as_output_mut())
                .collect::<Vec<_>>(),
        )?;
        let rows = (0..2)
            .map(|row| {
                stats(
                    &(0..n)
                        .map(|c| f64::from(host[version][c * 2 + row]))
                        .collect::<Vec<_>>(),
                    0.25,
                )
            })
            .collect::<Vec<_>>();
        for (index, expected) in [
            rows.iter().flat_map(|r| r.soft.clone()).collect::<Vec<_>>(),
            rows.iter().flat_map(|r| r.log.clone()).collect(),
            rows.iter().map(|r| r.lse).collect(),
            rows.iter().map(|r| r.mean).collect(),
            rows.iter().map(|r| r.variance).collect(),
            rows.iter().flat_map(|r| r.norm.clone()).collect(),
        ]
        .iter()
        .enumerate()
        {
            close(&rt.read_f32(result[index].as_f32()?)?, expected);
        }
        if let Some(dtype) = dtype {
            for index in 0..6 {
                check_low_round(rt, dtype, &result[index], &result[index + 6])?;
            }
        }
        close(&rt.read_f32(result[result.len() - 2].as_f32()?)?, &[1., 1.]);
        let prefix = rt.read_f32(result.last().unwrap().as_f32()?)?;
        close(&[prefix[n - 1], prefix[2 * n - 1]], &[1., 1.]);
    }
    assert_eq!(rt.read_f32(old[0].as_f32()?)?, old_bits);
    Ok(())
}
pub fn singleton_empty(rt: &CudaRuntime, dtype: Option<LowDtype>) -> Result {
    let data = if let Some(dtype) = dtype {
        let max = if dtype == LowDtype::F16 {
            0x7bff
        } else {
            0x7f7f
        };
        [0, 0x8000, 1, 0x8001, max]
            .map(|v| decode(dtype, v) as f32)
            .to_vec()
    } else {
        vec![0., -0., f32::from_bits(1), -f32::from_bits(1), 1e30]
    };
    let t = upload(rt, dtype, &[5, 1], &data)?;
    let empty = upload(rt, dtype, &[0, 3], &[])?;
    let mut b = rt.program();
    let x = record(&mut b, &t)?;
    let e = record(&mut b, &empty)?;
    let mut out = outputs(&mut b, x, dtype, &[1], f32::from_bits(1))?;
    let empty_start = out.len();
    out.extend(outputs(&mut b, e, dtype, &[1], 0.5)?);
    let mut p = b.prepare(&out, CudaPrepareOptions::default())?;
    for _ in 0..2 {
        let r = p.run_typed(&[t.as_input(), empty.as_input()])?;
        for i in [2, 3] {
            assert_eq!(
                rt.read_f32(r[i].as_f32()?)?
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                data.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
        }
        assert_eq!(rt.read_f32(r[0].as_f32()?)?, [1.; 5]);
        for i in [1, 4, 5] {
            assert!(rt.read_f32(r[i].as_f32()?)?.iter().all(|&v| v == 0.));
        }
        if let Some(dtype) = dtype {
            for i in 0..6 {
                check_low_round(rt, dtype, &r[i], &r[i + 6])?;
            }
        }
        for x in &r[empty_start..] {
            assert!(x.layout().shape().is_empty());
        }
    }
    Ok(())
}
pub fn extremes(rt: &CudaRuntime) -> Result {
    for values in [
        vec![f32::MAX, -f32::MAX, f32::MAX],
        vec![1e30; 3],
        vec![100000000., 100000008., 99999992.],
    ] {
        let input = rt.upload_f32(shape(&[3]), &values)?;
        let mut b = rt.program();
        let x = b.input(input.layout().clone())?;
        let m = b.moments(x, &[0], false)?;
        let n = b.layer_norm(x, &[0], 1e-5)?;
        let mut p = b.prepare(&[m.mean, m.variance, n], CudaPrepareOptions::default())?;
        let r = p.run(&[&input])?;
        let expected = stats(
            &values.iter().map(|&v| f64::from(v)).collect::<Vec<_>>(),
            f64::from(1e-5f32),
        );
        close(&rt.read_f32(&r[0])?, &[expected.mean]);
        close(&rt.read_f32(&r[1])?, &[expected.variance]);
        close(&rt.read_f32(&r[2])?, &expected.norm);
    }
    let input = rt.upload_low(LowDtype::Bf16, shape(&[3]), &[1, 2, 3])?;
    let mut b = rt.program();
    let x = b.input_low(LowDtype::Bf16, input.layout().clone())?;
    let n = b.layer_norm_low_f32(x, &[0], f32::from_bits(1))?;
    let mut p = b.prepare(&[n], CudaPrepareOptions::default())?;
    let r = p.run_typed(&[(&input).into()])?;
    let expected = stats(
        &[
            decode(LowDtype::Bf16, 1),
            decode(LowDtype::Bf16, 2),
            decode(LowDtype::Bf16, 3),
        ],
        f64::from(f32::from_bits(1)),
    );
    for (a, e) in rt.read_f32(r[0].as_f32()?)?.iter().zip(expected.norm) {
        assert!((f64::from(*a) - e).abs() <= 1e-5 * e.abs().max(1e-35));
    }
    Ok(())
}
