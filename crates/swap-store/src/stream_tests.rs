use super::*;
#[test]
fn stream_roundtrip_rejects_partial_decoders_and_bad_encoders() {
    let store = SwapStore::new(
        std::env::temp_dir().as_path(),
        SwapLimits {
            max_bytes: 200_000,
            max_objects: None,
        },
    )
    .unwrap();
    let source: Vec<u8> = (0..150_000)
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect();
    let handle = store
        .write_stream(150_000, || false, |writer| writer.write_all(&source))
        .unwrap();
    let result = store
        .read_stream(
            &handle,
            || false,
            |reader| {
                let mut out = Vec::new();
                reader.read_to_end(&mut out)?;
                Ok(out)
            },
        )
        .unwrap();
    assert_eq!(result, source);
    assert!(matches!(
        store.read_stream(
            &handle,
            || false,
            |reader| {
                let mut byte = [0];
                reader.read_exact(&mut byte)?;
                Ok(byte)
            }
        ),
        Err(SwapError::Length)
    ));
    // Read-ahead can fetch the whole file, but a codec consuming only one
    // byte must still fail exact-length validation.
    let tiny = store
        .write_stream(4, || false, |writer| writer.write_all(&[1, 2, 3, 4]))
        .unwrap();
    assert!(matches!(
        store.read_stream(
            &tiny,
            || false,
            |reader| {
                let mut byte = [0];
                reader.read_exact(&mut byte)?;
                Ok(byte)
            }
        ),
        Err(SwapError::Length)
    ));
    let flushed = store
        .write_stream(
            5,
            || false,
            |writer| {
                writer.write_all(&[1, 2])?;
                writer.flush()?;
                writer.flush()?;
                writer.write_all(&[3, 4, 5])
            },
        )
        .unwrap();
    let restored = store
        .restore(&flushed, &MemoryBudget::new(5), || false)
        .unwrap();
    assert_eq!(&*restored, &[1, 2, 3, 4, 5]);
    assert_stream_rejections(&store, &handle);
}

fn assert_stream_rejections(store: &SwapStore, handle: &SwapHandle) {
    let before = store.usage().unwrap();
    assert!(
        store
            .write_stream(2, || false, |writer| writer.write_all(&[1]))
            .is_err()
    );
    assert!(
        store
            .write_stream(1, || false, |writer| writer.write_all(&[1, 2]))
            .is_err()
    );
    assert_eq!(store.usage().unwrap(), before);
    let mut calls = 0;
    assert!(matches!(
        store.read_stream(
            handle,
            || {
                calls += 1;
                calls > 2
            },
            |reader| {
                let mut out = Vec::new();
                reader.read_to_end(&mut out)?;
                Ok(out)
            }
        ),
        Err(SwapError::Cancelled)
    ));
    std::fs::write(handle.0.file.path(), vec![124; 150_000]).unwrap();
    assert!(matches!(
        store.read_stream(
            handle,
            || false,
            |reader| {
                let mut out = Vec::new();
                reader.read_to_end(&mut out)?;
                Ok(out)
            }
        ),
        Err(SwapError::Corrupt)
    ));
}
