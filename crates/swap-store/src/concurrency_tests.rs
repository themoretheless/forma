use super::*;
#[test]
fn concurrent_writes_share_disk_quota_and_reservations_roll_back() {
    let store = SwapStore::new(
        std::env::temp_dir().as_path(),
        SwapLimits {
            max_bytes: 8,
            max_objects: Some(2),
        },
    )
    .unwrap();
    let gate = Arc::new(std::sync::Barrier::new(8));
    let successes = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let store = store.clone();
            let gate = gate.clone();
            let successes = &successes;
            scope.spawn(move || {
                let result = store.write_chunks(4, [&[1, 2, 3, 4][..]], || false);
                if result.is_ok() {
                    successes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                gate.wait();
                assert!(store.usage().unwrap().bytes <= 8);
                assert!(result.is_ok() || matches!(result, Err(SwapError::Quota)));
                drop(result);
            });
        }
    });
    assert_eq!(successes.load(std::sync::atomic::Ordering::Relaxed), 2);
    assert_eq!(store.usage().unwrap(), SwapUsage::default());
}
#[test]
fn concurrent_restores_share_ram_cap_without_consuming_disk_object() {
    use std::sync::atomic::Ordering;
    let directory = tempfile::tempdir().unwrap();
    let bytes = BLOCK as u64;
    let store = SwapStore::new(
        directory.path(),
        SwapLimits {
            max_bytes: bytes,
            max_objects: Some(1),
        },
    )
    .unwrap();
    let source = vec![73u8; BLOCK];
    let handle = store.write_chunks(bytes, [&source[..]], || false).unwrap();
    let budget = MemoryBudget::new(bytes);
    let children: Vec<_> = (0..8).map(|_| budget.child(bytes)).collect();
    let start = std::sync::Barrier::new(8);
    let retained = std::sync::Barrier::new(8);
    let successes = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for child in &children {
            let store = &store;
            let handle = &handle;
            let budget = child;
            let start = &start;
            let retained = &retained;
            let successes = &successes;
            scope.spawn(move || {
                start.wait();
                let restored = store.restore(handle, budget, || false);
                match &restored {
                    Ok(buffer) => {
                        assert!(buffer.iter().all(|sample| *sample == 73));
                        successes.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(error) => assert!(matches!(error, SwapError::Memory(_))),
                }
                // Keep the successful allocation live until all competing
                // requests have observed its budget reservation.
                retained.wait();
                assert_eq!(store.usage().unwrap(), SwapUsage { bytes, objects: 1 });
                drop(restored);
            });
        }
    });
    assert_eq!(successes.load(Ordering::Relaxed), 1);
    assert_eq!(budget.peak(), bytes);
    assert_eq!(budget.used(), 0);
    assert!(children.iter().all(|child| child.used() == 0));
    assert_eq!(children.iter().filter(|child| child.peak() != 0).count(), 1);
    // Local admission can fail after reserving its ancestor. That partial
    // reservation must roll back so an unrelated child can still restore.
    let too_small = budget.child(bytes - 1);
    assert!(matches!(
        store.restore(&handle, &too_small, || false),
        Err(SwapError::Memory(_))
    ));
    assert_eq!(too_small.used(), 0);
    assert_eq!(budget.used(), 0);
    let retry = store.restore(&handle, &budget, || false).unwrap();
    assert_eq!(&*retry, source.as_slice());
    drop(retry);
    drop(handle);
    assert_eq!(store.usage().unwrap(), SwapUsage::default());
    assert_eq!(budget.used(), 0);
}
#[test]
fn already_cancelled_restore_allocates_no_managed_memory() {
    let store = SwapStore::new(
        std::env::temp_dir().as_path(),
        SwapLimits {
            max_bytes: 1,
            max_objects: None,
        },
    )
    .unwrap();
    let handle = store.write_chunks(1, [&[42][..]], || false).unwrap();
    let budget = MemoryBudget::new(0);
    assert!(matches!(
        store.restore(&handle, &budget, || true),
        Err(SwapError::Cancelled)
    ));
    assert_eq!(budget.peak(), 0);
}

#[test]
fn multiblock_restore_cancellation_releases_ram_and_preserves_retry() {
    let directory = tempfile::tempdir().unwrap();
    let length = 2 * BLOCK + 17;
    let source: Vec<u8> = (0..length)
        .map(|index| u8::try_from(index % 251 + 1).unwrap())
        .collect();
    let store = SwapStore::new(
        directory.path(),
        SwapLimits {
            max_bytes: length as u64,
            max_objects: Some(1),
        },
    )
    .unwrap();
    let handle = store
        .write_chunks(length as u64, [&source[..]], || false)
        .unwrap();
    let expected_usage = store.usage().unwrap();
    let budget = MemoryBudget::new(length as u64);

    // Exercise every post-admission checkpoint, including cancellation after
    // the final block and digest verification, before publishing the buffer.
    for checkpoint in 2..=6 {
        let mut polls = 0;
        let mut live_at_cancellation = false;
        let result = store.restore(&handle, &budget, || {
            polls += 1;
            if polls == checkpoint {
                live_at_cancellation = budget.used() == length as u64;
                true
            } else {
                false
            }
        });
        assert!(
            matches!(result, Err(SwapError::Cancelled)),
            "checkpoint {checkpoint}"
        );
        assert!(live_at_cancellation);
        assert_eq!(budget.used(), 0);
        assert_eq!(store.usage().unwrap(), expected_usage);
        let restored = store.restore(&handle, &budget, || false).unwrap();
        assert_eq!(&*restored, &source);
        drop(restored);
        assert_eq!(budget.used(), 0);
    }
    drop(handle);
    assert_eq!(store.usage().unwrap(), SwapUsage::default());
}
