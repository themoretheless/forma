//! Policy-only timings; excludes decode, allocation payload, disk, GPU and display.
use memory_budget::WeightedLru;
use std::{hint::black_box, time::Instant};
fn percentile(mut samples: Vec<f64>, percent: usize) -> f64 {
    assert!(!samples.is_empty() && (1..=100).contains(&percent));
    samples.sort_by(f64::total_cmp);
    let rank = samples
        .len()
        .checked_mul(percent)
        .expect("percentile rank overflow")
        .div_ceil(100);
    samples[rank - 1]
}
fn main() {
    println!("entries,operation,repetitions,p50_us,p95_us");
    for size in [64_u32, 1024, 4096, 16384] {
        for operation in [
            "hit",
            "ttl_maintenance_hit",
            "rotating_hit",
            "single_eviction",
            "ttl_history_single_eviction",
            "bulk_eviction",
        ] {
            let mut samples = Vec::new();
            for repetition in 0..23 {
                let mut cache = WeightedLru::new(u64::from(size));
                if operation == "ttl_maintenance_hit" {
                    let mut limits = memory_budget::CacheLimits::bytes(u64::from(size));
                    limits.ttl = Some(std::time::Duration::from_secs(3600));
                    cache.set_limits(limits).unwrap();
                }
                for key in 0..size {
                    assert!(cache.insert(key, key, 1));
                }
                if operation == "ttl_history_single_eviction" {
                    let mut limits = memory_budget::CacheLimits::bytes(u64::from(size));
                    limits.ttl = Some(std::time::Duration::ZERO);
                    cache.set_limits(limits).unwrap();
                    assert!(cache.insert(size, size, 0));
                    assert_eq!(cache.prune_expired(), 1);
                    cache
                        .set_limits(memory_budget::CacheLimits::bytes(u64::from(size)))
                        .unwrap();
                }
                assert!(cache.pin(&(size - 1)));
                assert_eq!(cache.get(&0), Some(&0));
                let started = Instant::now();
                let (admitted, victims, elapsed) = match operation {
                    "ttl_maintenance_hit" => {
                        for _ in 0..100 {
                            assert!(cache.drain_expired().is_empty());
                            black_box(cache.get(black_box(&0)));
                        }
                        (
                            true,
                            Vec::new(),
                            started.elapsed().as_secs_f64() * 1e6 / 100.0,
                        )
                    }
                    "hit" => {
                        for _ in 0..10000 {
                            black_box(cache.get(black_box(&0)));
                        }
                        (
                            true,
                            Vec::new(),
                            started.elapsed().as_secs_f64() * 1e6 / 10000.0,
                        )
                    }
                    "rotating_hit" => {
                        for i in 0..10000_u32 {
                            black_box(cache.get(black_box(&(i % size))));
                        }
                        (
                            true,
                            Vec::new(),
                            started.elapsed().as_secs_f64() * 1e6 / 10000.0,
                        )
                    }
                    "single_eviction" | "ttl_history_single_eviction" => {
                        let (ok, victims) = cache.insert_prefetch_with_evictions(size, size, 1);
                        (ok, victims, started.elapsed().as_secs_f64() * 1e6)
                    }
                    _ => {
                        let (ok, victims) =
                            cache.insert_prefetch_with_evictions(size, size, u64::from(size - 1));
                        (ok, victims, started.elapsed().as_secs_f64() * 1e6)
                    }
                };
                assert!(admitted);
                assert_eq!(cache.get(&(size - 1)), Some(&(size - 1)));
                match operation {
                    "hit" | "rotating_hit" | "ttl_maintenance_hit" => {
                        assert!(victims.is_empty());
                        assert_eq!(cache.len(), size as usize);
                    }
                    "single_eviction" | "ttl_history_single_eviction" => {
                        assert_eq!(victims, [(1, 1)]);
                        assert_eq!(cache.len(), size as usize);
                    }
                    _ => {
                        let expected: Vec<_> = (1..size - 1)
                            .chain(std::iter::once(0))
                            .map(|key| (key, key))
                            .collect();
                        assert_eq!(victims, expected);
                        assert_eq!(cache.len(), 2);
                    }
                }
                assert_eq!(cache.resident_weight(), u64::from(size));
                if repetition >= 3 {
                    samples.push(elapsed);
                }
            }
            println!(
                "{size},{operation},{},{:.3},{:.3}",
                samples.len(),
                percentile(samples.clone(), 50),
                percentile(samples, 95)
            );
        }
    }
}
