use std::{
    collections::{BTreeMap, HashMap, HashSet},
    hash::Hash,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy)]
pub struct CacheLimits {
    pub max_bytes: u64,
    pub max_entries: Option<usize>,
    /// Fixed lifetime from successful insertion/replacement; hits do not renew it.
    pub ttl: Option<Duration>,
}
impl CacheLimits {
    pub fn bytes(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            max_entries: None,
            ttl: None,
        }
    }
}
#[derive(Debug)]
struct Entry<V> {
    value: V,
    weight: u64,
    last_used: u64,
    /// This entry's key in `evictable`. Hits only advance `last_used`, so the rank can lag
    /// behind it until `settle` moves the entry to its real position.
    rank: u64,
    /// Whether the key is already in `touched`, so a hot entry is queued once per settle.
    queued: bool,
    deadline: Option<Instant>,
}
impl<V> Entry<V> {
    fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|v| now >= v)
    }
    /// An entry with a deadline is only ever checked with a clock reading.
    fn expired_at(&self, now: Option<Instant>) -> bool {
        now.is_some_and(|now| self.expired(now))
    }
}
/// Single-owner, byte-weighted LRU with independently optional count and TTL limits.
/// Membership weights do not account for external owners; managed `SharedBuffer`
/// allocations retain their own `MemoryBudget` reservation after cache eviction.
#[derive(Debug)]
pub struct WeightedLru<K, V> {
    entries: HashMap<K, Entry<V>>,
    capacity: u64,
    resident: u64,
    clock: u64,
    protected: HashSet<K>,
    evictable: BTreeMap<u64, K>,
    // Keys hit since the last `settle`; their `evictable` rank may be stale.
    touched: Vec<K>,
    limits: CacheLimits,
    // Track live deadlines so removing the last TTL entry restores indexed eviction.
    deadline_entries: usize,
    // Conservative earliest unprotected deadline; stale earlier hints only cause one scan.
    next_expiry: Option<Instant>,
}
impl<K: Clone + Eq + Hash, V> WeightedLru<K, V> {
    /// Whether the resident bytes and entry count already satisfy `limits`.
    fn fits(&self, limits: &CacheLimits) -> bool {
        self.resident <= limits.max_bytes
            && limits
                .max_entries
                .is_none_or(|max| self.entries.len() <= max)
    }
    /// The current time, read only when a TTL limit or a live deadline can use it.
    /// `Instant::now` panics on `wasm32-unknown-unknown`, so a cache without TTL never reads it.
    fn ttl_now(&self) -> Option<Instant> {
        (self.limits.ttl.is_some() || self.deadline_entries > 0).then(Instant::now)
    }
    /// Keeps `next_expiry` at the earliest deadline a pruner still has to visit.
    fn note_deadline(&mut self, deadline: Instant) {
        self.next_expiry = Some(self.next_expiry.map_or(deadline, |next| next.min(deadline)));
    }
    /// Moves every entry hit since the last call to its real recency rank, so `evictable`
    /// iterates in exact least-recently-used order again. Each hit costs at most one move.
    fn settle(&mut self) {
        // Reuse the list's allocation: a hit after settling must not allocate.
        let mut touched = std::mem::take(&mut self.touched);
        for key in touched.drain(..) {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            entry.queued = false;
            if entry.rank != entry.last_used && !self.protected.contains(&key) {
                self.evictable.remove(&entry.rank);
                self.evictable.insert(entry.last_used, key);
                entry.rank = entry.last_used;
            }
        }
        self.touched = touched;
    }
    pub fn new(capacity: u64) -> Self {
        Self::with_limits(CacheLimits::bytes(capacity))
    }
    pub fn with_limits(limits: CacheLimits) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: limits.max_bytes,
            resident: 0,
            clock: 0,
            protected: HashSet::new(),
            evictable: BTreeMap::new(),
            touched: Vec::new(),
            limits,
            deadline_entries: 0,
            next_expiry: None,
        }
    }
    pub fn limits(&self) -> CacheLimits {
        self.limits
    }
    /// Applies byte/count limits atomically, returning unpinned victims for spill.
    /// Returns `None` without changing entries or recency when pinned entries
    /// cannot fit. TTL changes affect future insertions; existing deadlines stay
    /// fixed. Expired entries are preferred over live LRU victims.
    /// # Panics
    /// Panics if a protected key is missing from the internal entry index.
    pub fn set_limits(&mut self, limits: CacheLimits) -> Option<Vec<(K, V)>> {
        let pinned_bytes = self
            .protected
            .iter()
            .fold(0_u64, |sum, key| sum + self.entries[key].weight);
        if pinned_bytes > limits.max_bytes
            || limits
                .max_entries
                .is_some_and(|max| self.protected.len() > max)
        {
            return None;
        }
        // Policy changes do not rewrite existing TTL deadlines. When membership
        // already fits, there are no victims to plan or keys to clone/sort.
        if self.fits(&limits) {
            self.capacity = limits.max_bytes;
            self.limits = limits;
            return Some(Vec::new());
        }
        let now = self.ttl_now();
        let mut candidates: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| !self.protected.contains(*key))
            .map(|(key, entry)| (!entry.expired_at(now), entry.last_used, key.clone()))
            .collect();
        candidates.sort_unstable_by_key(|entry| (entry.0, entry.1));
        let mut victims = Vec::new();
        for (_, _, key) in candidates {
            if self.fits(&limits) {
                break;
            }
            victims.push((
                key.clone(),
                self.remove(&key).expect("planned victim exists"),
            ));
        }
        self.capacity = limits.max_bytes;
        self.limits = limits;
        Some(victims)
    }
    pub fn capacity(&self) -> u64 {
        self.capacity
    }
    pub fn resident_weight(&self) -> u64 {
        self.resident
    }
    /// Stored entries, including expired protected entries awaiting release.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn pin(&mut self, key: &K) -> bool {
        if self.entries.contains_key(key) {
            if self.protected.insert(key.clone()) {
                self.evictable.remove(&self.entries[key].rank);
            }
            true
        } else {
            false
        }
    }
    pub fn unpin(&mut self, key: &K) {
        if self.protected.remove(key)
            && let Some(entry) = self.entries.get_mut(key)
        {
            entry.rank = entry.last_used;
            self.evictable.insert(entry.rank, key.clone());
            if let Some(deadline) = entry.deadline {
                self.note_deadline(deadline);
            }
        }
    }
    /// Reads a live value without changing recency.
    pub fn peek(&self, key: &K) -> Option<&V> {
        let entry = self.entries.get(key)?;
        (!entry.expired_at(self.ttl_now())).then_some(&entry.value)
    }
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self.deadline_entries > 0
            && self
                .entries
                .get(key)
                .is_some_and(|entry| entry.deadline.is_some())
        {
            self.get_at(key, Instant::now())
        } else {
            self.get_live(key)
        }
    }
    fn get_at(&mut self, key: &K, now: Instant) -> Option<&V> {
        if self.entries.get(key).is_some_and(|v| v.expired(now)) {
            if !self.protected.contains(key) {
                self.remove(key);
            }
            return None;
        }
        self.get_live(key)
    }
    fn next_clock(&mut self) {
        if self.clock == u64::MAX {
            let mut ordered: Vec<_> = self
                .entries
                .iter()
                .map(|(k, v)| (v.last_used, k.clone()))
                .collect();
            ordered.sort_unstable_by_key(|v| v.0);
            self.evictable.clear();
            self.touched.clear();
            for (index, (_, key)) in ordered.into_iter().enumerate() {
                let rank = index as u64 + 1;
                let entry = self.entries.get_mut(&key).unwrap();
                entry.last_used = rank;
                entry.rank = rank;
                entry.queued = false;
                if !self.protected.contains(&key) {
                    self.evictable.insert(rank, key);
                }
            }
            self.clock = self.entries.len() as u64;
        }
        self.clock += 1;
    }
    /// A hit is one hash lookup: it advances the clock and remembers the key for `settle`
    /// instead of moving it in `evictable`.
    fn get_live(&mut self, key: &K) -> Option<&V> {
        // Each live entry is queued at most once; only keys of removed entries can pile
        // up, so settle when they outnumber the live ones. Renumbering needs the whole
        // cache; both run before the lookup.
        if self.touched.len() > 2 * self.entries.len() + 16 {
            self.settle();
        }
        if self.clock == u64::MAX {
            self.next_clock();
        }
        let entry = self.entries.get_mut(key)?;
        // Repeated access to the most recent entry changes no eviction order.
        if entry.last_used != self.clock {
            self.clock += 1;
            entry.last_used = self.clock;
            if !entry.queued && (self.protected.is_empty() || !self.protected.contains(key)) {
                entry.queued = true;
                self.touched.push(key.clone());
            }
        }
        Some(&entry.value)
    }
    /// Transfers expired unprotected values to the caller without cloning them.
    /// Returned values retain allocation ownership until dropped or handed to a writer.
    /// Protected entries stay resident even after their deadline. Order is unspecified.
    pub fn drain_expired(&mut self) -> Vec<(K, V)> {
        if self.deadline_entries == 0 {
            return Vec::new();
        }
        self.drain_expired_at(Instant::now())
    }
    /// Plans expiry at a fixed cutoff without transferring values. The key plan
    /// uses the same storage as `prune_expired`; callers may release values in
    /// bounded batches outside their mutex. Plans may be abandoned or overlap.
    /// Recheck each key with `remove_if_expired_at`: replacement and pinning may
    /// have occurred since planning. Order is unspecified.
    pub fn expired_keys_for_prune(&mut self, cutoff: Instant) -> Vec<K> {
        if self.deadline_entries == 0 {
            return Vec::new();
        }
        let keys = self.expired_keys(cutoff);
        if !keys.is_empty() {
            // Until every planned entry is removed, a cancelled/overlapping
            // pruner must still be able to discover the remaining entries.
            self.note_deadline(cutoff);
        }
        keys
    }
    /// Transfers one still-expired, unprotected value at the plan's cutoff.
    /// Fresh replacements and entries pinned after planning are left untouched.
    pub fn remove_if_expired_at(&mut self, key: &K, cutoff: Instant) -> Option<V> {
        if !self.protected.contains(key)
            && self
                .entries
                .get(key)
                .is_some_and(|entry| entry.expired(cutoff))
        {
            self.remove(key)
        } else {
            None
        }
    }
    /// Drops expired unprotected entries; existing consumer-owned buffers survive.
    pub fn prune_expired(&mut self) -> usize {
        if self.deadline_entries == 0 {
            return 0;
        }
        self.prune_expired_at(Instant::now())
    }
    fn prune_expired_at(&mut self, now: Instant) -> usize {
        let keys = self.expired_keys(now);
        let count = keys.len();
        for key in keys {
            self.remove(&key);
        }
        count
    }
    fn expired_keys(&mut self, now: Instant) -> Vec<K> {
        if self.next_expiry.is_none_or(|deadline| now < deadline) {
            return Vec::new();
        }
        let mut expired = Vec::new();
        let mut next: Option<Instant> = None;
        for (key, entry) in &self.entries {
            if self.protected.contains(key) {
                continue;
            }
            if let Some(deadline) = entry.deadline {
                if now >= deadline {
                    expired.push(key.clone());
                } else {
                    next = Some(next.map_or(deadline, |old| old.min(deadline)));
                }
            }
        }
        self.next_expiry = next;
        expired
    }
    fn drain_expired_at(&mut self, now: Instant) -> Vec<(K, V)> {
        self.expired_keys(now)
            .into_iter()
            .map(|key| {
                let value = self.remove(&key).expect("expired resident exists");
                (key, value)
            })
            .collect()
    }
    fn deadline(&self, now: Option<Instant>) -> Result<Option<Instant>, ()> {
        match (self.limits.ttl, now) {
            (Some(ttl), Some(now)) => now.checked_add(ttl).map(Some).ok_or(()),
            (Some(_), None) => Err(()),
            (None, _) => Ok(None),
        }
    }
    /// Conservative count-only admission check without eviction or recency changes.
    /// Existing keys may replace membership; byte/TTL constraints still apply later.
    pub fn can_admit_key_by_count(&self, key: &K) -> bool {
        self.limits.max_entries.is_none_or(|max| {
            max > 0 && (self.entries.contains_key(key) || self.protected.len() < max)
        })
    }
    /// Conservative admission using only a lower bound on the new value's weight.
    /// Other protected entries cannot be evicted; an existing key can replace
    /// its own weight. Does not evict, renew TTL or change recency.
    pub fn can_admit_key_by_minimum_weight(&self, key: &K, minimum: u64) -> bool {
        if !self.can_admit_key_by_count(key) || minimum > self.capacity {
            return false;
        }
        let protected_others = self
            .protected
            .iter()
            .filter(|protected| *protected != key)
            .filter_map(|protected| self.entries.get(protected))
            .fold(0u64, |weight, entry| weight.saturating_add(entry.weight));
        minimum <= self.capacity.saturating_sub(protected_others)
    }
    fn needs_slot(&self) -> bool {
        self.limits
            .max_entries
            .is_some_and(|max| self.entries.len() >= max)
    }
    /// Plain insertion can evict protected entries; use `insert_prefetch` for pin-aware admission.
    pub fn insert(&mut self, key: K, value: V, weight: u64) -> bool {
        let now = self.ttl_now();
        self.insert_at_collect(key, value, weight, now, false, &mut Vec::new())
    }
    /// Rejected pin-aware admission preserves every existing value and recency.
    pub fn insert_prefetch(&mut self, key: K, value: V, weight: u64) -> bool {
        let now = self.ttl_now();
        self.insert_at_collect(key, value, weight, now, true, &mut Vec::new())
    }
    pub fn insert_prefetch_with_evictions(
        &mut self,
        key: K,
        value: V,
        weight: u64,
    ) -> (bool, Vec<(K, V)>) {
        let mut evicted = Vec::new();
        let now = self.ttl_now();
        let admitted = self.insert_at_collect(key, value, weight, now, true, &mut evicted);
        (admitted, evicted)
    }
    pub fn take_lru(&mut self) -> Option<(K, V)> {
        if self.deadline_entries == 0 {
            self.settle();
            let key = self.evictable.first_key_value()?.1.clone();
            return self.remove(&key).map(|value| (key, value));
        }
        let now = Instant::now();
        let key = self
            .entries
            .iter()
            .filter(|(k, _)| !self.protected.contains(*k))
            .min_by_key(|(_, value)| (!value.expired(now), value.last_used))
            .map(|(k, _)| k.clone())?;
        self.remove(&key).map(|value| (key, value))
    }
    #[cfg(test)]
    fn insert_at(&mut self, key: K, value: V, weight: u64, now: Instant, pin_aware: bool) -> bool {
        self.insert_at_collect(key, value, weight, Some(now), pin_aware, &mut Vec::new())
    }
    fn insert_at_collect(
        &mut self,
        key: K,
        value: V,
        weight: u64,
        now: Option<Instant>,
        pin_aware: bool,
        evicted: &mut Vec<(K, V)>,
    ) -> bool {
        if weight > self.capacity || self.limits.max_entries == Some(0) {
            return false;
        }
        let Ok(deadline) = self.deadline(now) else {
            return false;
        };
        let previous = self.entries.get(&key);
        let retained = self.resident - previous.map_or(0, |v| v.weight);
        let required = weight.saturating_sub(self.capacity - retained);
        let remaining = self.entries.len() - usize::from(previous.is_some());
        let slots = self
            .limits
            .max_entries
            .map_or(0, |max| remaining.saturating_add(1).saturating_sub(max));
        // Build the eviction plan once. Re-scanning the HashMap for each victim
        // makes a large admission quadratic in the number of resident entries.
        if pin_aware && slots <= 1 && self.deadline_entries == 0 && (required > 0 || slots > 0) {
            self.settle();
        }
        let single = if pin_aware && slots <= 1 && self.deadline_entries == 0 {
            self.evictable
                .iter()
                .find(|(_, candidate)| *candidate != &key)
                .and_then(|(rank, candidate)| {
                    let entry = &self.entries[candidate];
                    (entry.weight >= required)
                        .then(|| (true, *rank, candidate.clone(), entry.weight))
                })
        } else {
            None
        };
        let mut victims: Vec<_> = if required > 0 || slots > 0 {
            if let Some(victim) = single {
                vec![victim]
            } else {
                self.entries
                    .iter()
                    .filter(|(k, _)| *k != &key && (!pin_aware || !self.protected.contains(*k)))
                    .map(|(k, v)| (!v.expired_at(now), v.last_used, k.clone(), v.weight))
                    .collect()
            }
        } else {
            Vec::new()
        };
        if pin_aware && (required > 0 || slots > 0) {
            let bytes = victims.iter().fold(0_u64, |sum, v| sum.saturating_add(v.3));
            if bytes < required || victims.len() < slots {
                return false;
            }
        }
        if let Some((index, first)) = victims.iter().enumerate().min_by_key(|(_, v)| (v.0, v.1)) {
            if slots <= 1 && first.3 >= required {
                let victim = victims.swap_remove(index);
                victims.clear();
                victims.push(victim);
            } else {
                victims.sort_unstable_by_key(|v| (v.0, v.1));
            }
        }
        let mut victims = victims.into_iter();
        self.next_clock();
        if let Some(previous) = self.entries.remove(&key) {
            self.evictable.remove(&previous.rank);
            self.resident -= previous.weight;
            self.deadline_entries -= usize::from(previous.deadline.is_some());
        }
        while self.resident > self.capacity - weight || self.needs_slot() {
            // Preflight guarantees a victim exists for every required byte/slot.
            let victim = victims
                .next()
                .expect("admission preflight missed a victim")
                .2;
            if let Some(value) = self.remove(&victim) {
                evicted.push((victim, value));
            }
        }
        self.resident += weight;
        self.deadline_entries += usize::from(deadline.is_some());
        if !self.protected.contains(&key) {
            if let Some(deadline) = deadline {
                self.note_deadline(deadline);
            }
            self.evictable.insert(self.clock, key.clone());
        }
        self.entries.insert(
            key,
            Entry {
                value,
                weight,
                last_used: self.clock,
                rank: self.clock,
                queued: false,
                deadline,
            },
        );
        true
    }
    /// Drops every entry and pin, keeping the limits. Returns the values so a caller can
    /// release them outside a lock.
    pub fn clear(&mut self) -> Vec<(K, V)> {
        let cleared = std::mem::replace(self, Self::with_limits(self.limits));
        cleared
            .entries
            .into_iter()
            .map(|(key, entry)| (key, entry.value))
            .collect()
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let entry = self.entries.remove(key)?;
        self.evictable.remove(&entry.rank);
        self.resident -= entry.weight;
        self.deadline_entries -= usize::from(entry.deadline.is_some());
        self.protected.remove(key);
        Some(entry.value)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod limit_tests;

#[cfg(test)]
mod expiry_plan_tests;
