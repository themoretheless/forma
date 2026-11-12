use super::*;
#[test]
fn interrupted_plans_recheck_replacement_pins_and_cutoff() {
    let start = Instant::now();
    let cutoff = start + Duration::from_secs(2);
    let mut cache = WeightedLru::with_limits(CacheLimits {
        max_bytes: 16,
        max_entries: None,
        ttl: Some(Duration::from_secs(1)),
    });
    for key in 0..4 {
        assert!(cache.insert_at(key, key, 1, start, true));
    }
    let keys = cache.expired_keys_for_prune(cutoff);
    assert_eq!(keys.len(), 4);
    // Abandon the first plan. A second plan must find the same entries.
    assert_eq!(cache.expired_keys_for_prune(cutoff).len(), 4);
    assert!(cache.pin(&1));
    assert!(cache.insert_at(2, 22, 1, cutoff, true));
    assert_eq!(cache.remove_if_expired_at(&1, cutoff), None);
    assert_eq!(cache.remove_if_expired_at(&2, cutoff), None);
    assert_eq!(cache.remove_if_expired_at(&0, cutoff), Some(0));
    assert_eq!(cache.remove_if_expired_at(&0, cutoff), None);
    assert_eq!(cache.remove_if_expired_at(&3, cutoff), Some(3));
    assert_eq!(cache.len(), 2);
    cache.unpin(&1);
    assert_eq!(cache.expired_keys_for_prune(cutoff), vec![1]);
    assert_eq!(cache.remove_if_expired_at(&1, cutoff), Some(1));
    assert_eq!(cache.get_at(&2, cutoff), Some(&22));
    assert_eq!(cache.prune_expired_at(cutoff + Duration::from_secs(1)), 1);
    assert_eq!(cache.resident_weight(), 0);
}
