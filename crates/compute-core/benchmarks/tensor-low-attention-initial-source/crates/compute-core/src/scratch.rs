use crate::{ComputeError, ComputeRuntime, GpuArray, GpuElement};
use gpu_compute::{GpuBuffer, wgpu};
use std::collections::HashMap;

struct Slot {
    buffer: GpuBuffer,
    generation: u64,
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
        }
    }
    pub fn reserved_bytes(&self) -> u64 {
        self.reserved
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
        let old = self.slots.get(name);
        if let Some(slot) = old
            && slot.buffer.size() >= bytes
        {
            return Ok(ScratchArray {
                array: self.runtime.import_buffer(slot.buffer.clone(), len)?,
                generation: slot.generation,
            });
        }
        let previous = old.map_or(0, |slot| slot.buffer.size());
        let generation = old.map_or(1, |slot| slot.generation + 1);
        let reserved = self.reserved - previous + bytes;
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
            .insert(name.to_owned(), Slot { buffer, generation });
        self.reserved = reserved;
        Ok(ScratchArray { array, generation })
    }
}
