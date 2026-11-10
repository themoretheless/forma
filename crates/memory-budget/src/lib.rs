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
use std::{
    collections::TryReserveError,
    fmt,
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
mod lease_cache;
mod policy;
pub use lease_cache::{CacheLease, LeaseCache};
pub use policy::{CacheLimits, WeightedLru};
#[derive(Debug)]
struct BudgetState {
    parent: Option<MemoryBudget>,
    limit: u64,
    used: AtomicU64,
    peak: AtomicU64,
}
#[derive(Debug, Clone)]
pub struct MemoryBudget(Arc<BudgetState>);
#[derive(Debug)]
pub enum BufferError {
    Capacity {
        requested: u64,
        used: u64,
        limit: u64,
    },
    Overflow,
    SharedOwners,
    BudgetMismatch,
    Allocate(TryReserveError),
}
impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Capacity {
                requested,
                used,
                limit,
            } => write!(
                f,
                "memory reservation {requested} exceeds remaining budget (used {used}, limit {limit})"
            ),
            Self::Overflow => f.write_str("buffer capacity overflow"),
            Self::SharedOwners => {
                f.write_str("buffer adoption requires exclusive allocation ownership")
            }
            Self::BudgetMismatch => f.write_str("reservation merge requires the same local budget"),
            Self::Allocate(e) => write!(f, "buffer allocation failed: {e}"),
        }
    }
}
impl std::error::Error for BufferError {}
impl MemoryBudget {
    pub fn new(limit: u64) -> Self {
        Self::with_parent(None, limit)
    }
    fn with_parent(parent: Option<Self>, limit: u64) -> Self {
        Self(Arc::new(BudgetState {
            parent,
            limit,
            used: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }))
    }
    /// This budget followed by each of its ancestors up to the root.
    fn ancestors(&self) -> impl Iterator<Item = &Self> {
        std::iter::successors(Some(self), |budget| budget.0.parent.as_ref())
    }
    /// Independent local cap sharing its parent's allocation budget.
    /// Reservations count in every ancestor until the last buffer owner drops.
    #[must_use]
    pub fn child(&self, limit: u64) -> Self {
        Self::with_parent(Some(self.clone()), limit)
    }
    /// Whether reservations in this budget also count in `ancestor`.
    /// Includes the same budget; independent roots and sibling budgets differ.
    #[must_use]
    pub fn is_descendant_of(&self, ancestor: &Self) -> bool {
        self.ancestors()
            .any(|budget| Arc::ptr_eq(&budget.0, &ancestor.0))
    }
    pub fn limit(&self) -> u64 {
        self.0.limit
    }
    /// Maximum allocation permitted by local and ancestor caps, ignoring use.
    pub fn allocation_limit(&self) -> u64 {
        self.ancestors().map(Self::limit).min().unwrap_or(u64::MAX)
    }
    pub fn used(&self) -> u64 {
        self.0.used.load(Ordering::Acquire)
    }
    pub fn peak(&self) -> u64 {
        self.0.peak.load(Ordering::Acquire)
    }
    /// Advisory headroom across this budget and all its ancestors.
    /// Concurrent reservations can change the result immediately; allocation
    /// must still use `try_reserve` or `try_buffer` for atomic admission.
    pub fn available_bytes(&self) -> u64 {
        self.ancestors()
            .map(Self::local_headroom)
            .min()
            .unwrap_or(u64::MAX)
    }
    fn local_headroom(&self) -> u64 {
        self.limit().saturating_sub(self.used())
    }
    /// Advisory simultaneous admission across two budgets, including shared ancestors.
    /// Does not reserve memory or update peaks; concurrent allocations still require
    /// `try_reserve` at the actual allocation boundary.
    #[must_use]
    pub fn can_reserve_with(&self, bytes: u64, other: &Self, other_bytes: u64) -> bool {
        for (first, amount, second, second_amount) in [
            (self, bytes, other, other_bytes),
            (other, other_bytes, self, bytes),
        ] {
            for budget in first.ancestors() {
                let shared = if second.is_descendant_of(budget) {
                    second_amount
                } else {
                    0
                };
                let Some(total) = amount.checked_add(shared) else {
                    return false;
                };
                if total > budget.local_headroom() {
                    return false;
                }
            }
        }
        true
    }
    /// Nonblocking admission; the caller decides whether to evict, wait or cancel.
    ///
    /// # Errors
    /// Returns `BufferError::Capacity` when managed reservations would exceed the limit.
    pub fn try_reserve(&self, bytes: u64) -> Result<Reservation, BufferError> {
        let parent = self
            .0
            .parent
            .as_ref()
            .map(|budget| budget.try_reserve(bytes).map(Box::new))
            .transpose()?;
        let mut used = self.used();
        loop {
            if bytes > self.0.limit - used {
                return Err(BufferError::Capacity {
                    requested: bytes,
                    used,
                    limit: self.0.limit,
                });
            }
            match self.0.used.compare_exchange_weak(
                used,
                used + bytes,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.0.peak.fetch_max(used + bytes, Ordering::Relaxed);
                    return Ok(Reservation {
                        parent,
                        budget: self.clone(),
                        bytes,
                    });
                }
                Err(current) => used = current,
            }
        }
    }
    /// Reserves before allocation, then accounts for actual reported Vec capacity.
    /// Values are flat `Copy` elements, so initialization does not clone owned heap data.
    ///
    /// # Errors
    /// Rejects capacity arithmetic overflow, exhausted budget and allocation failure.
    pub fn try_buffer<T: Copy>(
        &self,
        length: usize,
        value: T,
    ) -> Result<MutableBuffer<T>, BufferError> {
        let requested = buffer_bytes::<T>(length)?;
        self.try_reserve(requested)?.try_buffer(length, value)
    }
    /// Adopts a legacy shared vector only when its allocation has one strong owner.
    /// No samples are copied. Existing external owners must be released first,
    /// otherwise their lifetime could escape the managed reservation.
    /// Construction of the original vector is outside this budget.
    ///
    /// # Errors
    /// Returns `SharedOwners` for a shared allocation, or the normal adoption errors.
    pub fn try_adopt_arc<T: Copy>(
        &self,
        values: Arc<Vec<T>>,
    ) -> Result<SharedBuffer<T>, BufferError> {
        let values = Arc::try_unwrap(values).map_err(|_| BufferError::SharedOwners)?;
        self.try_adopt(values)
    }

    /// Adopts an already allocated owned Vec; this does not budget its construction.
    ///
    /// # Errors
    /// Rejects capacity arithmetic overflow or exhausted budget.
    pub fn try_adopt<T: Copy>(&self, values: Vec<T>) -> Result<SharedBuffer<T>, BufferError> {
        self.try_reserve(buffer_bytes::<T>(values.capacity())?)?
            .try_adopt(values)
    }
}
fn buffer_bytes<T>(capacity: usize) -> Result<u64, BufferError> {
    capacity
        .checked_mul(std::mem::size_of::<T>())
        .and_then(|v| u64::try_from(v).ok())
        .ok_or(BufferError::Overflow)
}
#[derive(Debug)]
pub struct Reservation {
    parent: Option<Box<Reservation>>,
    budget: MemoryBudget,
    bytes: u64,
}
impl Reservation {
    /// Allocate mutable storage from already admitted credit without charging
    /// it twice. Conservative excess credit remains attached to the buffer.
    /// Additional capacity is admitted before initialization; errors roll back.
    /// # Errors
    /// Returns an error on byte-size overflow, exhausted memory credit or allocation failure.
    pub fn try_buffer<T: Copy>(
        mut self,
        length: usize,
        value: T,
    ) -> Result<MutableBuffer<T>, BufferError> {
        self.ensure_bytes(buffer_bytes::<T>(length)?)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(length)
            .map_err(BufferError::Allocate)?;
        self.ensure_bytes(buffer_bytes::<T>(values.capacity())?)?;
        values.resize_with(length, || value);
        Ok(MutableBuffer {
            values,
            reservation: self,
        })
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    /// Transfers a pre-allocation reservation into owned flat storage without copying.
    /// Keeps any conservative excess reservation; admits extra capacity if necessary.
    ///
    /// # Errors
    /// Rejects capacity arithmetic overflow and admission beyond the budget limit.
    pub fn try_adopt<T>(mut self, values: Vec<T>) -> Result<SharedBuffer<T>, BufferError> {
        self.ensure_bytes(buffer_bytes::<T>(values.capacity())?)?;
        Ok(MutableBuffer {
            values,
            reservation: self,
        }
        .freeze())
    }
    /// Admits at least the requested retained capacity, preserving existing credit.
    /// Failure leaves this reservation unchanged.
    ///
    /// # Errors
    /// Returns `BufferError::Capacity` when additional credit cannot be admitted.
    pub fn ensure_bytes(&mut self, bytes: u64) -> Result<(), BufferError> {
        if bytes > self.bytes {
            self.grow(bytes - self.bytes)?;
        }
        Ok(())
    }
    /// Transfers credit from another reservation in the same local budget.
    /// No counters change or new admission occurs. On success the source has
    /// zero credit; failure leaves both reservations unchanged. Sharing only an
    /// ancestor is insufficient because each local budget must retain its cap.
    ///
    /// # Errors
    /// Returns `BudgetMismatch` for distinct local budgets.
    pub fn merge_from(&mut self, source: &mut Self) -> Result<(), BufferError> {
        if !Arc::ptr_eq(&self.budget.0, &source.budget.0) {
            return Err(BufferError::BudgetMismatch);
        }
        self.absorb(source);
        Ok(())
    }
    fn grow(&mut self, extra: u64) -> Result<(), BufferError> {
        let mut additional = self.budget.try_reserve(extra)?;
        self.absorb(&mut additional);
        Ok(())
    }
    fn absorb(&mut self, additional: &mut Self) {
        self.bytes += additional.bytes;
        additional.bytes = 0;
        if let (Some(parent), Some(extra_parent)) = (&mut self.parent, &mut additional.parent) {
            parent.absorb(extra_parent);
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.0.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
#[derive(Debug)]
pub struct MutableBuffer<T> {
    values: Vec<T>,
    reservation: Reservation,
}
impl<T> Deref for MutableBuffer<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values
    }
}
impl<T> DerefMut for MutableBuffer<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values
    }
}
impl<T> MutableBuffer<T> {
    pub fn capacity_bytes(&self) -> u64 {
        self.reservation.bytes()
    }
    pub fn freeze(self) -> SharedBuffer<T> {
        SharedBuffer(Arc::new(self))
    }
}
/// Cloning shares the allocation and reservation. No mutable Vec/Arc escapes.
#[derive(Debug)]
pub struct SharedBuffer<T>(Arc<MutableBuffer<T>>);
impl<T> Clone for SharedBuffer<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}
impl<T> Deref for SharedBuffer<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.0.values
    }
}
impl<T> SharedBuffer<T> {
    pub fn capacity_bytes(&self) -> u64 {
        self.0.capacity_bytes()
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
/// Immutable pixels supporting legacy ownership and explicitly budgeted ownership.
/// Legacy allocations are unaccounted until exclusive adoption succeeds.
#[derive(Debug, Clone)]
pub struct PixelBuffer<T>(PixelStorage<T>);
#[derive(Debug, Clone)]
enum PixelStorage<T> {
    Legacy(Arc<Vec<T>>),
    Managed(SharedBuffer<T>),
}
impl<T> From<Arc<Vec<T>>> for PixelBuffer<T> {
    fn from(values: Arc<Vec<T>>) -> Self {
        Self(PixelStorage::Legacy(values))
    }
}
impl<T> From<SharedBuffer<T>> for PixelBuffer<T> {
    fn from(values: SharedBuffer<T>) -> Self {
        Self(PixelStorage::Managed(values))
    }
}
impl<T> Deref for PixelBuffer<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match &self.0 {
            PixelStorage::Legacy(values) => values,
            PixelStorage::Managed(values) => values,
        }
    }
}
impl<T: PartialEq> PartialEq for PixelBuffer<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl<T> AsRef<[T]> for PixelBuffer<T> {
    fn as_ref(&self) -> &[T] {
        self
    }
}
impl<T> PixelBuffer<T> {
    /// Mutates only exclusively owned storage; never copies or changes its budget.
    pub fn get_mut(&mut self) -> Option<&mut [T]> {
        match &mut self.0 {
            PixelStorage::Legacy(values) => Arc::get_mut(values).map(Vec::as_mut_slice),
            PixelStorage::Managed(values) => {
                Arc::get_mut(&mut values.0).map(|v| v.values.as_mut_slice())
            }
        }
    }
    pub fn as_slice(&self) -> &[T] {
        self
    }
    pub fn is_managed(&self) -> bool {
        matches!(self.0, PixelStorage::Managed(_))
    }
    pub fn capacity_bytes(&self) -> u64 {
        match &self.0 {
            PixelStorage::Legacy(values) => {
                buffer_bytes::<T>(values.capacity()).unwrap_or(u64::MAX)
            }
            PixelStorage::Managed(values) => values.capacity_bytes(),
        }
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (PixelStorage::Legacy(a), PixelStorage::Legacy(b)) => Arc::ptr_eq(a, b),
            (PixelStorage::Managed(a), PixelStorage::Managed(b)) => a.ptr_eq(b),
            _ => false,
        }
    }
}
impl<T: Copy> PixelBuffer<T> {
    /// Converts exclusive legacy ownership without copying the allocation.
    /// Already managed buffers retain their original budget.
    ///
    /// # Errors
    /// Rejects external legacy owners and exhausted budget.
    pub fn try_manage(self, budget: &MemoryBudget) -> Result<Self, BufferError> {
        match self.0 {
            PixelStorage::Legacy(values) => budget.try_adopt_arc(values).map(Self::from),
            PixelStorage::Managed(values) => Ok(Self::from(values)),
        }
    }
}

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
