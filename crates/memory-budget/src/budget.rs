//! Hierarchical byte budgets and the reservations that hold their credit.
use crate::buffer::{MutableBuffer, SharedBuffer};
use std::{
    collections::TryReserveError,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
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
pub(crate) fn buffer_bytes<T>(capacity: usize) -> Result<u64, BufferError> {
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
        Ok(MutableBuffer::new(values, self))
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
        Ok(MutableBuffer::new(values, self).freeze())
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
    pub(crate) fn grow(&mut self, extra: u64) -> Result<(), BufferError> {
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
