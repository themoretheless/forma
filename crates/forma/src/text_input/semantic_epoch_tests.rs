
#[test]
fn exhausted_epoch_never_reuses_an_earlier_cache_key() {
    let counter = std::sync::atomic::AtomicU32::new(u32::MAX - 1);
    assert_eq!(super::allocate_semantic_revision(&counter), u32::MAX);
    for _ in 0..4 {
        assert_eq!(super::allocate_semantic_revision(&counter), 0);
    }
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), u32::MAX);
}
