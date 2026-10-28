use super::*;

#[test]
fn compiled_gather_empty_outputs_still_count_original_indices() {
    let Some(b) = backend() else { return };
    for dtype in [None, Some(LowDtype::F16), Some(LowDtype::Bf16)] {
        for (dims, axis, expected_shape) in [([0, 3], 1, vec![0, 4]), ([2, 0], 1, vec![2, 4])] {
            let mut graph = b.program();
            let x = match dtype {
                None => graph.input(shape(&dims)),
                Some(t) => graph.input_low(t, shape(&dims)),
            }
            .unwrap();
            let i = graph.input_u32(shape(&[4])).unwrap();
            let pair = if dtype.is_some() {
                graph.gather_low(x, i, axis)
            } else {
                graph.gather(x, i, axis)
            }
            .unwrap();
            let program = graph.compile(&[pair.values, pair.invalid_count]).unwrap();
            for (replay, indices) in [[0, 1, 3, u32::MAX], [2, 0, 1, 0]].into_iter().enumerate() {
                let index = b.upload_u32(shape(&[4]), &indices).unwrap();
                let out = match dtype {
                    None => {
                        let input = b.upload_f32(shape(&dims), &[]).unwrap();
                        program
                            .run_typed(&[(&input).into(), (&index).into()])
                            .unwrap()
                    }
                    Some(t) => {
                        let input = b.upload_low(t, shape(&dims), &[]).unwrap();
                        program
                            .run_typed(&[(&input).into(), (&index).into()])
                            .unwrap()
                    }
                };
                assert_eq!(out[0].shape(), &shape(&expected_shape));
                if dtype.is_some() {
                    bits(
                        &b,
                        &out[0],
                        &expected_shape,
                        &vec![0; shape(&expected_shape).numel()],
                    );
                } else {
                    assert_eq!(
                        b.read_f32(out[0].as_tensor().unwrap()).unwrap(),
                        vec![0.; shape(&expected_shape).numel()]
                    );
                }
                uints(
                    &b,
                    &out[1],
                    &[],
                    &[indices
                        .iter()
                        .filter(|&&v| v as usize >= dims[axis])
                        .count() as u32],
                );
                traces(&program, replay + 1);
            }
        }
    }
    // A scalar index removes the axis; an empty index tensor adds an empty
    // axis and contributes exactly zero invalid indices.
    let mut graph = b.program();
    let x = graph.input_u32(shape(&[0])).unwrap();
    let scalar = graph.input_u32(shape(&[])).unwrap();
    let empty = graph.input_u32(shape(&[0])).unwrap();
    let a = graph.gather_u32(x, scalar, 0).unwrap();
    let c = graph.gather_u32(x, empty, 0).unwrap();
    let program = graph
        .compile(&[a.values, a.invalid_count, c.values, c.invalid_count])
        .unwrap();
    let x = b.upload_u32(shape(&[0]), &[]).unwrap();
    let scalar = b.upload_u32(shape(&[]), &[u32::MAX]).unwrap();
    let empty = b.upload_u32(shape(&[0]), &[]).unwrap();
    let out = program
        .run_typed(&[(&x).into(), (&scalar).into(), (&empty).into()])
        .unwrap();
    uints(&b, &out[0], &[], &[0]);
    uints(&b, &out[1], &[], &[1]);
    uints(&b, &out[2], &[0], &[]);
    uints(&b, &out[3], &[], &[0]);
}

#[test]
fn compiled_indexing_rejects_invalid_types_and_owners_before_trace() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    let x = graph.input(shape(&[0, 3])).unwrap();
    let a = graph.input_low(LowDtype::F16, shape(&[0, 3])).unwrap();
    let c = graph.input_low(LowDtype::Bf16, shape(&[0, 3])).unwrap();
    let i = graph.input_u32(shape(&[])).unwrap();
    let mut other = b.program();
    let foreign = other.input_u32(shape(&[])).unwrap();
    assert!(matches!(
        graph.gather(x, foreign, 1),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(graph.gather(x, x, 1), Err(MlxError::Dtype)));
    assert!(matches!(graph.gather_u32(x, i, 1), Err(MlxError::Dtype)));
    assert!(matches!(graph.gather_low(x, i, 1), Err(MlxError::Dtype)));
    assert!(graph.gather(x, i, 2).is_err());
    assert!(graph.compare_low(a, c, CompareOp::Equal).is_err());
    assert!(graph.select_low(i, a, c).is_err());
    assert!(matches!(graph.select(x, x, x), Err(MlxError::Dtype)));
    assert!(matches!(graph.select(i, a, a), Err(MlxError::Dtype)));
    let pair = graph.gather(x, i, 1).unwrap();
    let program = graph.compile(&[pair.values, pair.invalid_count]).unwrap();
    let x = b.upload_f32(shape(&[0, 3]), &[]).unwrap();
    let a = b.upload_low(LowDtype::F16, shape(&[0, 3]), &[]).unwrap();
    let c = b.upload_low(LowDtype::Bf16, shape(&[0, 3]), &[]).unwrap();
    let i = b.upload_u32(shape(&[]), &[3]).unwrap();
    // Rejected recording calls must not append new input slots.
    assert!(matches!(
        program.run_typed(&[]),
        Err(MlxError::ProgramInputCount { expected: 4, .. })
    ));
    assert!(matches!(
        program.run(&[&x, &x, &x, &i]),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        program.run_typed(&[(&x).into(), (&a).into(), (&c).into(), (&x).into()]),
        Err(MlxError::Dtype)
    ));
    let foreign_backend = MlxBackend::new_gpu().unwrap();
    let foreign = foreign_backend.upload_u32(shape(&[]), &[0]).unwrap();
    assert!(matches!(
        program.run_typed(&[(&x).into(), (&a).into(), (&c).into(), (&foreign).into()]),
        Err(MlxError::ForeignContext)
    ));
    assert_eq!(program.trace_count(), 0);
    let out = program
        .run_typed(&[(&x).into(), (&a).into(), (&c).into(), (&i).into()])
        .unwrap();
    uints(&b, &out[1], &[], &[1]);
    traces(&program, 1);
}
