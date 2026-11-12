use super::*;
use crate::MemoryBudget;
#[test]
fn fitting_policy_update_does_not_clone_collection_keys_or_change_lru() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Debug)]
    struct Key(u32, Arc<AtomicUsize>);
    impl Clone for Key {
        fn clone(&self) -> Self {
            self.1.fetch_add(1, Ordering::Relaxed);
            Self(self.0, self.1.clone())
        }
    }
    impl PartialEq for Key {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }
    impl Eq for Key {}
    impl Hash for Key {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.0.hash(state);
        }
    }
    let clones = Arc::new(AtomicUsize::new(0));
    let mut cache = WeightedLru::new(100_000);
    for id in 0..100_000 {
        assert!(cache.insert(Key(id, clones.clone()), id, 1));
    }
    assert_eq!(cache.get(&Key(0, clones.clone())), Some(&0));
    clones.store(0, Ordering::Relaxed);
    let limits = CacheLimits {
        max_bytes: 100_000,
        max_entries: Some(100_000),
        ttl: Some(Duration::ZERO),
    };
    assert!(cache.set_limits(limits).unwrap().is_empty());
    assert_eq!(clones.load(Ordering::Relaxed), 0);
    assert_eq!(cache.len(), 100_000);
    assert_eq!(
        cache.drain_expired().len(),
        0,
        "existing deadlines stay fixed"
    );
    assert_eq!(
        cache.take_lru().map(|(key, value)| (key.0, value)),
        Some((1, 1))
    );
}
#[test]
fn resizing_preserves_pins_and_returns_ordered_spill_without_partial_failure() {
    let mut cache = WeightedLru::new(12);
    for key in ["old", "recent", "visible"] {
        assert!(cache.insert(key, key, 4));
    }
    cache.pin(&"visible");
    let clock = cache.clock;
    assert!(cache.set_limits(CacheLimits::bytes(3)).is_none());
    assert!(
        cache
            .set_limits(CacheLimits {
                max_bytes: 12,
                max_entries: Some(0),
                ttl: None
            })
            .is_none()
    );
    assert_eq!(cache.clock, clock);
    assert_eq!(cache.resident_weight(), 12);
    assert_eq!(cache.capacity(), 12);
    let victims = cache
        .set_limits(CacheLimits {
            max_bytes: 8,
            max_entries: Some(1),
            ttl: None,
        })
        .unwrap();
    assert_eq!(victims, vec![("old", "old"), ("recent", "recent")]);
    assert_eq!(cache.get(&"visible"), Some(&"visible"));
    cache.unpin(&"visible");
    assert_eq!(
        cache.set_limits(CacheLimits::bytes(0)).unwrap(),
        vec![("visible", "visible")]
    );
    assert!(cache.is_empty());
}
#[test]
fn disabling_ttl_does_not_resurrect_entries_or_change_expiration_order() {
    let now = Instant::now();
    let mut cache = WeightedLru::with_limits(CacheLimits {
        max_bytes: 2,
        max_entries: None,
        ttl: Some(Duration::ZERO),
    });
    assert!(cache.insert_at("expired", 1, 1, now, true));
    assert!(cache.set_limits(CacheLimits::bytes(2)).unwrap().is_empty());
    assert!(cache.insert("live", 2, 1));
    assert_eq!(cache.take_lru(), Some(("expired", 1)));
    assert_eq!(cache.get(&"live"), Some(&2));
    cache
        .set_limits(CacheLimits {
            max_bytes: 2,
            max_entries: None,
            ttl: Some(Duration::ZERO),
        })
        .unwrap();
    assert!(cache.insert_at("expired_again", 3, 1, now, true));
    assert_eq!(cache.get(&"expired_again"), None);
    assert_eq!(cache.len(), 1);
}
#[test]
fn count_and_byte_limits_are_independent_and_replacement_is_transactional() {
    let mut cache = WeightedLru::with_limits(CacheLimits {
        max_bytes: 10,
        max_entries: Some(2),
        ttl: None,
    });
    assert!(cache.insert_prefetch("visible", 1, 6));
    assert!(cache.pin(&"visible"));
    assert!(cache.insert_prefetch("other", 2, 2));
    assert!(!cache.insert_prefetch("other", 99, 5));
    assert_eq!(cache.get(&"other"), Some(&2));
    assert!(cache.insert_prefetch("new", 3, 2));
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.resident_weight(), 8);
    assert!(cache.get(&"other").is_none());
    assert!(cache.pin(&"new"));
    assert!(!cache.insert_prefetch("third", 4, 0));
    assert_eq!(cache.len(), 2);
    let mut zero = WeightedLru::with_limits(CacheLimits {
        max_bytes: 10,
        max_entries: Some(0),
        ttl: None,
    });
    assert!(!zero.insert("a", 1, 0));
    assert!(zero.is_empty());
}
#[test]
fn ttl_is_insertion_age_not_idle_age_and_boundary_is_exact() {
    let now = Instant::now();
    let mut cache = WeightedLru::with_limits(CacheLimits {
        max_bytes: 10,
        max_entries: None,
        ttl: Some(Duration::from_secs(10)),
    });
    assert!(cache.insert_at("a", 1, 2, now, true));
    assert_eq!(cache.get_at(&"a", now + Duration::from_secs(9)), Some(&1));
    assert_eq!(cache.get_at(&"a", now + Duration::from_secs(10)), None);
    assert!(cache.is_empty());
    assert_eq!(cache.resident_weight(), 0);
    assert!(cache.insert_at("a", 2, 2, now, true));
    assert!(cache.insert_at("a", 3, 2, now + Duration::from_secs(9), true));
    assert_eq!(cache.get_at(&"a", now + Duration::from_secs(10)), Some(&3));
    assert_eq!(cache.get_at(&"a", now + Duration::from_secs(19)), None);
}
#[test]
fn unrepresentable_ttl_rejects_admission_without_eviction_or_replacement() {
    let now = Instant::now();
    assert!(now.checked_add(Duration::MAX).is_none());
    let mut cache = WeightedLru::new(2);
    assert!(cache.insert_at("old", 1, 2, now, true));
    cache.limits.ttl = Some(Duration::MAX);
    let mut victims = Vec::new();
    assert!(!cache.insert_at_collect("new", 2, 2, Some(now), true, &mut victims));
    assert!(!cache.insert_at_collect("old", 3, 2, Some(now), true, &mut victims));
    assert!(victims.is_empty());
    assert_eq!(cache.resident_weight(), 2);
    assert_eq!(cache.get_at(&"old", now), Some(&1));
    assert_eq!(cache.len(), 1);
}
#[test]
fn maximum_byte_capacity_keeps_admission_arithmetic_exact() {
    let mut cache = WeightedLru::new(u64::MAX);
    assert!(cache.insert("large", 1, u64::MAX - 1));
    assert!(cache.pin(&"large"));
    assert!(cache.insert_prefetch("small", 2, 1));
    assert_eq!(cache.resident_weight(), u64::MAX);
    let (admitted, victims) = cache.insert_prefetch_with_evictions("too_big", 3, 2);
    assert!(!admitted);
    assert!(victims.is_empty());
    assert_eq!(cache.resident_weight(), u64::MAX);
    assert_eq!(cache.get(&"small"), Some(&2));
    cache.unpin(&"large");
    assert!(cache.insert_prefetch("replacement", 4, u64::MAX));
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.resident_weight(), u64::MAX);
}
#[test]
fn expired_pinned_data_and_external_owners_keep_memory_accounted() {
    let now = Instant::now();
    let budget = MemoryBudget::new(8);
    let buffer = budget.try_buffer(4, 7_u16).unwrap().freeze();
    let consumer = buffer.clone();
    let mut cache = WeightedLru::with_limits(CacheLimits {
        max_bytes: 8,
        max_entries: Some(1),
        ttl: Some(Duration::from_secs(1)),
    });
    assert!(cache.insert_at("visible", buffer, 8, now, true));
    assert!(cache.pin(&"visible"));
    let expired = now + Duration::from_secs(1);
    assert!(cache.get_at(&"visible", expired).is_none());
    assert_eq!(cache.prune_expired_at(expired), 0);
    assert_eq!(cache.resident_weight(), 8);
    assert_eq!(budget.used(), 8);
    assert!(budget.try_buffer(1, 0_u8).is_err());
    cache.unpin(&"visible");
    assert_eq!(cache.prune_expired_at(expired), 1);
    assert_eq!(cache.resident_weight(), 0);
    assert_eq!(budget.used(), 8);
    assert_eq!(consumer[0], 7);
    drop(consumer);
    assert_eq!(budget.used(), 0);
}
#[test]
fn expired_entries_are_preferred_over_live_lru_victims() {
    let now = Instant::now();
    let mut cache = WeightedLru::with_limits(CacheLimits {
        max_bytes: 4,
        max_entries: Some(2),
        ttl: Some(Duration::from_secs(10)),
    });
    assert!(cache.insert_at("old", 1, 2, now, true));
    assert!(cache.insert_at("live", 2, 2, now + Duration::from_secs(5), true));
    assert_eq!(cache.get_at(&"old", now + Duration::from_secs(9)), Some(&1));
    assert!(cache.insert_at("new", 3, 2, now + Duration::from_secs(10), true));
    assert_eq!(
        cache.get_at(&"live", now + Duration::from_secs(10)),
        Some(&2)
    );
    assert_eq!(cache.get_at(&"old", now + Duration::from_secs(10)), None);
}
#[test]
fn spill_receives_only_successful_unpinned_evictions() {
    let mut cache = WeightedLru::new(4);
    assert!(cache.insert("visible", 1, 2));
    assert!(cache.pin(&"visible"));
    assert!(cache.insert("old", 2, 2));
    let (admitted, victims) = cache.insert_prefetch_with_evictions("new", 3, 2);
    assert!(admitted);
    assert_eq!(victims, vec![("old", 2)]);
    assert_eq!(cache.get(&"visible"), Some(&1));
    let (admitted, victims) = cache.insert_prefetch_with_evictions("large", 4, 4);
    assert!(!admitted);
    assert!(victims.is_empty());
    assert_eq!(cache.get(&"new"), Some(&3));
    assert_eq!(cache.resident_weight(), 4);
}
