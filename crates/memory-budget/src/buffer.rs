//! Flat storage that keeps its budget reservation for as long as it lives.
use crate::budget::{BufferError, MemoryBudget, Reservation, buffer_bytes};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};
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
    /// Storage whose capacity `reservation` already covers.
    pub(crate) fn new(values: Vec<T>, reservation: Reservation) -> Self {
        Self {
            values,
            reservation,
        }
    }
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
