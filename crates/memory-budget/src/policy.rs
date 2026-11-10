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
    deadline: Option<Instant>,
}
impl<V> Entry<V> {
    fn expired(&self, now: Instant) -> bool {
        self.deadline.is_some_and(|v| now >= v)
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
    limits: CacheLimits,
    // Track live deadlines so removing the last TTL entry restores indexed eviction.
    deadline_entries: usize,
    // Conservative earliest unprotected deadline; stale earlier hints only cause one scan.
    next_expiry: Option<Instant>,
}
impl<K: Clone + Eq + Hash, V> WeightedLru<K, V> {
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
        if self.resident <= limits.max_bytes
            && limits
                .max_entries
                .is_none_or(|max| self.entries.len() <= max)
        {
            self.capacity = limits.max_bytes;
            self.limits = limits;
            return Some(Vec::new());
        }
        let now = Instant::now();
        let mut candidates: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| !self.protected.contains(*key))
            .map(|(key, entry)| (!entry.expired(now), entry.last_used, key.clone()))
            .collect();
        candidates.sort_unstable_by_key(|entry| (entry.0, entry.1));
        let mut victims = Vec::new();
        for (_, _, key) in candidates {
            if self.resident <= limits.max_bytes
                && limits
                    .max_entries
                    .is_none_or(|max| self.entries.len() <= max)
            {
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
            self.evictable.remove(&self.entries[key].last_used);
            self.protected.insert(key.clone());
            true
        } else {
            false
        }
    }
    pub fn unpin(&mut self, key: &K) {
        if self.protected.remove(key)
            && let Some(entry) = self.entries.get(key)
        {
            self.evictable.insert(entry.last_used, key.clone());
            if let Some(deadline) = entry.deadline {
                self.next_expiry = Some(self.next_expiry.map_or(deadline, |old| old.min(deadline)));
            }
        }
    }
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if self
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
            for (index, (_, key)) in ordered.into_iter().enumerate() {
                let rank = index as u64 + 1;
                self.entries.get_mut(&key).unwrap().last_used = rank;
                if !self.protected.contains(&key) {
                    self.evictable.insert(rank, key);
                }
            }
            self.clock = self.entries.len() as u64;
        }
        self.clock += 1;
    }
    fn get_live(&mut self, key: &K) -> Option<&V> {
        // Repeated access to the most recent entry changes no eviction order.
        if self
            .entries
            .get(key)
            .is_some_and(|entry| entry.last_used == self.clock)
        {
            return self.entries.get(key).map(|entry| &entry.value);
        }
        self.next_clock();
        let entry = self.entries.get_mut(key)?;
        if !self.protected.contains(key) {
            self.evictable.remove(&entry.last_used);
            self.evictable.insert(self.clock, key.clone());
        }
        entry.last_used = self.clock;
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
            self.next_expiry = Some(self.next_expiry.map_or(cutoff, |next| next.min(cutoff)));
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
    fn deadline(&self, now: Instant) -> Result<Option<Instant>, ()> {
        match self.limits.ttl {
            Some(ttl) => now.checked_add(ttl).map(Some).ok_or(()),
            None => Ok(None),
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
        self.insert_at(key, value, weight, Instant::now(), false)
    }
    /// Rejected pin-aware admission preserves every existing value and recency.
    pub fn insert_prefetch(&mut self, key: K, value: V, weight: u64) -> bool {
        self.insert_at(key, value, weight, Instant::now(), true)
    }
    pub fn insert_prefetch_with_evictions(
        &mut self,
        key: K,
        value: V,
        weight: u64,
    ) -> (bool, Vec<(K, V)>) {
        let mut evicted = Vec::new();
        let admitted =
            self.insert_at_collect(key, value, weight, Instant::now(), true, &mut evicted);
        (admitted, evicted)
    }
    pub fn take_lru(&mut self) -> Option<(K, V)> {
        let now = Instant::now();
        if self.deadline_entries == 0 {
            let key = self.evictable.first_key_value()?.1.clone();
            return self.remove(&key).map(|value| (key, value));
        }
        let key = self
            .entries
            .iter()
            .filter(|(k, _)| !self.protected.contains(*k))
            .min_by_key(|(_, value)| (!value.expired(now), value.last_used))
            .map(|(k, _)| k.clone())?;
        self.remove(&key).map(|value| (key, value))
    }
    fn insert_at(&mut self, key: K, value: V, weight: u64, now: Instant, pin_aware: bool) -> bool {
        self.insert_at_collect(key, value, weight, now, pin_aware, &mut Vec::new())
    }
    fn insert_at_collect(
        &mut self,
        key: K,
        value: V,
        weight: u64,
        now: Instant,
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
                    .map(|(k, v)| (!v.expired(now), v.last_used, k.clone(), v.weight))
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
            self.evictable.remove(&previous.last_used);
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
                self.next_expiry = Some(self.next_expiry.map_or(deadline, |old| old.min(deadline)));
            }
            self.evictable.insert(self.clock, key.clone());
        }
        self.entries.insert(
            key,
            Entry {
                value,
                weight,
                last_used: self.clock,
                deadline,
            },
        );
        true
    }
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let entry = self.entries.remove(key)?;
        self.evictable.remove(&entry.last_used);
        self.resident -= entry.weight;
        self.deadline_entries -= usize::from(entry.deadline.is_some());
        self.protected.remove(key);
        Some(entry.value)
    }
}

#[cfg(test)]
mod tests {
    use super::WeightedLru;
    #[test]
    fn limit_update_matrix_is_atomic_preserves_pins_and_fixed_deadlines() {
        use super::CacheLimits;
        use std::time::Duration;
        let snapshot_entries = |cache: &WeightedLru<u8, u8>| {
            cache
                .entries
                .iter()
                .map(|(&k, v)| (k, v.value, v.weight, v.last_used, v.deadline))
                .collect::<std::collections::BTreeSet<_>>()
        };
        let mut cases = 0;
        for mask in 0u8..8 {
            for old_ttl in [None, Some(Duration::ZERO)] {
                for bytes in [0, 1, 3, 4, 7, 8, 11, 12, 13] {
                    for count in [None, Some(0), Some(1), Some(2), Some(3), Some(4)] {
                        for ttl in [None, Some(Duration::ZERO), Some(Duration::MAX)] {
                            let mut cache = WeightedLru::with_limits(CacheLimits {
                                max_bytes: 12,
                                max_entries: Some(3),
                                ttl: old_ttl,
                            });
                            for key in 0u8..3 {
                                assert!(cache.insert(key, key, 4));
                            }
                            for key in 0u8..3 {
                                if mask & (1 << key) != 0 {
                                    assert!(cache.pin(&key));
                                }
                            }
                            let previous = snapshot_entries(&cache);
                            let clock = cache.clock;
                            let pins = mask.count_ones() as usize;
                            let accepted =
                                pins as u64 * 4 <= bytes && count.is_none_or(|limit| pins <= limit);
                            let result = cache.set_limits(CacheLimits {
                                max_bytes: bytes,
                                max_entries: count,
                                ttl,
                            });
                            assert_eq!(result.is_some(), accepted);
                            if let Some(victims) = result {
                                assert_eq!(cache.capacity(), bytes);
                                assert_eq!(cache.limits().max_entries, count);
                                assert_eq!(cache.limits().ttl, ttl);
                                let mut survivors = vec![0u8, 1, 2];
                                let mut expected = Vec::new();
                                while survivors.len() as u64 * 4 > bytes
                                    || count.is_some_and(|limit| survivors.len() > limit)
                                {
                                    let index = survivors
                                        .iter()
                                        .position(|key| mask & (1 << key) == 0)
                                        .unwrap();
                                    expected.push(survivors.remove(index));
                                }
                                assert_eq!(
                                    victims,
                                    expected.iter().map(|&key| (key, key)).collect::<Vec<_>>()
                                );
                                assert_eq!(cache.len(), survivors.len());
                                assert_eq!(cache.resident_weight(), survivors.len() as u64 * 4);
                                for key in survivors {
                                    let entry = &cache.entries[&key];
                                    let old = previous.iter().find(|v| v.0 == key).unwrap();
                                    assert_eq!((entry.last_used, entry.deadline), (old.3, old.4));
                                    assert_eq!(
                                        cache.protected.contains(&key),
                                        mask & (1 << key) != 0
                                    );
                                }
                                if ttl == Some(Duration::MAX) {
                                    let snapshot = cache
                                        .entries
                                        .keys()
                                        .copied()
                                        .collect::<std::collections::BTreeSet<_>>();
                                    assert!(!cache.insert_prefetch(99, 99, 1));
                                    assert_eq!(
                                        cache
                                            .entries
                                            .keys()
                                            .copied()
                                            .collect::<std::collections::BTreeSet<_>>(),
                                        snapshot
                                    );
                                }
                            } else {
                                assert_eq!(cache.capacity(), 12);
                                assert_eq!(cache.limits().max_entries, Some(3));
                                assert_eq!(cache.limits().ttl, old_ttl);
                                assert_eq!(cache.protected.len(), pins);
                                for key in 0u8..3 {
                                    assert_eq!(
                                        cache.protected.contains(&key),
                                        mask & (1 << key) != 0
                                    );
                                }
                                assert_eq!(snapshot_entries(&cache), previous);
                            }
                            assert_eq!(cache.clock, clock);
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 2592);
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Keep the reference state machine and generated operations together"
    )]
    fn deterministic_policy_model_covers_zero_limits_expiry_pins_and_replacement() {
        use super::CacheLimits;
        use std::time::{Duration, Instant};
        #[derive(Clone, Copy)]
        struct Item {
            value: u64,
            weight: u64,
            rank: u64,
            expires: Option<u64>,
            pin: bool,
        }
        let base = Instant::now();
        let mut operations = 0;
        for capacity in [0, 1, 8, 20] {
            for count in [None, Some(0), Some(1), Some(3)] {
                for ttl in [None, Some(0), Some(5)] {
                    let mut cache = WeightedLru::with_limits(CacheLimits {
                        max_bytes: capacity,
                        max_entries: count,
                        ttl: ttl.map(Duration::from_millis),
                    });
                    let mut model: [Option<Item>; 16] = [None; 16];
                    let mut rank = 0;
                    let mut seed = 0x1234_5678_9abc_def0u64;
                    let mut time = 0u64;
                    for step in 0..2000u64 {
                        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                        let key = ((seed >> 8) % 16) as usize;
                        time += (seed >> 32) % 3;
                        let now = base + Duration::from_millis(time);
                        match seed % 7 {
                            0 | 1 => {
                                let weight = (seed >> 16) % 12;
                                let mut trial = model;
                                let pin = trial[key].is_some_and(|v| v.pin);
                                trial[key] = None;
                                let mut candidates: Vec<_> = (0..16)
                                    .filter(|&i| trial[i].is_some_and(|v| !v.pin))
                                    .collect();
                                candidates.sort_unstable_by_key(|&i| {
                                    let v = trial[i].unwrap();
                                    (v.expires.is_none_or(|deadline| time < deadline), v.rank)
                                });
                                let mut admitted = weight <= capacity && count != Some(0);
                                while admitted
                                    && (trial.iter().flatten().map(|v| v.weight).sum::<u64>()
                                        + weight
                                        > capacity
                                        || count.is_some_and(|limit| {
                                            trial.iter().flatten().count() + 1 > limit
                                        }))
                                {
                                    if candidates.is_empty() {
                                        admitted = false;
                                    } else {
                                        trial[candidates.remove(0)] = None;
                                    }
                                }
                                assert_eq!(cache.insert_at(key, step, weight, now, true), admitted);
                                if admitted {
                                    rank += 1;
                                    trial[key] = Some(Item {
                                        value: step,
                                        weight,
                                        rank,
                                        expires: ttl.map(|v| time + v),
                                        pin,
                                    });
                                    model = trial;
                                }
                            }
                            2 => {
                                let expected = if let Some(mut item) = model[key] {
                                    if item.expires.is_some_and(|deadline| time >= deadline) {
                                        if !item.pin {
                                            model[key] = None;
                                        }
                                        None
                                    } else {
                                        rank += 1;
                                        item.rank = rank;
                                        model[key] = Some(item);
                                        Some(item.value)
                                    }
                                } else {
                                    None
                                };
                                assert_eq!(cache.get_at(&key, now).copied(), expected);
                            }
                            3 => {
                                assert_eq!(cache.pin(&key), model[key].is_some());
                                if let Some(item) = &mut model[key] {
                                    item.pin = true;
                                }
                            }
                            4 => {
                                cache.unpin(&key);
                                if let Some(item) = &mut model[key] {
                                    item.pin = false;
                                }
                            }
                            5 => {
                                let mut removed = 0;
                                for item in &mut model {
                                    if item.is_some_and(|v| {
                                        !v.pin && v.expires.is_some_and(|deadline| time >= deadline)
                                    }) {
                                        *item = None;
                                        removed += 1;
                                    }
                                }
                                assert_eq!(cache.prune_expired_at(now), removed);
                            }
                            _ => assert_eq!(cache.remove(&key), model[key].take().map(|v| v.value)),
                        }
                        assert_eq!(cache.len(), model.iter().flatten().count());
                        assert_eq!(
                            cache.resident_weight(),
                            model.iter().flatten().map(|v| v.weight).sum::<u64>()
                        );
                        for (key, expected) in model.iter().enumerate() {
                            assert_eq!(
                                cache.entries.get(&key).map(|v| (v.value, v.weight)),
                                expected.map(|v| (v.value, v.weight))
                            );
                            assert_eq!(
                                cache.protected.contains(&key),
                                expected.is_some_and(|v| v.pin)
                            );
                        }
                        operations += 1;
                    }
                }
            }
        }
        assert_eq!(operations, 96_000);
    }

    #[test]
    fn next_expiry_skips_future_and_protected_entries_then_tracks_unpin() {
        let now = std::time::Instant::now();
        let mut cache = WeightedLru::with_limits(super::CacheLimits {
            max_bytes: 4,
            max_entries: None,
            ttl: Some(std::time::Duration::from_secs(10)),
        });
        assert!(cache.insert_at(1, 11, 1, now, true));
        assert!(cache.insert_at(2, 22, 1, now + std::time::Duration::from_secs(5), true));
        cache.pin(&1);
        assert!(
            cache
                .drain_expired_at(now + std::time::Duration::from_secs(9))
                .is_empty()
        );
        assert!(
            cache
                .drain_expired_at(now + std::time::Duration::from_secs(10))
                .is_empty()
        );
        assert_eq!(
            cache.next_expiry,
            Some(now + std::time::Duration::from_secs(15))
        );
        cache.unpin(&1);
        assert_eq!(
            cache.drain_expired_at(now + std::time::Duration::from_secs(10)),
            vec![(1, 11)]
        );
        cache.pin(&2);
        assert!(
            cache
                .drain_expired_at(now + std::time::Duration::from_secs(15))
                .is_empty()
        );
        assert_eq!(cache.next_expiry, None);
        cache.unpin(&2);
        assert_eq!(
            cache.drain_expired_at(now + std::time::Duration::from_secs(15)),
            vec![(2, 22)]
        );
        assert_eq!(cache.deadline_entries, 0);
    }
    #[test]
    fn deadline_tracking_survives_replacement_pins_and_expiry() {
        let now = std::time::Instant::now();
        let mut cache = WeightedLru::with_limits(super::CacheLimits {
            max_bytes: 3,
            max_entries: Some(3),
            ttl: Some(std::time::Duration::ZERO),
        });
        assert!(cache.insert_at(1, 1, 1, now, true));
        assert!(cache.insert_at(2, 2, 1, now, true));
        assert_eq!(cache.deadline_entries, 2);
        cache.pin(&1);
        assert_eq!(cache.prune_expired_at(now), 1);
        assert_eq!(cache.deadline_entries, 1);
        cache.set_limits(super::CacheLimits::bytes(3)).unwrap();
        // Replacement removes the old deadline while retaining the owner pin.
        assert!(cache.insert_at(1, 9, 1, now, true));
        assert_eq!(cache.deadline_entries, 0);
        assert!(cache.protected.contains(&1));
        assert_eq!(cache.prune_expired_at(now), 0);
        assert_eq!(cache.get_at(&1, now), Some(&9));
        cache.unpin(&1);
        assert_eq!(cache.take_lru(), Some((1, 9)));
        assert_eq!(cache.deadline_entries, 0);
        cache
            .set_limits(super::CacheLimits {
                max_bytes: 3,
                max_entries: None,
                ttl: Some(std::time::Duration::ZERO),
            })
            .unwrap();
        assert!(cache.insert_at(3, 3, 1, now, true));
        assert_eq!(cache.remove(&3), Some(3));
        assert_eq!(cache.deadline_entries, 0);
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Keep the independent state model beside its operation checks.
    fn indexed_policy_matches_sequence_model_under_mixed_pressure() {
        #[derive(Clone, Debug)]
        struct Item {
            key: u32,
            value: u32,
            weight: u64,
            pinned: bool,
        }
        let mut cache = WeightedLru::with_limits(super::CacheLimits {
            max_bytes: 16,
            max_entries: Some(4),
            ttl: None,
        });
        // The oracle stores oldest-to-newest entries in a flat sequence.
        let mut model: Vec<Item> = Vec::new();
        let mut seed = 0x71a3_99c5_u64;
        for step in 0..10000_u32 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let key = ((seed >> 32) % 12) as u32;
            let operation = (seed >> 48) % 6;
            let index = model.iter().position(|item| item.key == key);
            match operation {
                0 => {
                    let weight = (seed >> 8) % 20;
                    let pinned = index.is_some_and(|i| model[i].pinned);
                    let mut proposal = model.clone();
                    if let Some(i) = index {
                        proposal.remove(i);
                    }
                    let mut victims = Vec::new();
                    let mut admitted = weight <= 16;
                    while admitted
                        && (proposal.iter().map(|v| v.weight).sum::<u64>() + weight > 16
                            || proposal.len() >= 4)
                    {
                        if let Some(i) = proposal.iter().position(|v| !v.pinned) {
                            let item = proposal.remove(i);
                            victims.push((item.key, item.value));
                        } else {
                            admitted = false;
                        }
                    }
                    let actual = cache.insert_prefetch_with_evictions(key, step, weight);
                    assert_eq!(actual.0, admitted, "admission at step {step}");
                    if admitted {
                        assert_eq!(actual.1, victims, "victims at step {step}");
                        proposal.push(Item {
                            key,
                            value: step,
                            weight,
                            pinned,
                        });
                        model = proposal;
                    } else {
                        assert!(actual.1.is_empty());
                    }
                }
                1 => {
                    let expected = index.map(|i| {
                        let item = model.remove(i);
                        let value = item.value;
                        model.push(item);
                        value
                    });
                    assert_eq!(cache.get(&key).copied(), expected);
                }
                2 => {
                    if let Some(i) = index {
                        model[i].pinned = true;
                    }
                    assert_eq!(cache.pin(&key), index.is_some());
                }
                3 => {
                    if let Some(i) = index {
                        model[i].pinned = false;
                    }
                    cache.unpin(&key);
                }
                4 => {
                    let expected = index.map(|i| model.remove(i).value);
                    assert_eq!(cache.remove(&key), expected);
                }
                _ => {
                    let expected = model.iter().position(|v| !v.pinned).map(|i| {
                        let item = model.remove(i);
                        (item.key, item.value)
                    });
                    assert_eq!(cache.take_lru(), expected);
                }
            }
            assert_eq!(cache.len(), model.len());
            assert_eq!(
                cache.resident_weight(),
                model.iter().map(|v| v.weight).sum::<u64>()
            );
            for item in &model {
                assert_eq!(cache.entries[&item.key].value, item.value);
                assert_eq!(cache.protected.contains(&item.key), item.pinned);
            }
            assert_eq!(
                cache.evictable.len(),
                model.iter().filter(|v| !v.pinned).count()
            );
            // Exercise index compaction without changing the sequence oracle.
            if step % 997 == 0 {
                cache.clock = u64::MAX;
            }
        }
    }

    #[test]
    fn eviction_index_tracks_pin_unpin_hits_and_clock_rollover() {
        let mut cache = WeightedLru::new(3);
        for key in ["a", "b", "c"] {
            assert!(cache.insert(key, key, 1));
        }
        cache.get(&"a");
        cache.pin(&"b");
        cache.get(&"b");
        cache.unpin(&"b");
        cache.clock = u64::MAX;
        cache.get(&"c");
        assert!(cache.insert_prefetch("d", "d", 1));
        assert!(cache.get(&"a").is_none());
        assert_eq!(cache.take_lru(), Some(("b", "b")));
        assert_eq!(cache.take_lru(), Some(("c", "c")));
        assert_eq!(cache.take_lru(), Some(("d", "d")));
        assert!(cache.evictable.is_empty());
        assert_eq!(cache.resident_weight(), 0);
    }

    #[test]
    fn bulk_admission_preserves_visible_and_orders_thousands_of_victims() {
        let mut cache = WeightedLru::new(4096);
        for key in 0_u32..4096 {
            assert!(cache.insert(key, key, 1));
        }
        cache.pin(&4095);
        assert_eq!(cache.get(&0), Some(&0));
        let (admitted, victims) = cache.insert_prefetch_with_evictions(5000, 5000, 4095);
        assert!(admitted);
        let expected: Vec<_> = (1_u32..4095)
            .chain(std::iter::once(0))
            .map(|k| (k, k))
            .collect();
        assert_eq!(victims, expected);
        assert_eq!(cache.get(&4095), Some(&4095));
        assert_eq!(cache.get(&5000), Some(&5000));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.resident_weight(), 4096);
    }

    #[test]
    fn evicts_by_bytes_and_recency() {
        let mut cache = WeightedLru::new(10);
        assert!(cache.insert("a", 1, 4));
        assert!(cache.insert("b", 2, 4));
        assert_eq!(cache.get(&"a"), Some(&1));
        assert!(cache.insert("c", 3, 4));
        assert!(cache.get(&"b").is_none());
        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.get(&"c"), Some(&3));
        assert_eq!(cache.resident_weight(), 8);
    }

    #[test]
    fn rejects_an_entry_larger_than_budget() {
        let mut cache = WeightedLru::new(3);
        assert!(!cache.insert("large", 1, 4));
        assert!(cache.is_empty());
    }

    #[test]
    fn prefetch_does_not_evict_pinned_visible_frame() {
        let mut cache = WeightedLru::new(10);
        assert!(cache.insert("current", 1, 6));
        assert!(cache.pin(&"current"));
        assert!(cache.insert_prefetch("next", 2, 4));
        assert!(cache.insert_prefetch("far", 3, 4));
        assert_eq!(cache.get(&"current"), Some(&1));
        assert!(cache.get(&"next").is_none());
    }

    #[test]
    fn rejected_replacement_preserves_old_value_other_entries_and_recency() {
        let mut cache = WeightedLru::new(10);
        assert!(cache.insert("visible", 1, 6));
        assert!(cache.pin(&"visible"));
        assert!(cache.insert_prefetch("older", 2, 2));
        assert!(cache.insert_prefetch("replace", 3, 2));
        let clock = cache.clock;
        assert!(!cache.insert_prefetch("replace", 99, 5));
        assert_eq!(cache.clock, clock);
        assert_eq!(cache.resident_weight(), 10);
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get(&"older"), Some(&2));
        assert_eq!(cache.get(&"replace"), Some(&3));
        assert_eq!(cache.get(&"visible"), Some(&1));
        assert!(!cache.insert_prefetch("new", 4, 5));
        assert_eq!(cache.resident_weight(), 10);
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn extreme_byte_weights_cannot_overflow_admission() {
        for protected in [false, true] {
            let mut cache = WeightedLru::new(u64::MAX);
            assert!(cache.insert("full", 1, u64::MAX));
            if protected {
                assert!(cache.pin(&"full"));
            }
            assert_eq!(cache.insert_prefetch("one", 2, 1), !protected);
            assert_eq!(
                cache.resident_weight(),
                if protected { u64::MAX } else { 1 }
            );
            assert_eq!(cache.len(), 1);
        }
        let mut cache = WeightedLru::new(u64::MAX);
        assert!(cache.insert("full", 1, u64::MAX));
        assert!(cache.insert("one", 2, 1));
        assert_eq!(cache.resident_weight(), 1);
        assert!(cache.get(&"full").is_none());
    }
}

#[cfg(test)]
mod limit_tests {
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
        assert!(!cache.insert_at_collect("new", 2, 2, now, true, &mut victims));
        assert!(!cache.insert_at_collect("old", 3, 2, now, true, &mut victims));
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
}

#[cfg(test)]
mod expiry_plan_tests {
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
}
