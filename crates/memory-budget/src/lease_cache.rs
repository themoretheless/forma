//! Single-owner cache with transferable consumer leases. No cache lock on lease drop.
use crate::{CacheLimits, WeightedLru};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Deref,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

/// Clones share both the immutable value and its eviction protection.
#[derive(Debug)]
pub struct CacheLease<V> {
    value: Option<Arc<V>>,
    released: Arc<AtomicBool>,
}
impl<V> Clone for CacheLease<V> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            released: self.released.clone(),
        }
    }
}
impl<V> Deref for CacheLease<V> {
    type Target = V;
    fn deref(&self) -> &V {
        self.value.as_deref().expect("live lease owns its value")
    }
}

impl<V> Drop for CacheLease<V> {
    fn drop(&mut self) {
        // Release the strong owner BEFORE notifying. Otherwise the owner could
        // consume the flag while still seeing the old strong count and miss
        // the final release indefinitely. No cache lock or payload copy.
        drop(self.value.take());
        self.released.store(true, Ordering::Release);
    }
}

/// Mutations reconcile released leases before selecting victims. The cache owner
/// remains single-threaded; leases can cross threads when the value permits it.
#[derive(Debug)]
pub struct LeaseCache<K, V> {
    inner: WeightedLru<K, Arc<V>>,
    leased: HashMap<K, Weak<V>>,
    pinned: HashSet<K>,
    released: Arc<AtomicBool>,
}
impl<K: Clone + Eq + Hash, V> LeaseCache<K, V> {
    pub fn new(limits: CacheLimits) -> Self {
        Self {
            inner: WeightedLru::with_limits(limits),
            leased: HashMap::new(),
            pinned: HashSet::new(),
            released: Arc::new(AtomicBool::new(false)),
        }
    }
    fn reconcile(&mut self) {
        if !self.released.swap(false, Ordering::AcqRel) {
            return;
        }
        self.leased.retain(|key, value| {
            // One strong owner belongs to the resident cache itself.
            if value.strong_count() <= 1 {
                if !self.pinned.contains(key) {
                    self.inner.unpin(key);
                }
                false
            } else {
                true
            }
        });
    }
    /// Count-only preflight; reconciles released leases before inspecting protection.
    pub fn can_admit_key_by_count(&mut self, key: &K) -> bool {
        self.reconcile();
        self.inner.can_admit_key_by_count(key)
    }
    /// Minimum-weight/count preflight after reconciling released leases.
    pub fn can_admit_key_by_minimum_weight(&mut self, key: &K, minimum: u64) -> bool {
        self.reconcile();
        self.inner.can_admit_key_by_minimum_weight(key, minimum)
    }
    pub fn capacity(&self) -> u64 {
        self.inner.capacity()
    }
    /// Explicit owner pin, independent of consumer leases. May protect expired entries.
    pub fn pin(&mut self, key: &K) -> bool {
        self.reconcile();
        if self.inner.pin(key) {
            self.pinned.insert(key.clone());
            true
        } else {
            false
        }
    }
    pub fn unpin(&mut self, key: &K) {
        self.reconcile();
        self.pinned.remove(key);
        if !self.leased.contains_key(key) {
            self.inner.unpin(key);
        }
    }
    /// Non-leased lookup for consumers managing lifetime separately.
    pub fn get_cloned(&mut self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        self.reconcile();
        self.inner.get(key).map(|value| (**value).clone())
    }
    /// Explicit invalidation clears the owner pin, but refuses live consumer leases.
    /// # Panics
    /// Panics if internal ownership bookkeeping would evict a still-shared value.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.reconcile();
        if self.leased.contains_key(key) {
            return None;
        }
        self.pinned.remove(key);
        self.inner
            .remove(key)
            .map(|value| into_owned(value, "removed value has no lease"))
    }
    /// # Panics
    /// Panics if internal ownership bookkeeping would evict a still-shared value.
    pub fn take_lru(&mut self) -> Option<(K, V)> {
        self.reconcile();
        self.inner
            .take_lru()
            .map(|(key, value)| (key, into_owned(value, UNPINNED_VICTIM)))
    }
    /// Expired objects are not handed to new consumers, even when an existing
    /// lease still protects their storage. TTL never invalidates a held lease.
    pub fn get(&mut self, key: &K) -> Option<CacheLease<V>> {
        self.reconcile();
        let value = self.inner.get(key)?.clone();
        self.inner.pin(key);
        self.leased.insert(key.clone(), Arc::downgrade(&value));
        Some(CacheLease {
            value: Some(value),
            released: self.released.clone(),
        })
    }
    /// Active leases forbid replacement as well as eviction. Rejection returns
    /// the caller's value without modifying resident membership.
    /// # Panics
    /// Panics if internal ownership bookkeeping would evict a still-shared value.
    /// # Errors
    /// Returns the supplied value when live leases or cache limits refuse admission.
    pub fn insert(&mut self, key: K, value: V, weight: u64) -> Result<Vec<(K, V)>, V> {
        self.reconcile();
        if self.leased.contains_key(&key) {
            return Err(value);
        }
        let value = Arc::new(value);
        let (accepted, victims) =
            self.inner
                .insert_prefetch_with_evictions(key, value.clone(), weight);
        if !accepted {
            return Err(into_owned(value, "rejected value has no cache owner"));
        }
        Ok(into_owned_all(victims, UNPINNED_VICTIM))
    }
    /// Failure preserves limits and membership when held leases cannot fit.
    /// # Panics
    /// Panics if internal ownership bookkeeping would evict a still-shared value.
    pub fn set_limits(&mut self, limits: CacheLimits) -> Option<Vec<(K, V)>> {
        self.reconcile();
        self.inner
            .set_limits(limits)
            .map(|victims| into_owned_all(victims, UNPINNED_VICTIM))
    }
    /// Transfers expired values for optional background spill. Live consumer leases
    /// and explicit owner pins retain their entries; no payload cloning or I/O occurs.
    /// Returned values continue to own their managed memory reservations.
    /// # Panics
    /// Panics if internal ownership bookkeeping would evict a still-shared value.
    pub fn drain_expired(&mut self) -> Vec<(K, V)> {
        self.reconcile();
        into_owned_all(self.inner.drain_expired(), "expired victim has no lease")
    }
    pub fn prune_expired(&mut self) -> usize {
        self.reconcile();
        self.inner.prune_expired()
    }
    pub fn resident_weight(&self) -> u64 {
        self.inner.resident_weight()
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

const UNPINNED_VICTIM: &str = "unpinned victim has no lease";

/// Takes a value back out of its `Arc` once the cache is its only owner. Leases and owner pins
/// keep shared values resident, so a value handed out here having another owner is a bookkeeping
/// bug, reported with `why`.
fn into_owned<V>(value: Arc<V>, why: &str) -> V {
    Arc::try_unwrap(value).ok().expect(why)
}

fn into_owned_all<K, V>(entries: Vec<(K, Arc<V>)>, why: &str) -> Vec<(K, V)> {
    entries
        .into_iter()
        .map(|(key, value)| (key, into_owned(value, why)))
        .collect()
}

#[cfg(test)]
mod tests {
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
            let victims =
                victims.unwrap_or_else(|| cache.set_limits(CacheLimits::bytes(0)).unwrap());
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
}
