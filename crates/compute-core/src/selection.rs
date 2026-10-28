use crate::scan::ScanPlan;
use crate::{Binding, ComputeError, ComputeProgram, GpuArray, Kernel, shaders, uniform_f32, wgpu};

/// Stable selection with a GPU-resident logical length. After its producing
/// program executes, `values()[..count()[0]]` contains selected values in input
/// order and the remaining capacity is zero. Updating either array afterwards
/// invalidates this invariant until the producer executes again.
#[derive(Clone)]
pub struct CompactedArray {
    values: GpuArray<f32>,
    count: GpuArray<u32>,
}

impl CompactedArray {
    /// Full input-sized capacity, including a zero-filled tail. `sum(values())`
    /// is valid without a CPU count read. Operations that change zero (such as
    /// adding a constant) must separately respect the logical count.
    pub fn values(&self) -> &GpuArray<f32> {
        &self.values
    }
    /// One u32 on the GPU; never inferred by reading the values' zero tail.
    pub fn count(&self) -> &GpuArray<u32> {
        &self.count
    }
    pub fn capacity(&self) -> usize {
        self.values.len()
    }
}

pub(crate) struct SelectionKernels {
    scatter: Kernel,
}

impl SelectionKernels {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, crate::KernelError> {
        Ok(Self {
            scatter: Kernel::new(
                device,
                "stable compact scatter",
                shaders::COMPACT_SCATTER_LOCAL_WGSL,
                "main",
                &[
                    Binding::Uniform,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageRead,
                    Binding::StorageReadWrite,
                    Binding::StorageReadWrite,
                ],
            )?,
        })
    }
}

impl ComputeProgram<'_> {
    /// Keeps every input whose equal-length mask is nonzero, preserving order.
    /// Output capacity equals input length; count and every intermediate stay on
    /// GPU. The tail is cleared on each execution, including repeated programs.
    pub fn compact(
        &mut self,
        input: &GpuArray<f32>,
        keep: &GpuArray<u32>,
    ) -> Result<CompactedArray, ComputeError> {
        self.check_selection_input(input, keep)?;
        let output = self.runtime.zeros(input.len())?;
        let count = self.runtime.zeros(1)?;
        self.compact_into(input, keep, &output, &count)
    }

    fn check_selection_input(
        &self,
        input: &GpuArray<f32>,
        keep: &GpuArray<u32>,
    ) -> Result<(), ComputeError> {
        self.runtime.check(input)?;
        self.runtime.check(keep)?;
        if input.len() != keep.len() {
            return Err(ComputeError::LengthMismatch {
                expected: input.len(),
                actual: keep.len(),
            });
        }
        Ok(())
    }

    /// Reuses caller-owned output and count. Output must match input length,
    /// count must have exactly one element, and neither may alias any input or
    /// each other (even when the same bytes were imported with different types).
    pub fn compact_into(
        &mut self,
        input: &GpuArray<f32>,
        keep: &GpuArray<u32>,
        output: &GpuArray<f32>,
        count: &GpuArray<u32>,
    ) -> Result<CompactedArray, ComputeError> {
        self.check_selection_input(input, keep)?;
        self.runtime.check(output)?;
        self.runtime.check(count)?;
        if output.len() != input.len() {
            return Err(ComputeError::LengthMismatch {
                expected: input.len(),
                actual: output.len(),
            });
        }
        if count.len() != 1 {
            return Err(ComputeError::LengthMismatch {
                expected: 1,
                actual: count.len(),
            });
        }
        if [input.buffer(), keep.buffer(), count.buffer()].contains(&output.buffer())
            || [input.buffer(), keep.buffer()].contains(&count.buffer())
        {
            return Err(ComputeError::AliasedOutput);
        }
        let offsets = self.runtime.zeros(input.len())?;
        let (scan, block_offsets, block_size) = ScanPlan::local(self.runtime, keep, &offsets)?;
        let scatter = &self.runtime.selection.scatter;
        let groups = scatter.workgroup_count(input.len).clamp(1, 65535);
        let params = uniform_f32(
            &self.runtime.device,
            &self.runtime.queue,
            &[
                f32::from_bits(input.len),
                f32::from_bits(groups),
                f32::from_bits(block_size),
                0.0,
            ],
        );
        let scatter_bindings = scatter.create_bind_group(
            &self.runtime.device,
            &[
                &params,
                input.buffer(),
                keep.buffer(),
                offsets.buffer(),
                block_offsets.buffer(),
                output.buffer(),
                count.buffer(),
            ],
        );
        scan.append_to(&mut self.batch);
        // Also resets count for empty inputs and overwrites stale tail on reuse.
        self.batch.push(scatter, &scatter_bindings, groups);
        Ok(CompactedArray {
            values: output.clone(),
            count: count.clone(),
        })
    }
}
