use super::*;
#[test]
fn mixed_small_and_bulk_restore_keeps_prefix_tail_and_hash_exact() {
    let source: Vec<u8> = (0..BLOCK * 4 + 97)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    let store = make_store(source.len() as u64, Some(1));
    let handle = store
        .write_stream(
            source.len() as u64,
            || false,
            |writer| writer.write_all(&source),
        )
        .unwrap();
    let restored = store
        .read_stream(
            &handle,
            || false,
            |reader| {
                let mut prefix = [0u8; 17];
                reader.read_exact(&mut prefix)?;
                let mut bulk = vec![0u8; BLOCK * 3];
                reader.read_exact(&mut bulk)?;
                let mut tail = Vec::new();
                reader.read_to_end(&mut tail)?;
                let mut result = prefix.to_vec();
                result.extend(bulk);
                result.extend(tail);
                Ok(result)
            },
        )
        .unwrap();
    assert_eq!(restored, source);
}

fn make_store(bytes: u64, objects: Option<usize>) -> SwapStore {
    SwapStore::new(
        std::env::temp_dir().as_path(),
        SwapLimits {
            max_bytes: bytes,
            max_objects: objects,
        },
    )
    .unwrap()
}
#[test]
fn streaming_cancellation_stays_cancelled_after_a_single_signal() {
    let store = make_store(8, Some(2));
    for swallow in [false, true] {
        let mut polls = 0;
        let result = store.write_stream(
            4,
            || {
                polls += 1;
                polls == 2
            },
            |writer| {
                let result = writer.write_all(&[1, 2, 3, 4]);
                if swallow { Ok(()) } else { result }
            },
        );
        assert!(matches!(result, Err(SwapError::Cancelled)));
        assert_eq!(
            store.usage().unwrap(),
            SwapUsage {
                bytes: 0,
                objects: 0
            }
        );
    }
    let handle = store
        .write_chunks(4, [&[1, 2, 3, 4][..]], || false)
        .unwrap();
    for swallow in [false, true] {
        let mut polls = 0;
        let result = store.read_stream(
            &handle,
            || {
                polls += 1;
                polls == 2
            },
            |reader| {
                let mut output = [0; 4];
                let result = reader.read_exact(&mut output);
                if swallow {
                    Ok(output)
                } else {
                    result.map(|()| output)
                }
            },
        );
        assert!(matches!(result, Err(SwapError::Cancelled)));
        assert_eq!(
            store.usage().unwrap(),
            SwapUsage {
                bytes: 4,
                objects: 1
            }
        );
    }
    assert_eq!(
        &*store
            .restore(&handle, &MemoryBudget::new(4), || false)
            .unwrap(),
        &[1, 2, 3, 4]
    );
    drop(handle);
    assert_eq!(
        store.usage().unwrap(),
        SwapUsage {
            bytes: 0,
            objects: 0
        }
    );
}
#[test]
fn roundtrip_shared_handles_and_buffers_retain_their_own_quotas() {
    let store = make_store(8, Some(1));
    let source = [1, 2, 3, 4, 5, 6, 7, 8];
    let handle = store
        .write_chunks(8, [&source[..3], &source[3..]], || false)
        .unwrap();
    let other = handle.clone();
    assert_eq!(
        store.usage().unwrap(),
        SwapUsage {
            bytes: 8,
            objects: 1
        }
    );
    assert!(matches!(
        store.write_chunks(0, [&[][..]], || false),
        Err(SwapError::Quota)
    ));
    let budget = MemoryBudget::new(8);
    let restored = store.restore(&handle, &budget, || false).unwrap();
    assert_eq!(&*restored, source);
    let consumer = restored.clone();
    drop(restored);
    assert_eq!(budget.used(), 8);
    let path = handle.0.file.path().to_path_buf();
    drop(handle);
    assert!(path.exists());
    assert_eq!(store.usage().unwrap().bytes, 8);
    drop(other);
    assert!(!path.exists());
    assert_eq!(store.usage().unwrap(), SwapUsage::default());
    assert_eq!(&*consumer, source);
    drop(consumer);
    assert_eq!(budget.used(), 0);
}
#[test]
fn cancellation_failed_length_and_ram_failure_leave_source_and_usage_intact() {
    let store = make_store(200_000, None);
    let source = vec![42; 150_000];
    let mut calls = 0;
    assert!(matches!(
        store.write_chunks(150_000, [source.as_slice()], || {
            calls += 1;
            calls > 3
        }),
        Err(SwapError::Cancelled)
    ));
    assert_eq!(source, vec![42; 150_000]);
    assert_eq!(store.usage().unwrap(), SwapUsage::default());
    for bytes in [149_999, 150_001] {
        assert!(matches!(
            store.write_chunks(bytes, [source.as_slice()], || false),
            Err(SwapError::Length)
        ));
        assert_eq!(store.usage().unwrap(), SwapUsage::default());
    }
    let handle = store
        .write_chunks(150_000, [source.as_slice()], || false)
        .unwrap();
    let small = MemoryBudget::new(1);
    assert!(matches!(
        store.restore(&handle, &small, || false),
        Err(SwapError::Memory(_))
    ));
    assert_eq!(small.used(), 0);
    let budget = MemoryBudget::new(150_000);
    let mut calls = 0;
    assert!(matches!(
        store.restore(&handle, &budget, || {
            calls += 1;
            calls > 2
        }),
        Err(SwapError::Cancelled)
    ));
    assert_eq!(budget.used(), 0);
    assert_eq!(store.usage().unwrap().bytes, 150_000);
}
#[test]
fn corruption_truncation_trailing_and_foreign_handles_reject() {
    let store = make_store(16, None);
    let other = make_store(16, None);
    let handle = store
        .write_chunks(4, [&[1, 2, 3, 4][..]], || false)
        .unwrap();
    let budget = MemoryBudget::new(4);
    assert!(matches!(
        other.restore(&handle, &budget, || false),
        Err(SwapError::ForeignHandle)
    ));
    assert_eq!(budget.used(), 0);
    std::fs::write(handle.0.file.path(), [1, 2, 3, 9]).unwrap();
    assert!(matches!(
        store.restore(&handle, &budget, || false),
        Err(SwapError::Corrupt)
    ));
    assert_eq!(budget.used(), 0);
    std::fs::write(handle.0.file.path(), [1, 2, 3]).unwrap();
    assert!(matches!(
        store.restore(&handle, &budget, || false),
        Err(SwapError::Io(_))
    ));
    assert_eq!(budget.used(), 0);
    std::fs::write(handle.0.file.path(), [1, 2, 3, 4, 5]).unwrap();
    assert!(matches!(
        store.restore(&handle, &budget, || false),
        Err(SwapError::Length)
    ));
    assert_eq!(budget.used(), 0);
}
#[test]
fn handles_keep_store_alive_and_zero_count_refuses_admission() {
    let store = make_store(0, Some(1));
    let handle = store.write_chunks(0, [&[][..]], || false).unwrap();
    let root = store.0.directory.path().to_path_buf();
    drop(store);
    assert!(root.exists());
    drop(handle);
    assert!(!root.exists());
    assert!(matches!(
        make_store(0, Some(0)).write_chunks(0, [&[][..]], || false),
        Err(SwapError::Quota)
    ));
}
