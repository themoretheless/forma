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
mod tests;
