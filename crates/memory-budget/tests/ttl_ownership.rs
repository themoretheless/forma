use memory_budget::{CacheLimits, LeaseCache, MemoryBudget};
use std::time::Duration;

#[test]
fn real_ttl_refuses_new_consumers_but_retains_leases_and_external_allocation_owners() {
    let budget = MemoryBudget::new(16);
    let mut cache = LeaseCache::new(CacheLimits {
        max_bytes: 16,
        max_entries: Some(1),
        ttl: Some(Duration::from_secs(1)),
    });
    let pixels = budget.try_buffer(16, 73u8).unwrap().freeze();
    let external = pixels.clone();
    cache.insert("visible", pixels, 16).unwrap();
    let lease = cache.get(&"visible").unwrap();
    let render_owner = lease.clone();
    std::thread::sleep(Duration::from_millis(1100));
    assert!(cache.get(&"visible").is_none());
    assert_eq!(cache.prune_expired(), 0);
    assert!(cache.set_limits(CacheLimits::bytes(0)).is_none());
    assert_eq!(&**render_owner, &[73; 16]);
    assert_eq!(budget.used(), 16);
    assert!(budget.try_buffer(1, 0u8).is_err());
    drop(lease);
    assert_eq!(cache.prune_expired(), 0);
    drop(render_owner);
    assert_eq!(cache.prune_expired(), 1);
    assert!(cache.is_empty());
    assert_eq!(budget.used(), 16);
    assert!(budget.try_buffer(16, 0u8).is_err());
    drop(external);
    assert_eq!(budget.used(), 0);
    let admitted = budget.try_buffer(16, 91u8).unwrap().freeze();
    assert_eq!(&*admitted, &[91; 16]);
    drop(admitted);
    assert_eq!(budget.used(), 0);
}
