use super::*;

pub fn rejection_budget_empty_and_lifetime(rt: &CudaRuntime) -> Result {
    let mut x = rt.upload_f32(shape(&[4]), &[1., 2., 3., 4.])?;
    let mut b = rt.program();
    let input = b.input(x.layout().clone())?;
    let square = b.unary(input, UnaryOp::Square)?;
    let prepared = b.prepare(&[square, input], CudaPrepareOptions::default())?;
    assert!(matches!(
        prepared.capture_owned(CudaCaptureOptions {
            max_owned_bytes: 0,
            ..Default::default()
        }),
        Err(CudaError::PreparationBudget { .. })
    ));
    let mut b = rt.program();
    let input = b.input(x.layout().clone())?;
    let square = b.unary(input, UnaryOp::Square)?;
    let mut graph = b
        .prepare(&[square, input], CudaPrepareOptions::default())?
        .capture_owned(CudaCaptureOptions::default())?;
    let stats = graph.stats();
    assert!(
        stats.owned_bytes
            >= stats.input_slot_bytes
                + stats.output_slot_bytes
                + stats.transfer_metadata_bytes
                + stats.workspace_bytes
                + stats.program.scratch_bytes
                + stats.program.metadata_bytes
    );
    assert_eq!(stats.input_copy_launches, 1);
    assert_eq!(stats.output_copy_launches, 2);
    let mut out = rt.upload_f32(shape(&[4]), &[99.; 4])?;
    let mut other = rt.upload_f32(shape(&[4]), &[98.; 4])?;
    assert!(graph.run_into(&[], &mut [&mut out, &mut other]).is_err());
    assert_eq!(rt.read_f32(&out)?, [99.; 4]);
    let bad = rt.upload_f32(shape(&[2, 2]), &[1.; 4])?;
    assert!(
        graph
            .run_into(&[&bad], &mut [&mut out, &mut other])
            .is_err()
    );
    assert_eq!(rt.read_f32(&out)?, [99.; 4]);
    let wrong = rt.upload_u32(shape(&[4]), &[1; 4])?;
    assert!(matches!(
        graph.run_typed(&[(&wrong).into()]),
        Err(CudaError::Dtype)
    ));
    let mut alias = x.clone();
    assert!(
        graph
            .run_into(&[&x], &mut [&mut alias, &mut other])
            .is_err()
    );
    assert_eq!(rt.read_f32(&x)?, [1., 2., 3., 4.]);
    drop(alias);
    let foreign_rt = CudaRuntime::new()?;
    let foreign = foreign_rt.upload_f32(shape(&[4]), &[1.; 4])?;
    assert!(matches!(
        graph.run_into(&[&foreign], &mut [&mut out, &mut other]),
        Err(CudaError::ForeignRuntime)
    ));
    assert_eq!(rt.read_f32(&out)?, [99.; 4]);
    assert!(!graph.is_poisoned());
    // Change values in the very same external allocation, then use new handles.
    rt.write_f32(&mut x, &[2., 4., 6., 8.])?;
    graph.run_into(&[&x], &mut [&mut out, &mut other])?;
    assert_eq!(rt.read_f32(&out)?, [4., 16., 36., 64.]);
    assert_eq!(rt.read_f32(&other)?, [2., 4., 6., 8.]);
    let fresh = rt.upload_f32(shape(&[4]), &[3., 1., -2., 0.5])?;
    let outputs = graph.run(&[&fresh])?;
    // Drop without an explicit sync: the graph owner must preserve completion
    // and exported arrays must retain ordinary external runtime ownership.
    drop(graph);
    assert_eq!(rt.read_f32(&outputs[0])?, [9., 1., 4., 0.25]);
    assert_eq!(rt.read_f32(&out)?, [4., 16., 36., 64.]);
    let mut empty = rt
        .program()
        .prepare(&[], CudaPrepareOptions::default())?
        .capture_owned(CudaCaptureOptions::default())?;
    assert!(empty.run(&[])?.is_empty());
    empty.synchronize()?;
    let e = rt.upload_f32(shape(&[0]), &[])?;
    let mut b = rt.program();
    let input = b.input(e.layout().clone())?;
    let mut graph = b
        .prepare(&[input, input], CudaPrepareOptions::default())?
        .capture_owned(CudaCaptureOptions::default())?;
    for _ in 0..2 {
        for output in graph.run(&[&e])? {
            assert!(rt.read_f32(&output)?.is_empty());
        }
    }
    graph.synchronize()?;
    Ok(())
}
