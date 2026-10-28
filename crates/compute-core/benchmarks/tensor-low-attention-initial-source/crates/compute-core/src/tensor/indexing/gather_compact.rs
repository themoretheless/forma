use super::{GpuTensor, Result, checked_index, kind};
use crate::{ComputeProgram, GpuElement, scan::ScanPlan};
use tensor_core::{Compacted, Gathered, Shape, compact_shape, gather_shape};
impl<'a> ComputeProgram<'a> {
    /// Out-of-range indices produce zero; invalid_count counts index elements
    /// once, independently of replicated output dimensions, and remains on GPU.
    pub fn tensor_gather<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        indices: &GpuTensor<u32>,
        axis: usize,
    ) -> Result<Gathered<GpuTensor<T>, GpuTensor<u32>>> {
        self.index_check(input)?;
        self.index_check(indices)?;
        let values = self.index_new(gather_shape(input.shape(), indices.shape(), axis)?)?;
        let invalid_count = self.index_new(Shape::new(vec![])?)?;
        self.tensor_gather_into(input, indices, axis, &values, &invalid_count)?;
        Ok(Gathered {
            values,
            invalid_count,
        })
    }
    pub fn tensor_gather_into<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        indices: &GpuTensor<u32>,
        axis: usize,
        output: &GpuTensor<T>,
        invalid_count: &GpuTensor<u32>,
    ) -> Result<()> {
        self.index_check(input)?;
        self.index_check(indices)?;
        let shape = gather_shape(input.shape(), indices.shape(), axis)?;
        self.index_output(
            output,
            &shape,
            &[
                input.values().buffer(),
                indices.values().buffer(),
                invalid_count.values().buffer(),
            ],
        )?;
        self.index_output(
            invalid_count,
            &Shape::new(vec![])?,
            &[
                input.values().buffer(),
                indices.values().buffer(),
                output.values().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        let axis_length = checked_index(input.shape().dims()[axis])?;
        prepared.index_count_into(indices, axis_length, invalid_count)?;
        let kernels = self.runtime.tensor_index_kernels()?;
        let count = checked_index(shape.numel())?;
        if count > 0 {
            let groups = prepared.index_groups(count.div_ceil(256));
            let meta = super::gather_metadata(
                input.layout(),
                indices.layout(),
                output.layout(),
                axis,
                groups,
            )?;
            prepared.index_dispatch(
                &kernels.gather[kind::<T>()],
                &meta,
                &[
                    input.values().buffer(),
                    indices.values().buffer(),
                    output.values().buffer(),
                ],
                groups,
            )?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
    /// Stable logical row-major selection. Capacity equals input.numel(); count
    /// is a scalar u32 tensor and the unused output tail is zero on every run.
    pub fn tensor_compact<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        mask: &GpuTensor<u32>,
    ) -> Result<Compacted<GpuTensor<T>, GpuTensor<u32>>> {
        self.index_check(input)?;
        self.index_check(mask)?;
        let values = self.index_new(compact_shape(input.shape(), mask.shape())?)?;
        let count = self.index_new(Shape::new(vec![])?)?;
        self.tensor_compact_into(input, mask, &values, &count)?;
        Ok(Compacted { values, count })
    }
    pub fn tensor_compact_into<T: GpuElement>(
        &mut self,
        input: &GpuTensor<T>,
        mask: &GpuTensor<u32>,
        output: &GpuTensor<T>,
        count: &GpuTensor<u32>,
    ) -> Result<()> {
        self.index_check(input)?;
        self.index_check(mask)?;
        let shape = compact_shape(input.shape(), mask.shape())?;
        self.index_output(
            output,
            &shape,
            &[
                input.values().buffer(),
                mask.values().buffer(),
                count.values().buffer(),
            ],
        )?;
        self.index_output(
            count,
            &Shape::new(vec![])?,
            &[
                input.values().buffer(),
                mask.values().buffer(),
                output.values().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        let keep = prepared.dense_typed(&mask.broadcast_to(input.shape().clone())?)?;
        let local = self.runtime.zeros::<u32>(input.shape().numel())?;
        let (scan, offsets, block_size) = ScanPlan::local(self.runtime, keep.values(), &local)?;
        let length = checked_index(input.shape().numel())?;
        let groups = prepared.index_groups(length.div_ceil(256).max(1));
        let mut meta = vec![
            length,
            checked_index(input.shape().rank())?,
            groups,
            block_size,
            checked_index(input.layout().offset())?,
            checked_index(output.layout().offset())?,
            checked_index(count.layout().offset())?,
        ];
        for (&dim, &stride) in input.shape().dims().iter().zip(input.layout().strides()) {
            meta.extend([checked_index(dim)?, checked_index(stride)?]);
        }
        let kernel = &self.runtime.tensor_index_kernels()?.compact[kind::<T>()];
        // All fallible preparation stays in this temporary program.
        scan.append_to(&mut prepared.batch);
        prepared.index_dispatch(
            kernel,
            &meta,
            &[
                input.values().buffer(),
                keep.values().buffer(),
                local.buffer(),
                offsets.buffer(),
                output.values().buffer(),
                count.values().buffer(),
            ],
            groups,
        )?;
        self.batch.append(prepared.batch);
        Ok(())
    }
}
