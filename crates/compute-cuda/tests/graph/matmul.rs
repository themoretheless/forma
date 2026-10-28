use super::*;

pub fn strided_views_and_gemm(rt: &CudaRuntime, dtype: Option<LowDtype>) -> Result {
    // Physical [4,2] is transposed then narrowed: logical [2,3], offset2,
    // strides[1,2]. A dense graph slot may not reuse these original strides.
    let make = |version: usize| -> Result<CudaProgramOutput> {
        let data: Vec<_> = (0..8).map(|i| i as f32 * 0.25 + version as f32).collect();
        let source = upload_float(rt, dtype, &[4, 2], &data)?;
        let source = transpose(rt, &source)?;
        Ok(match source {
            CudaProgramOutput::F32(t) => CudaProgramOutput::F32(rt.narrow(&t, 1, 1, 3)?),
            CudaProgramOutput::Low(t) => CudaProgramOutput::Low(rt.narrow_low(&t, 1, 1, 3)?),
            _ => unreachable!(),
        })
    };
    let x = make(0)?;
    let right = upload_float(rt, dtype, &[3, 2], &[1., 0.5, -1., 2., 0.25, -0.5])?;
    let right2 = upload_float(rt, dtype, &[2, 2], &[1., 0.5, -1., 2.])?;
    let mut builder = rt.program();
    let input = record(&mut builder, &x)?;
    let rhs = record(&mut builder, &right)?;
    let rhs2 = record(&mut builder, &right2)?;
    // Direct GEMM must materialize the external strided input. The separate
    // transpose->reshape branch checks views against rebased owned storage.
    let gemm = if dtype.is_some() {
        builder.matmul_low_f32(input, rhs)?
    } else {
        builder.matmul(input, rhs, MatmulPrecision::F32)?
    };
    let transposed = builder.permute(input, &[1, 0])?;
    // This view is dense in the original allocation, but noncontiguous after
    // graph input rebasing. Capture must insert the required GEMM input copy.
    let transposed_gemm = if dtype.is_some() {
        builder.matmul_low_f32(transposed, rhs2)?
    } else {
        builder.matmul(transposed, rhs2, MatmulPrecision::F32)?
    };
    let flat = builder.reshape(transposed, shape(&[6]))?;
    let prepared = builder.prepare(
        &[gemm, flat, input, gemm, transposed_gemm],
        CudaPrepareOptions::default(),
    )?;
    let mut graph = prepared.capture_owned(CudaCaptureOptions::default())?;
    assert_eq!(graph.stats().program.gemm_calls, 2);
    assert!(graph.stats().workspace_bytes >= 16 * 1024 + 255);
    assert_eq!(
        graph.input_dtypes(),
        vec![dtype.map_or(CudaDtype::F32, Into::into); 3]
    );
    let old = graph.run_typed(&[x.as_input(), right.as_input(), right2.as_input()])?;
    let old_gemm = rt.read_f32(old[0].as_f32()?)?;
    let mut outputs = graph.run_typed(&[x.as_input(), right.as_input(), right2.as_input()])?;
    for version in [2, 1, 0] {
        let x = make(version)?;
        replay(&mut graph, &[&x, &right, &right2], &mut outputs)?;
        let logical: Vec<f32> = (0..2)
            .flat_map(|r| (0..3).map(move |c| (2 * (c + 1) + r) as f32 * 0.25 + version as f32))
            .collect();
        let weights = [1f64, 0.5, -1., 2., 0.25, -0.5];
        let expected: Vec<_> = (0..2)
            .flat_map(|r| {
                let logical = &logical;
                (0..2).map(move |c| {
                    (0..3)
                        .map(|k| f64::from(logical[r * 3 + k]) * weights[k * 2 + c])
                        .sum::<f64>()
                })
            })
            .collect();
        close(&rt.read_f32(outputs[0].as_f32()?)?, &expected);
        close(&rt.read_f32(outputs[3].as_f32()?)?, &expected);
        let weights2 = [1f64, 0.5, -1., 2.];
        let expected_transposed: Vec<_> = (0..3)
            .flat_map(|r| {
                let logical = &logical;
                (0..2).map(move |c| {
                    f64::from(logical[r]) * weights2[c]
                        + f64::from(logical[3 + r]) * weights2[2 + c]
                })
            })
            .collect();
        close(&rt.read_f32(outputs[4].as_f32()?)?, &expected_transposed);
        let raw = |x: f32| dtype.map_or(x.to_bits(), |d| u32::from(encode(d, x)));
        assert_eq!(
            read(rt, &outputs[2])?,
            logical.iter().copied().map(raw).collect::<Vec<_>>()
        );
        let flat: Vec<_> = (0..3)
            .flat_map(|c| {
                let logical = &logical;
                (0..2).map(move |r| raw(logical[r * 3 + c]))
            })
            .collect();
        assert_eq!(read(rt, &outputs[1])?, flat);
        assert_eq!(rt.read_f32(old[0].as_f32()?)?, old_gemm);
    }
    graph.synchronize()?;
    assert!(!graph.is_poisoned());
    drop(graph);
    assert_eq!(rt.read_f32(old[0].as_f32()?)?, old_gemm);
    Ok(())
}

pub fn precision_modes(rt: &CudaRuntime) -> Result {
    for precision in [
        MatmulPrecision::F32,
        MatmulPrecision::AllowTf32,
        MatmulPrecision::AllowF16,
        MatmulPrecision::AllowBf16,
    ] {
        let a = rt.upload_f32(shape(&[2, 2]), &[1., 2., 3., 4.])?;
        let b = rt.upload_f32(shape(&[2, 2]), &[0.5, 1., -1., 2.])?;
        let mut builder = rt.program();
        let x = builder.input(a.layout().clone())?;
        let y = builder.input(b.layout().clone())?;
        let value = builder.matmul(x, y, precision)?;
        if rt.matmul_policy(precision).is_err() {
            assert!(
                builder
                    .prepare(&[value], CudaPrepareOptions::default())
                    .is_err()
            );
            continue;
        }
        let mut graph = builder
            .prepare(&[value], CudaPrepareOptions::default())?
            .capture_owned(CudaCaptureOptions::default())?;
        let outputs = graph.run(&[&a, &b])?;
        assert_eq!(rt.read_f32(&outputs[0])?, [-1.5, 5., -2.5, 11.]);
        graph.synchronize()?;
    }
    Ok(())
}
