use super::WeightedLru;
#[test]
fn clear_drops_entries_and_pins_but_keeps_limits() {
    let limits = super::CacheLimits {
        max_bytes: 8,
        max_entries: Some(2),
        ttl: None,
    };
    let mut cache = WeightedLru::with_limits(limits);
    assert!(cache.insert(1, 10, 3));
    assert!(cache.insert(2, 20, 3));
    assert!(cache.pin(&1));
    let mut cleared = cache.clear();
    cleared.sort_unstable();
    assert_eq!(cleared, [(1, 10), (2, 20)]);
    assert!(cache.is_empty());
    assert_eq!(cache.resident_weight(), 0);
    assert_eq!(cache.limits().max_entries, Some(2));
    assert!(cache.insert(3, 30, 8));
    assert_eq!(cache.take_lru(), Some((3, 30)));
}
#[test]
fn lazy_recency_matches_an_exact_lru_model() {
    // Reference: keys least recently used first, plus the pinned set.
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % bound
    };
    for capacity in [1_u64, 2, 3, 8] {
        let mut cache = WeightedLru::new(capacity);
        let mut order: Vec<u64> = Vec::new();
        let mut pinned = std::collections::HashSet::new();
        let touch = |order: &mut Vec<u64>, key: u64| {
            order.retain(|&k| k != key);
            order.push(key);
        };
        for step in 0..20_000 {
            let key = next(12);
            match next(6) {
                0 | 1 => {
                    let hit = cache.get(&key).copied();
                    assert_eq!(hit.is_some(), order.contains(&key), "step {step}");
                    if hit.is_some() {
                        touch(&mut order, key);
                    }
                }
                2 => {
                    let (admitted, mut evicted) =
                        cache.insert_prefetch_with_evictions(key, key * 10, 1);
                    let expected = if order.contains(&key) || (order.len() as u64) < capacity {
                        Some(None)
                    } else {
                        order
                            .iter()
                            .find(|k| !pinned.contains(*k))
                            .map(|&victim| Some(victim))
                    };
                    assert_eq!(admitted, expected.is_some(), "step {step}");
                    if let Some(victim) = expected.flatten() {
                        assert_eq!(evicted.pop(), Some((victim, victim * 10)), "step {step}");
                        order.retain(|&k| k != victim);
                    }
                    assert!(evicted.is_empty(), "step {step}");
                    if admitted {
                        touch(&mut order, key);
                    }
                }
                3 => {
                    assert_eq!(cache.pin(&key), order.contains(&key), "step {step}");
                    if order.contains(&key) {
                        pinned.insert(key);
                    }
                }
                4 => {
                    cache.unpin(&key);
                    pinned.remove(&key);
                }
                _ => {
                    if next(2) == 0 {
                        let expected = order.iter().copied().find(|k| !pinned.contains(k));
                        assert_eq!(
                            cache.take_lru(),
                            expected.map(|k| (k, k * 10)),
                            "step {step}"
                        );
                        if let Some(k) = expected {
                            order.retain(|&o| o != k);
                        }
                    } else {
                        assert_eq!(cache.remove(&key).is_some(), order.contains(&key));
                        order.retain(|&k| k != key);
                        pinned.remove(&key);
                    }
                }
            }
            assert_eq!(cache.len(), order.len(), "step {step}");
        }
    }
}
#[test]
fn peek_reads_without_touching_recency() {
    let mut cache = WeightedLru::new(2);
    assert!(cache.insert(1, 10, 1));
    assert!(cache.insert(2, 20, 1));
    assert_eq!(cache.peek(&1), Some(&10));
    assert_eq!(cache.peek(&3), None);
    // 1 stays least recently used despite the peek.
    assert_eq!(cache.take_lru(), Some((1, 10)));
}
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
                                assert_eq!(cache.protected.contains(&key), mask & (1 << key) != 0);
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
                                assert_eq!(cache.protected.contains(&key), mask & (1 << key) != 0);
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
                                && (trial.iter().flatten().map(|v| v.weight).sum::<u64>() + weight
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
