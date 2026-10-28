use crate::{ComputeError, ComputeRuntime, GpuArray, GpuElement};
use gpu_compute::{GpuBuffer, wgpu};
use std::collections::HashMap;

struct Slot {
    buffer: GpuBuffer,
    generation: u64,
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
    budget: u64,
    reserved: u64,
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
        Self {
            runtime,
            slots: HashMap::new(),
            budget: budget_bytes,
            reserved: 0,
            hits: 0,
            misses: 0,
            reallocations: 0,
        }
    }
    pub fn reserved_bytes(&self) -> u64 {
        self.reserved
    }
    pub fn stats(&self) -> ScratchPoolStats {
        ScratchPoolStats {
            hits: self.hits,
            misses: self.misses,
            reallocations: self.reallocations,
            reserved_bytes: self.reserved,
            budget_bytes: self.budget,
            slot_count: self.slots.len(),
        }
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
        if let Some(slot) = self.slots.get_mut(name) {
            if slot.buffer.size() >= bytes {
                self.hits += 1;
                return Ok(ScratchArray {
                    array: self.runtime.import_buffer(slot.buffer.clone(), len)?,
                    generation: slot.generation,
                });
            }
            let reserved = self.reserved - slot.buffer.size() + bytes;
            if reserved > self.budget {
                return Err(ComputeError::BudgetExceeded {
                    requested: reserved,
                    budget: self.budget,
                });
            }
            let buffer = GpuBuffer::new(
                self.runtime.context(),
                bytes,
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            )?;
            let generation = slot.generation + 1;
            let array = self.runtime.import_buffer(buffer.clone(), len)?;
            slot.buffer = buffer;
            slot.generation = generation;
            self.reserved = reserved;
            self.reallocations += 1;
            return Ok(ScratchArray { array, generation });
        }
        let reserved = self.reserved + bytes;
        if reserved > self.budget {
            return Err(ComputeError::BudgetExceeded {
                requested: reserved,
                budget: self.budget,
            });
        }
        let buffer = GpuBuffer::new(
            self.runtime.context(),
            bytes,
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        )?;
        let array = self.runtime.import_buffer(buffer.clone(), len)?;
        self.slots
            .insert(name.to_owned(), Slot { buffer, generation: 1 });
        self.reserved = reserved;
        self.misses += 1;
        Ok(ScratchArray { array, generation: 1 })
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
