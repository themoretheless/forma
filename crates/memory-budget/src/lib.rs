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
mod tests;

#[cfg(test)]
mod staged_buffer_tests;
