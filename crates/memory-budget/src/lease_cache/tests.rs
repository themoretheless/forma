use super::*;
use crate::MemoryBudget;
#[test]
fn cross_thread_clone_release_notifies_after_ownership_changes() {
    let mut cache = LeaseCache::new(CacheLimits::bytes(8));
    cache.insert(1u8, 42u32, 4).unwrap();
    let lease = cache.get(&1).unwrap();
    let cloned = lease.clone();
    assert!(!cache.released.load(Ordering::Acquire));
    std::thread::spawn(move || drop(cloned)).join().unwrap();
    assert!(cache.released.load(Ordering::Acquire));
    // A clone release triggers reconciliation but must keep the remaining
    // consumer protected. Subsequent operations have no release to scan.
    assert!(cache.set_limits(CacheLimits::bytes(0)).is_none());
    assert!(!cache.released.load(Ordering::Acquire));
    assert_eq!(*lease, 42);
    assert!(cache.set_limits(CacheLimits::bytes(0)).is_none());
    std::thread::spawn(move || drop(lease)).join().unwrap();
    assert!(cache.released.load(Ordering::Acquire));
    let victims = cache.set_limits(CacheLimits::bytes(0)).unwrap();
    assert_eq!(victims, vec![(1, 42)]);
    assert!(cache.is_empty());
}

#[test]
fn count_preflight_reconciles_last_lease_and_preserves_owner_pins() {
    let mut cache = LeaseCache::new(CacheLimits {
        max_bytes: 16,
        max_entries: Some(2),
        ttl: None,
    });
    cache.insert(1u8, 11u8, 1).unwrap();
    cache.insert(2, 22, 1).unwrap();
    assert!(cache.pin(&1));
    let lease = cache.get(&2).unwrap();
    let clone = lease.clone();
    assert!(!cache.can_admit_key_by_count(&3));
    assert!(
        cache.can_admit_key_by_count(&1),
        "existing membership is not a new slot"
    );
    drop(lease);
    assert!(
        !cache.can_admit_key_by_count(&3),
        "last clone still protects slot"
    );
    assert!(cache.pin(&2));
    drop(clone);
    assert!(
        !cache.can_admit_key_by_count(&3),
        "owner pin survives consumer release"
    );
    cache.unpin(&2);
    assert!(cache.can_admit_key_by_count(&3));
    assert_eq!((cache.len(), cache.resident_weight()), (2, 2));
    assert_eq!(cache.get_cloned(&1), Some(11));
    let mut disabled: LeaseCache<u8, u8> = LeaseCache::new(CacheLimits {
        max_bytes: 16,
        max_entries: Some(0),
        ttl: None,
    });
    assert!(!disabled.can_admit_key_by_count(&1));
    let mut unlimited: LeaseCache<u8, u8> = LeaseCache::new(CacheLimits::bytes(0));
    assert!(
        unlimited.can_admit_key_by_count(&1),
        "byte admission is a separate constraint"
    );
}

#[test]
fn concurrent_release_and_eviction_retain_budget_until_last_owner() {
    for _ in 0..64 {
        let budget = MemoryBudget::new(64);
        let mut cache = LeaseCache::new(CacheLimits::bytes(64));
        cache
            .insert(1u8, budget.try_buffer(64, 93u8).unwrap().freeze(), 64)
            .unwrap();
        let keeper = cache.get(&1).unwrap();
        let consumers: Vec<_> = (0..16).map(|_| keeper.clone()).collect();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            for lease in consumers {
                assert!(lease.iter().all(|&v| v == 93));
                drop(lease);
                std::thread::yield_now();
            }
        });
        barrier.wait();
        for _ in 0..32 {
            // Keeper alone forbids eviction, regardless of worker releases.
            assert!(cache.set_limits(CacheLimits::bytes(0)).is_none());
            assert_eq!(budget.used(), 64);
            std::thread::yield_now();
        }
        drop(keeper);
        let mut victims = None;
        for _ in 0..128 {
            if let Some(values) = cache.set_limits(CacheLimits::bytes(0)) {
                victims = Some(values);
                break;
            }
            std::thread::yield_now();
        }
        worker.join().unwrap();
        let victims = victims.unwrap_or_else(|| cache.set_limits(CacheLimits::bytes(0)).unwrap());
        assert_eq!(victims.len(), 1);
        assert_eq!(budget.used(), 64); // Eviction transfers ownership.
        assert!(victims[0].1.iter().all(|&v| v == 93));
        drop(victims);
        assert_eq!(budget.used(), 0);
        assert!(cache.is_empty());
    }
}

#[test]
fn drain_expired_respects_owner_pins_and_returns_ownership() {
    let budget = MemoryBudget::new(4);
    let mut cache = LeaseCache::new(CacheLimits {
        max_bytes: 4,
        max_entries: Some(1),
        ttl: Some(std::time::Duration::ZERO),
    });
    let pixels = budget.try_buffer(4, 91u8).unwrap().freeze();
    let pointer = pixels.as_ptr();
    cache.insert(1, pixels, 4).unwrap();
    assert!(cache.pin(&1));
    assert!(cache.drain_expired().is_empty());
    assert!(cache.get(&1).is_none());
    cache.unpin(&1);
    let victims = cache.drain_expired();
    assert_eq!(victims.len(), 1);
    assert_eq!(victims[0].0, 1);
    assert_eq!(victims[0].1.as_ptr(), pointer);
    assert_eq!(budget.used(), 4);
    assert_eq!(cache.resident_weight(), 0);
    drop(victims);
    assert_eq!(budget.used(), 0);
}
#[test]
fn last_consumer_release_allows_spill_and_preserves_allocation_accounting() {
    let budget = MemoryBudget::new(8);
    let mut cache = LeaseCache::new(CacheLimits {
        max_bytes: 8,
        max_entries: Some(2),
        ttl: None,
    });
    cache
        .insert(1, budget.try_buffer(4, 1u8).unwrap().freeze(), 4)
        .unwrap();
    cache
        .insert(2, budget.try_buffer(4, 2u8).unwrap().freeze(), 4)
        .unwrap();
    let lease = cache.get(&1).unwrap();
    let second = lease.clone();
    assert!(cache.set_limits(CacheLimits::bytes(3)).is_none());
    let victims = cache.set_limits(CacheLimits::bytes(4)).unwrap();
    assert_eq!(victims.len(), 1);
    assert_eq!(victims[0].0, 2);
    assert_eq!(budget.used(), 8); // returned spill owner still retains bytes
    drop(victims);
    assert_eq!(budget.used(), 4);
    let rejected = cache
        .insert(3, budget.try_buffer(1, 3u8).unwrap().freeze(), 1)
        .unwrap_err();
    assert_eq!(budget.used(), 5);
    assert_eq!(cache.len(), 1);
    drop(rejected);
    assert_eq!(budget.used(), 4);
    assert!(
        cache
            .insert(1, budget.try_buffer(1, 9u8).unwrap().freeze(), 1)
            .is_err()
    );
    drop(lease);
    assert!(cache.set_limits(CacheLimits::bytes(0)).is_none());
    std::thread::spawn(move || {
        assert_eq!(&**second, &[1; 4]);
    })
    .join()
    .unwrap();
    let victims = cache.set_limits(CacheLimits::bytes(0)).unwrap();
    assert_eq!(cache.len(), 0);
    assert_eq!(budget.used(), 4);
    drop(victims);
    assert_eq!(budget.used(), 0);
}
#[test]
fn explicit_invalidation_refuses_consumers_and_clears_owner_pin() {
    let mut cache = LeaseCache::new(CacheLimits::bytes(1));
    cache.insert(1, 7u8, 1).unwrap();
    assert!(cache.pin(&1));
    let lease = cache.get(&1).unwrap();
    assert!(cache.remove(&1).is_none());
    assert_eq!(*lease, 7);
    drop(lease);
    assert_eq!(cache.remove(&1), Some(7));
    cache.insert(1, 8, 1).unwrap();
    assert_eq!(cache.take_lru(), Some((1, 8)));
}
#[test]
fn zero_ttl_entries_expire_without_invalidating_existing_leases() {
    let mut cache = LeaseCache::new(CacheLimits::bytes(1));
    cache.insert(1, 7u8, 1).unwrap();
    let lease = cache.get(&1).unwrap();
    // A deterministic zero TTL replacement cannot affect the held entry.
    assert!(
        cache
            .set_limits(CacheLimits {
                max_bytes: 2,
                max_entries: None,
                ttl: Some(std::time::Duration::ZERO)
            })
            .is_some()
    );
    cache.insert(2, 8u8, 1).unwrap();
    assert!(cache.get(&2).is_none());
    assert_eq!(*lease, 7);
    drop(lease);
    assert_eq!(cache.set_limits(CacheLimits::bytes(0)).unwrap().len(), 1);
}
#[test]
fn expired_last_lease_and_owner_pin_independently_block_replacement() {
    let budget = MemoryBudget::new(4);
    let mut cache = LeaseCache::new(CacheLimits {
        max_bytes: 4,
        max_entries: Some(1),
        ttl: Some(std::time::Duration::from_secs(1)),
    });
    cache
        .insert(1, budget.try_buffer(4, 31u8).unwrap().freeze(), 4)
        .unwrap();
    let lease = cache.get(&1).unwrap();
    let other = lease.clone();
    assert!(cache.pin(&1));
    std::thread::sleep(std::time::Duration::from_millis(1010));
    assert!(cache.get(&1).is_none());
    drop(lease);
    assert!(cache.drain_expired().is_empty());
    std::thread::spawn(move || {
        assert_eq!(&**other, &[31; 4]);
        drop(other);
    })
    .join()
    .unwrap();
    assert!(
        cache.drain_expired().is_empty(),
        "explicit owner pin survives last lease"
    );
    assert_eq!(budget.used(), 4);
    cache.unpin(&1);
    let expired = cache.drain_expired();
    assert_eq!(expired.len(), 1);
    assert_eq!(budget.used(), 4, "spill victim still owns allocation");
    drop(expired);
    assert_eq!(budget.used(), 0);
    cache
        .insert(2, budget.try_buffer(4, 32u8).unwrap().freeze(), 4)
        .unwrap();
    assert_eq!(&**cache.get(&2).unwrap(), &[32; 4]);
}
