use super::*;

#[test]
fn compiled_scan_compaction_validate_owners_types_shapes_and_empty_outputs() {
    let Some(b) = backend() else { return };
    let mut graph = b.program();
    let f = graph.input(shape(&[2, 0])).unwrap();
    let u = graph.input_u32(shape(&[2, 0])).unwrap();
    let h = graph.input_low(LowDtype::F16, shape(&[2, 0])).unwrap();
    let bf = graph.input_low(LowDtype::Bf16, shape(&[2, 0])).unwrap();
    let mask = graph.input_u32(shape(&[1, 0])).unwrap();
    let mut other = b.program();
    let foreign = other.input(shape(&[2, 0])).unwrap();
    assert!(matches!(
        graph.scan(foreign, 1, ScanOptions::default()),
        Err(MlxError::ForeignValue)
    ));
    assert!(matches!(
        graph.scan(u, 1, ScanOptions::default()),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        graph.scan_u32(f, 1, ScanOptions::default()),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(
        graph.scan_low_f32(f, 1, ScanOptions::default()),
        Err(MlxError::Dtype)
    ));
    assert!(matches!(graph.compact_low(f, mask), Err(MlxError::Dtype)));
    assert!(matches!(graph.compact(f, f), Err(MlxError::Dtype)));
    assert!(graph.scan(f, 2, ScanOptions::default()).is_err());
    assert!(graph.scan_low(h, 2, ScanOptions::default()).is_err());
    let scalar = graph.input(shape(&[])).unwrap();
    assert!(graph.scan(scalar, 0, ScanOptions::default()).is_err());
    let badmask = graph.input_u32(shape(&[3, 1])).unwrap();
    assert!(graph.compact_low(bf, badmask).is_err());
    let mut values = vec![
        graph.scan(f, 1, ScanOptions::default()).unwrap(),
        graph.scan_u32(u, 1, ScanOptions::default()).unwrap(),
        graph.scan_low_f32(h, 1, ScanOptions::default()).unwrap(),
        graph.scan_low(bf, 1, ScanOptions::default()).unwrap(),
    ];
    for pair in [
        graph.compact(f, mask).unwrap(),
        graph.compact_u32(u, mask).unwrap(),
        graph.compact_low(h, mask).unwrap(),
        graph.compact_low(bf, mask).unwrap(),
    ] {
        values.push(pair.values);
        values.push(pair.count);
    }
    let program = graph.compile(&values).unwrap();
    let f = b.upload_f32(shape(&[2, 0]), &[]).unwrap();
    let u = b.upload_u32(shape(&[2, 0]), &[]).unwrap();
    let h = b.upload_low(LowDtype::F16, shape(&[2, 0]), &[]).unwrap();
    let bf = b.upload_low(LowDtype::Bf16, shape(&[2, 0]), &[]).unwrap();
    let mask = b.upload_u32(shape(&[1, 0]), &[]).unwrap();
    let scalar = b.upload_f32(shape(&[]), &[0.]).unwrap();
    let badmask = b.upload_u32(shape(&[3, 1]), &[0; 3]).unwrap();
    let foreign = MlxBackend::new_gpu()
        .unwrap()
        .upload_f32(shape(&[2, 0]), &[])
        .unwrap();
    assert!(matches!(
        program.run_typed(&[
            (&foreign).into(),
            (&u).into(),
            (&h).into(),
            (&bf).into(),
            (&mask).into(),
            (&scalar).into(),
            (&badmask).into()
        ]),
        Err(MlxError::ForeignContext)
    ));
    traces(&program, 0);
    let out = program
        .run_typed(&[
            (&f).into(),
            (&u).into(),
            (&h).into(),
            (&bf).into(),
            (&mask).into(),
            (&scalar).into(),
            (&badmask).into(),
        ])
        .unwrap();
    for index in [0, 2, 4] {
        assert!(
            b.read_f32(out[index].as_tensor().unwrap())
                .unwrap()
                .is_empty()
        );
    }
    for index in [1, 6] {
        assert!(
            b.read_u32(out[index].as_tensor().unwrap())
                .unwrap()
                .is_empty()
        );
    }
    for index in [3, 8, 10] {
        assert!(
            b.read_low_bits(out[index].as_low().unwrap())
                .unwrap()
                .is_empty()
        );
    }
    for index in [5, 7, 9, 11] {
        assert_eq!(out[index].shape(), &shape(&[]));
        assert_eq!(b.read_u32(out[index].as_tensor().unwrap()).unwrap(), [0]);
    }
    traces(&program, 1);
}
