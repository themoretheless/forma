//! Lookup policy timing with live consumers; excludes pixels, disk, GPU and UI.
use memory_budget::{CacheLimits, LeaseCache};
use std::{hint::black_box, time::Instant};
fn percentile(values: &mut [f64], percentile: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    values[(values.len() * percentile).div_ceil(100) - 1]
}
fn main() {
    println!("held_entries,operations_per_sample,samples,p50_ns,p95_ns");
    for count in [1usize, 64, 1024, 16384] {
        let mut cache = LeaseCache::new(CacheLimits::bytes(count as u64));
        for key in 0..count {
            cache.insert(key, key as u64, 1).unwrap();
        }
        let leases: Vec<_> = (0..count).map(|key| cache.get(&key).unwrap()).collect();
        let mut samples = Vec::new();
        for sample in 0..23 {
            let start = Instant::now();
            let mut checksum = 0u64;
            for i in 0..10000 {
                checksum += black_box(cache.get_cloned(&black_box(i % count)).unwrap());
            }
            let elapsed = start.elapsed().as_secs_f64() * 1e9 / 10000.;
            let expected: u64 = (0..10000).map(|i| (i % count) as u64).sum();
            assert_eq!(checksum, expected);
            assert_eq!(cache.len(), count);
            if sample >= 3 {
                samples.push(elapsed);
            }
        }
        let p50 = percentile(&mut samples, 50);
        let p95 = percentile(&mut samples, 95);
        println!("{count},10000,20,{p50:.3},{p95:.3}");
        assert!(cache.set_limits(CacheLimits::bytes(0)).is_none());
        drop(leases);
        let victims = cache.set_limits(CacheLimits::bytes(0)).unwrap();
        assert_eq!(victims.len(), count);
        assert!(cache.is_empty());
    }
}
