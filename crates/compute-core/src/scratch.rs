use crate::{ComputeError, ComputeRuntime, GpuArray, GpuElement};
use gpu_compute::{GpuBuffer, wgpu};
use memory_budget::{MemoryBudget, Reservation};
use std::collections::HashMap;

struct Slot {
    buffer: GpuBuffer,
    generation: u64,
    // Declared after the buffer so the handle drops before its credit returns.
    reservation: Reservation,
}

/// Telemetry counters for a [`ScratchPool`] session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScratchPoolStats {
    pub hits: usize,
    pub misses: usize,
    pub reallocations: usize,
    pub reserved_bytes: u64,
    pub budget_bytes: u64,
    pub slot_count: usize,
}

/// Named reusable storage owned by a session. The budget bounds the capacity
/// retained by this pool, not total device memory (old handles and in-flight
/// commands may retain replaced buffers). Growth creates a new generation;
/// rebuild prepared bindings when the generation changes. Old plans remain valid
/// and continue to use their original allocation until explicitly rebuilt.
pub struct ScratchPool<'a> {
    runtime: &'a ComputeRuntime,
    slots: HashMap<String, Slot>,
    budget: MemoryBudget,
    hits: usize,
    misses: usize,
    reallocations: usize,
}
pub struct ScratchArray<T: GpuElement> {
    pub array: GpuArray<T>,
    pub generation: u64,
}
impl<'a> ScratchPool<'a> {
    pub fn new(runtime: &'a ComputeRuntime, budget_bytes: u64) -> Self {
        Self::with_budget(runtime, MemoryBudget::new(budget_bytes))
    }
    /// Charges retained capacity to `budget`, which may be a child of an application budget.
    pub fn with_budget(runtime: &'a ComputeRuntime, budget: MemoryBudget) -> Self {
        Self {
            runtime,
            slots: HashMap::new(),
            budget,
            hits: 0,
            misses: 0,
            reallocations: 0,
        }
    }
    pub fn reserved_bytes(&self) -> u64 {
        self.budget.used()
    }
    pub fn stats(&self) -> ScratchPoolStats {
        ScratchPoolStats {
            hits: self.hits,
            misses: self.misses,
            reallocations: self.reallocations,
            reserved_bytes: self.budget.used(),
            budget_bytes: self.budget.limit(),
            slot_count: self.slots.len(),
        }
    }
    fn exceeded(&self, extra: u64) -> ComputeError {
        ComputeError::BudgetExceeded {
            requested: self.budget.used().saturating_add(extra),
            budget: self.budget.limit(),
        }
    }
    fn storage(&self, bytes: u64) -> Result<GpuBuffer, ComputeError> {
        Ok(GpuBuffer::new(
            self.runtime.context(),
            bytes,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        )?)
    }
    pub fn reserve<T: GpuElement>(
        &mut self,
        name: &str,
        len: usize,
    ) -> Result<ScratchArray<T>, ComputeError> {
        if len > u32::MAX as usize {
            return Err(ComputeError::OutOfBounds);
        }
        let bytes = (len as u64 * 4).max(4);
        if let Some(slot) = self.slots.get(name) {
            if slot.buffer.size() >= bytes {
                let array = self.runtime.import_buffer(slot.buffer.clone(), len)?;
                let generation = slot.generation;
                self.hits += 1;
                return Ok(ScratchArray { array, generation });
            }
            let extra = bytes - slot.reservation.bytes();
            if extra > self.budget.available_bytes() {
                return Err(self.exceeded(extra));
            }
            let buffer = self.storage(bytes)?;
            let array = self.runtime.import_buffer(buffer.clone(), len)?;
            let exceeded = self.exceeded(extra);
            let slot = self.slots.get_mut(name).expect("slot checked above");
            // A shared ancestor can still have filled up since the headroom check.
            slot.reservation.ensure_bytes(bytes).map_err(|_| exceeded)?;
            slot.buffer = buffer;
            slot.generation += 1;
            self.reallocations += 1;
            return Ok(ScratchArray {
                array,
                generation: slot.generation,
            });
        }
        let reservation = self
            .budget
            .try_reserve(bytes)
            .map_err(|_| self.exceeded(bytes))?;
        let buffer = self.storage(bytes)?;
        let array = self.runtime.import_buffer(buffer.clone(), len)?;
        self.slots.insert(
            name.to_owned(),
            Slot {
                buffer,
                generation: 1,
                reservation,
            },
        );
        self.misses += 1;
        Ok(ScratchArray {
            array,
            generation: 1,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_pool_tracks_hits_misses_and_reallocations() {
        let Some(context) = gpu_compute::GpuContext::new() else {
            return;
        };
        let runtime = ComputeRuntime::new(&context).unwrap();
        let mut pool = ScratchPool::new(&runtime, 1024);
        let a1 = pool.reserve::<f32>("tmp", 16).unwrap();
        assert_eq!(a1.generation, 1);
        assert_eq!(
            pool.stats(),
            ScratchPoolStats {
                hits: 0,
                misses: 1,
                reallocations: 0,
                reserved_bytes: 64,
                budget_bytes: 1024,
                slot_count: 1,
            }
        );

        let a2 = pool.reserve::<f32>("tmp", 8).unwrap();
        assert_eq!(a2.generation, 1);
        assert_eq!(pool.stats().hits, 1);

        let a3 = pool.reserve::<f32>("tmp", 32).unwrap();
        assert_eq!(a3.generation, 2);
        assert_eq!(pool.stats().reallocations, 1);
        assert_eq!(pool.stats().reserved_bytes, 128);
    }
}
