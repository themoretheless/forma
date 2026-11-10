//! Format-independent memory ownership and byte-weighted cache policies.
//! Limits account for managed buffer capacity, excluding allocator bookkeeping.
//!
//! # Composition contract
//! `MemoryBudget` governs live allocation ownership, whereas `CacheLimits`
//! governs cache membership. A removed entry may therefore stop contributing to
//! cache weight while its buffer still consumes memory through another owner.
//! Child budgets share their ancestors' caps; they are local ceilings, not
//! guaranteed reservations for foreground work. A coordinator must preserve
//! foreground headroom when admitting background allocations.
//!
//! `CacheLimits::ttl` is fixed from insertion, not renewed by a hit. Expiration
//! prevents a new lookup from acquiring the entry; an existing `CacheLease`
//! remains valid. Eviction returns owned values so a caller can transfer them to
//! swap without copying. Their allocations remain charged until ownership ends.
//!
//! This crate does not schedule decoding, perform disk I/O, or own GPU resources.
//! A GPU integration must retain its allocation reservations until submitted
//! work finishes, independently of whether the CPU cache still contains a key.
//!
//! A consumer lease protects cache membership while the shared allocation keeps
//! its byte reservation. Dropping the last consumer makes eviction possible on
//! the next cache operation; ownership of returned victims still holds memory.
//!
//! ```
//! use memory_budget::{CacheLimits, LeaseCache, MemoryBudget};
//!
//! let budget = MemoryBudget::new(8);
//! let mut cache = LeaseCache::new(CacheLimits::bytes(8));
//! let pixels = budget.try_buffer(8, 42u8)?.freeze();
//! cache.insert("frame", pixels, 8).unwrap();
//! let visible = cache.get(&"frame").unwrap();
//! assert_eq!(budget.used(), 8);
//! assert!(cache.set_limits(CacheLimits::bytes(0)).is_none());
//! assert_eq!(visible[0], 42);
//! drop(visible);
//! let evicted = cache.set_limits(CacheLimits::bytes(0)).unwrap();
//! assert_eq!(cache.resident_weight(), 0);
//! assert_eq!(budget.used(), 8); // The eviction result still owns the buffer.
//! drop(evicted);
//! assert_eq!(budget.used(), 0);
//! # Ok::<(), memory_budget::BufferError>(())
//! ```
mod budget;
mod buffer;
mod lease_cache;
mod policy;
pub use budget::{BufferError, MemoryBudget, Reservation};
pub use buffer::{MutableBuffer, PixelBuffer, SharedBuffer};
pub use lease_cache::{CacheLease, LeaseCache};
pub use policy::{CacheLimits, WeightedLru};
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
mod tests {
    #[test]
    fn admitted_non_copy_owners_drop_only_after_last_shared_owner() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        #[derive(Debug)]
        struct Owner(Arc<AtomicUsize>);
        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicUsize::new(0));
        let values = vec![Owner(dropped.clone()), Owner(dropped.clone())];
        let bytes = (values.capacity() * std::mem::size_of::<Owner>()) as u64;
        let budget = MemoryBudget::new(bytes);
        let buffer = budget
            .try_reserve(bytes)
            .unwrap()
            .try_adopt(values)
            .unwrap();
        let shared = buffer.clone();
        drop(buffer);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        assert_eq!(budget.used(), bytes);
        drop(shared);
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
        assert_eq!(budget.used(), 0);
        let short = MemoryBudget::new(bytes - 1);
        let credit = short.try_reserve(bytes - 1).unwrap();
        assert!(
            credit
                .try_adopt(vec![Owner(dropped.clone()), Owner(dropped.clone())])
                .is_err()
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 4);
        assert_eq!(short.used(), 0);
    }

    #[test]
    fn paired_headroom_counts_shared_ancestors_without_reserving() {
        use super::MemoryBudget;
        let root = MemoryBudget::new(96);
        let left = root.child(64).child(64);
        let right = root.child(64).child(64);
        assert!(left.can_reserve_with(48, &right, 48));
        assert!(!left.can_reserve_with(49, &right, 48));
        assert!(left.can_reserve_with(32, &left, 32));
        assert!(!left.can_reserve_with(33, &left, 32));
        assert!(left.can_reserve_with(48, &root, 48));
        assert!(!root.can_reserve_with(49, &left, 48));
        let pressure = root.try_reserve(1).unwrap();
        assert!(!left.can_reserve_with(48, &right, 48));
        drop(pressure);
        let independent = MemoryBudget::new(64);
        assert!(left.can_reserve_with(64, &independent, 64));
        assert!(!left.can_reserve_with(65, &independent, 0));
        assert_eq!(
            (left.used(), right.used(), root.used(), independent.used()),
            (0, 0, 0, 0)
        );
        assert_eq!((left.peak(), right.peak(), independent.peak()), (0, 0, 0));
        let huge = MemoryBudget::new(u64::MAX);
        assert!(!huge.can_reserve_with(u64::MAX, &huge, 1));
        assert!(huge.can_reserve_with(u64::MAX - 1, &huge, 1));
    }
    use super::*;
    #[test]
    fn merging_full_budget_transfers_credit_through_all_ancestors_without_admission() {
        let root = MemoryBudget::new(16);
        let branch = root.child(16);
        let leaf = branch.child(16);
        let mut target = leaf.try_reserve(4).unwrap();
        let mut source = leaf.clone().try_reserve(12).unwrap();
        target.merge_from(&mut source).unwrap();
        assert_eq!(target.bytes(), 16);
        assert_eq!(source.bytes(), 0);
        for budget in [&root, &branch, &leaf] {
            assert_eq!(budget.used(), 16);
            assert_eq!(budget.peak(), 16);
        }
        drop(source);
        let buffer = target.try_adopt(vec![7_u8; 16]).unwrap();
        let held = buffer.clone();
        drop(buffer);
        assert_eq!(root.used(), 16);
        drop(held);
        for budget in [&root, &branch, &leaf] {
            assert_eq!(budget.used(), 0);
        }
    }
    #[test]
    fn merging_different_local_budgets_preserves_credit_even_with_shared_parent() {
        let root = MemoryBudget::new(16);
        let a = root.child(8);
        let b = root.child(8);
        let foreign = MemoryBudget::new(8);
        let mut target = a.try_reserve(4).unwrap();
        let mut sibling = b.try_reserve(8).unwrap();
        let mut unrelated = foreign.try_reserve(8).unwrap();
        assert!(matches!(
            target.merge_from(&mut sibling),
            Err(BufferError::BudgetMismatch)
        ));
        assert!(matches!(
            target.merge_from(&mut unrelated),
            Err(BufferError::BudgetMismatch)
        ));
        assert_eq!(
            (target.bytes(), sibling.bytes(), unrelated.bytes()),
            (4, 8, 8)
        );
        assert_eq!(root.used(), 12);
        assert_eq!((a.used(), b.used(), foreign.used()), (4, 8, 8));
        drop(target);
        drop(sibling);
        drop(unrelated);
        assert_eq!(root.used(), 0);
        assert_eq!(foreign.used(), 0);
    }
    #[test]
    fn legacy_arc_adoption_preserves_allocation_and_last_owner_accounting() {
        let budget = MemoryBudget::new(16);
        let source = Arc::new(vec![7_u16; 8]);
        let pointer = source.as_ptr();
        let buffer = budget.try_adopt_arc(source).unwrap();
        assert_eq!(buffer.as_ptr(), pointer);
        let consumer = buffer.clone();
        drop(buffer);
        assert_eq!(budget.used(), 16);
        assert_eq!(consumer[7], 7);
        drop(consumer);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn shared_legacy_arc_is_rejected_without_reservation_or_data_changes() {
        let budget = MemoryBudget::new(16);
        let source = Arc::new(vec![9_u16; 8]);
        assert!(matches!(
            budget.try_adopt_arc(source.clone()),
            Err(BufferError::SharedOwners)
        ));
        assert_eq!(budget.used(), 0);
        assert_eq!(Arc::strong_count(&source), 1);
        assert_eq!(source[0], 9);
        let small = MemoryBudget::new(1);
        assert!(matches!(
            small.try_adopt_arc(source),
            Err(BufferError::Capacity { .. })
        ));
        assert_eq!(small.used(), 0);
    }

    #[test]
    fn shared_owners_hold_reservation_until_last_drop() {
        let budget = MemoryBudget::new(16);
        let mut buffer = budget.try_buffer(8, 0_u16).unwrap();
        buffer[3] = 42;
        let buffer = buffer.freeze();
        let consumer = buffer.clone();
        assert!(buffer.ptr_eq(&consumer));
        assert_eq!(consumer[3], 42);
        assert_eq!(budget.used(), 16);
        assert!(budget.try_reserve(1).is_err());
        drop(buffer);
        assert_eq!(budget.used(), 16);
        drop(consumer);
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 16);
    }
    #[test]
    fn capacity_overflow_cancellation_and_adoption() {
        let budget = MemoryBudget::new(u64::MAX);
        let reservation = budget.try_reserve(u64::MAX).unwrap();
        assert!(budget.try_reserve(1).is_err());
        drop(reservation);
        assert_eq!(budget.used(), 0);
        assert!(matches!(
            budget.try_buffer(usize::MAX, 0_u16),
            Err(BufferError::Overflow)
        ));
        let values = Vec::<u8>::with_capacity(128);
        let shared = budget.try_adopt(values).unwrap();
        assert_eq!(shared.capacity_bytes(), 128);
        assert_eq!(budget.used(), 128);
        drop(shared);
        assert_eq!(budget.used(), 0);
        let zero = budget.try_buffer(3, ()).unwrap();
        assert_eq!(zero.len(), 3);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn concurrent_reservations_never_overcommit() {
        let budget = MemoryBudget::new(64);
        let gate = Arc::new(std::sync::Barrier::new(16));
        std::thread::scope(|scope| {
            for _ in 0..16 {
                let budget = budget.clone();
                let gate = gate.clone();
                scope.spawn(move || {
                    let reservation = budget.try_reserve(8).ok();
                    gate.wait();
                    assert!(budget.used() <= 64);
                    drop(reservation);
                });
            }
        });
        assert_eq!(budget.used(), 0);
        assert_eq!(budget.peak(), 64);
    }

    #[test]
    fn adoption_keeps_allocation_and_float_bits_unchanged() {
        let budget = MemoryBudget::new(32);
        let source = vec![f32::from_bits(0x7fc1_2345), -0.0, 100_000.0, -0.25];
        let pointer = source.as_ptr();
        let bits: Vec<_> = source.iter().map(|v| v.to_bits()).collect();
        let buffer = budget.try_adopt(source).unwrap();
        assert_eq!(buffer.as_ptr(), pointer);
        assert_eq!(buffer.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), bits);
        assert_eq!(budget.used(), 16);
        drop(buffer);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn child_budgets_share_parent_and_keep_live_buffer_reservations() {
        let root = MemoryBudget::new(16);
        let a = root.child(12);
        let b = root.child(12);
        let buffer = a.try_buffer(8, 0_u8).unwrap().freeze();
        let owner = buffer.clone();
        drop(buffer);
        assert_eq!(root.used(), 8);
        assert!(b.try_reserve(9).is_err());
        assert_eq!(b.used(), 0);
        assert!(a.try_reserve(5).is_err());
        assert_eq!(root.used(), 8);
        let mut reservation = b.try_reserve(4).unwrap();
        reservation.grow(4).unwrap();
        assert_eq!(root.used(), 16);
        assert_eq!(b.used(), 8);
        assert!(reservation.grow(1).is_err());
        assert_eq!(root.used(), 16);
        drop(reservation);
        assert_eq!(root.used(), 8);
        drop(owner);
        assert_eq!(root.used(), 0);
        assert_eq!(a.used(), 0);
        assert_eq!(b.used(), 0);
    }
    #[test]
    fn available_bytes_tracks_ancestor_pressure_and_last_buffer_owner() {
        let root = MemoryBudget::new(32);
        let decode = root.child(24);
        let scratch = decode.child(20);
        let cache = root.child(32);
        assert!(scratch.is_descendant_of(&root));
        assert!(scratch.is_descendant_of(&decode));
        assert!(decode.is_descendant_of(&decode.clone()));
        assert!(!root.is_descendant_of(&scratch));
        assert!(!scratch.is_descendant_of(&cache));
        assert!(!scratch.is_descendant_of(&MemoryBudget::new(32)));
        assert_eq!(scratch.available_bytes(), 20);
        let buffer = cache.try_buffer(20, 0_u8).unwrap().freeze();
        let owner = buffer.clone();
        assert_eq!(scratch.available_bytes(), 12);
        let reservation = scratch.try_reserve(10).unwrap();
        assert_eq!(scratch.available_bytes(), 2);
        assert_eq!(decode.available_bytes(), 2);
        assert_eq!(cache.available_bytes(), 2);
        assert!(scratch.try_reserve(3).is_err());
        drop(buffer);
        assert_eq!(scratch.available_bytes(), 2);
        drop(owner);
        assert_eq!(scratch.available_bytes(), 10);
        drop(reservation);
        assert_eq!(scratch.available_bytes(), 20);
        assert_eq!(decode.available_bytes(), 24);
        assert_eq!(root.available_bytes(), 32);
        assert_eq!(root.child(0).available_bytes(), 0);
    }
}

#[cfg(test)]
mod staged_buffer_tests {
    use super::*;

    #[test]
    fn staged_growth_charges_all_ancestors_and_overflow_releases_credit() {
        let root = MemoryBudget::new(32);
        let child = root.child(24);
        let leaf = child.child(16);
        let other_owner = root.try_reserve(16).unwrap();
        let reserved = leaf.try_reserve(4).unwrap();
        let buffer = reserved.try_buffer(4, 11u32).unwrap().freeze();
        assert_eq!(&*buffer, &[11; 4]);
        assert_eq!(root.used(), 32);
        assert_eq!(child.used(), 16);
        assert_eq!(leaf.used(), 16);
        drop(buffer);
        assert_eq!(root.used(), 16);
        assert_eq!(child.used(), 0);
        assert_eq!(leaf.used(), 0);

        let reserved = leaf.try_reserve(4).unwrap();
        assert!(matches!(
            reserved.try_buffer(usize::MAX, 0u16),
            Err(BufferError::Overflow)
        ));
        assert_eq!(root.used(), 16);
        assert_eq!(child.used(), 0);
        assert_eq!(leaf.used(), 0);
        drop(other_owner);
        assert_eq!(root.used(), 0);
    }

    #[test]
    fn staged_buffer_reuses_credit_and_preserves_child_and_last_owner_accounting() {
        let root = MemoryBudget::new(32);
        let child = root.child(16);
        let reservation = child.try_reserve(16).unwrap();
        let mut buffer = reservation.try_buffer(4, 7u32).unwrap();
        assert_eq!(root.used(), 16);
        assert_eq!(child.used(), 16);
        assert_eq!(root.peak(), 16);
        buffer[1] = 9;
        let buffer = buffer.freeze();
        let clone = buffer.clone();
        drop(buffer);
        assert_eq!(&*clone, &[7, 9, 7, 7]);
        assert_eq!(root.used(), 16);
        drop(clone);
        assert_eq!(root.used(), 0);
        assert_eq!(child.used(), 0);

        let reservation = child.try_reserve(16).unwrap();
        assert!(matches!(
            reservation.try_buffer(5, 0u32),
            Err(BufferError::Capacity { .. })
        ));
        assert_eq!(root.used(), 0);
        assert_eq!(child.used(), 0);
        let reservation = child.try_reserve(16).unwrap();
        let smaller = reservation.try_buffer(1, 1u8).unwrap();
        assert_eq!(root.used(), 16);
        drop(smaller);
        assert_eq!(root.used(), 0);
    }
}
