use super::*;

#[test]
fn typed_bindings_validate_owner_dtype_shape_before_trace_and_keep_output_types() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    let u = graph.input_u32(shape(&[3])).unwrap();
    let h = graph.input_low(LowDtype::F16, shape(&[3])).unwrap();
    let bf = graph.input_low(LowDtype::Bf16, shape(&[3])).unwrap();
    let x = graph.input(shape(&[3])).unwrap();
    let program = graph.compile(&[u, h, bf, x, h]).unwrap();
    let u = b.upload_u32(shape(&[3]), &[u32::MAX, 0, 1 << 31]).unwrap();
    let h = b
        .upload_low(LowDtype::F16, shape(&[3]), &[1, 0x8000, 0xffff])
        .unwrap();
    let bf = b
        .upload_low(LowDtype::Bf16, shape(&[3]), &[2, 0x8001, 0x7fc1])
        .unwrap();
    let x = b.upload_f32(shape(&[3]), &[1., -2., 3.]).unwrap();
    let wrong_shape = b
        .upload_low(LowDtype::F16, shape(&[1, 3]), &[0; 3])
        .unwrap();
    let foreign = MlxBackend::new_gpu().unwrap();
    let foreign_h = foreign
        .upload_low(LowDtype::F16, shape(&[3]), &[0; 3])
        .unwrap();
    for run in 0..2 {
        assert!(matches!(
            program.run_typed(&[]),
            Err(MlxError::ProgramInputCount { .. })
        ));
        assert!(matches!(
            program.run_typed(&[(&x).into(), (&h).into(), (&bf).into(), (&x).into()]),
            Err(MlxError::Dtype)
        ));
        assert!(matches!(
            program.run_typed(&[(&u).into(), (&bf).into(), (&h).into(), (&x).into()]),
            Err(MlxError::Dtype)
        ));
        assert!(matches!(
            program.run_typed(&[
                (&u).into(),
                (&wrong_shape).into(),
                (&bf).into(),
                (&x).into()
            ]),
            Err(MlxError::ProgramInputShape { index: 1, .. })
        ));
        assert!(matches!(
            program.run_typed(&[(&u).into(), (&foreign_h).into(), (&bf).into(), (&x).into()]),
            Err(MlxError::ForeignContext)
        ));
        assert!(program.run(&[&u, &u, &u, &x]).is_err());
        traces(&program, run);
        let mut out = program
            .run_typed(&[(&u).into(), (&h).into(), (&bf).into(), (&x).into()])
            .unwrap();
        assert_eq!(out[0].dtype(), MlxDtype::U32);
        assert_eq!(
            b.read_u32(out[0].as_tensor().unwrap()).unwrap(),
            [u32::MAX, 0, 1 << 31]
        );
        low_values(&b, &out[1], &[3], LowDtype::F16, &[1, 0x8000, 0xffff]);
        low_values(&b, &out[2], &[3], LowDtype::Bf16, &[2, 0x8001, 0x7fc1]);
        floats(&b, &out[3], &[3], &[1., -2., 3.]);
        assert!(out[0].as_low().is_err());
        assert!(out[1].as_tensor().is_err());
        let duplicate = out.pop().unwrap().into_low().unwrap();
        assert_eq!(b.read_low_bits(&duplicate).unwrap(), [1, 0x8000, 0xffff]);
        assert!(out.remove(0).into_low().is_err());
        assert!(out.remove(0).into_tensor().is_err());
        assert_eq!(
            out.pop().unwrap().into_tensor().unwrap().dtype(),
            MlxDtype::F32
        );
    }
    traces(&program, 2);
}

#[test]
fn typed_builder_rejections_preserve_valid_empty_nodes_and_input_slots() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    assert!(matches!(
        graph.input_low(LowDtype::F16, shape(&[i32::MAX as usize + 1])),
        Err(MlxError::TooLarge)
    ));
    let h = graph.input_low(LowDtype::F16, shape(&[2, 0])).unwrap();
    let bf = graph.input_low(LowDtype::Bf16, shape(&[2, 0])).unwrap();
    let u = graph.input_u32(shape(&[2, 0])).unwrap();
    let f = graph.input(shape(&[2, 0])).unwrap();
    let mut other = b.program();
    let foreign = other.input_low(LowDtype::F16, shape(&[2, 0])).unwrap();
    assert!(matches!(
        graph.unary_low(foreign, UnaryOp::Abs),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        graph.cast_to_f32(foreign),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        graph.binary_low(h, foreign, BinaryOp::Add),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        graph.reduce_low_f32(foreign, ReduceOp::Sum, &[0], false),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        graph.matmul_low_f32(h, foreign),
        Err(MlxError::ForeignValue)
    ));
    assert!(graph.binary_low(h, bf, BinaryOp::Add).is_err());
    let bf_columns = graph.permute(bf, &[1, 0]).unwrap();
    // K=0 with otherwise valid shapes must still reject mixed input dtypes.
    assert!(graph.matmul_low_f32(h, bf_columns).is_err());
    assert!(graph.unary_low(f, UnaryOp::Square).is_err());
    assert!(graph.unary(h, UnaryOp::Square).is_err());
    assert!(graph.cast_to_low(u, LowDtype::F16).is_err());
    assert!(graph.cast_to_f32(u).is_err());
    assert!(graph.reduce_u32(h, ReduceOp::Sum, &[0], false).is_err());
    assert!(graph.reduce_low_f32(f, ReduceOp::Sum, &[0], false).is_err());
    assert!(graph.binary_u32(u, u, BinaryOp::Divide).is_err());
    assert!(graph.mean_axes(u, &[0], false).is_err());
    for op in [ReduceOp::Min, ReduceOp::Max] {
        assert!(graph.reduce_low_f32(h, op, &[1], false).is_err());
        assert!(graph.reduce_u32(u, op, &[1], false).is_err());
    }
    assert!(graph.mean_low_f32(h, &[1], false).is_err());
    assert!(
        graph
            .reduce_low_f32(h, ReduceOp::Sum, &[0, 0], false)
            .is_err()
    );
    assert!(graph.reduce_low_f32(h, ReduceOp::Sum, &[2], false).is_err());
    let h = graph.unary_low(h, UnaryOp::Abs).unwrap();
    let program = graph.compile(&[h, bf, u, f]).unwrap();
    let h = b.upload_low(LowDtype::F16, shape(&[2, 0]), &[]).unwrap();
    let bf = b.upload_low(LowDtype::Bf16, shape(&[2, 0]), &[]).unwrap();
    let u = b.upload_u32(shape(&[2, 0]), &[]).unwrap();
    let f = b.upload_f32(shape(&[2, 0]), &[]).unwrap();
    // Empty values still must reject a swapped low dtype before tracing.
    assert!(matches!(
        program.run_typed(&[(&bf).into(), (&h).into(), (&u).into(), (&f).into()]),
        Err(MlxError::Dtype)
    ));
    traces(&program, 0);
    let out = program
        .run_typed(&[(&h).into(), (&bf).into(), (&u).into(), (&f).into()])
        .unwrap();
    low_values(&b, &out[0], &[2, 0], LowDtype::F16, &[]);
    low_values(&b, &out[1], &[2, 0], LowDtype::Bf16, &[]);
    assert!(b.read_u32(out[2].as_tensor().unwrap()).unwrap().is_empty());
    floats(&b, &out[3], &[2, 0], &[]);
    traces(&program, 1);
}

#[test]
fn lazy_custom_outputs_outlive_program_kernels_constants_and_original_inputs() {
    let Some(b) = backend() else { return };
    let reader = b.clone();
    let dtype = LowDtype::Bf16;
    let mut graph = b.program();
    let x = graph.input_low(dtype, shape(&[3, 2])).unwrap();
    let w = graph.input_low(dtype, shape(&[2, 3])).unwrap();
    let abs = graph.unary_low(x, UnaryOp::Abs).unwrap();
    let sum = graph
        .reduce_low_f32(abs, ReduceOp::Sum, &[0, 1], false)
        .unwrap();
    let product = graph.matmul_low_f32(x, w).unwrap();
    let low = graph.cast_to_low(sum, dtype).unwrap();
    let program = graph.compile(&[abs, sum, product, low]).unwrap();
    let x = b
        .upload_low(
            dtype,
            shape(&[3, 2]),
            &[-1., 2., -3., 4., -5., 6.].map(|x| encode(dtype, x)),
        )
        .unwrap();
    let w = b
        .upload_low(
            dtype,
            shape(&[2, 3]),
            &[1., 0., -1., 0.5, 2., 3.].map(|x| encode(dtype, x)),
        )
        .unwrap();
    let old = program.run_typed(&[(&x).into(), (&w).into()]).unwrap();
    let changed = b
        .upload_low(dtype, shape(&[3, 2]), &[encode(dtype, 2.); 6])
        .unwrap();
    let new = program
        .run_typed(&[(&changed).into(), (&w).into()])
        .unwrap();
    traces(&program, 2);
    drop((x, w, changed, program, b));
    // Neither set was evaluated before the handles and compiled closure died.
    floats(&reader, &new[1], &[], &[12.]);
    floats(&reader, &old[1], &[], &[21.]);
    low_values(
        &reader,
        &old[0],
        &[3, 2],
        dtype,
        &[1., 2., 3., 4., 5., 6.].map(|x| encode(dtype, x)),
    );
    floats(
        &reader,
        &old[2],
        &[3, 3],
        &[0., 4., 7., -1., 8., 15., -2., 12., 23.],
    );
    floats(
        &reader,
        &new[2],
        &[3, 3],
        &[3., 4., 4., 3., 4., 4., 3., 4., 4.],
    );
    low_values(&reader, &old[3], &[], dtype, &[encode(dtype, 21.)]);
    reader.synchronize().unwrap();
}
