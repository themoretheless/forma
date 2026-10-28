use super::{GpuTensor, Result, checked_index, kind};
use crate::{ComputeProgram, GpuArray, GpuElement};
use tensor_core::{Layout, ScatterOp, Scattered, Shape, scatter_updates_shape};

impl ComputeProgram<'_> {
    /// Copies the base, then scatters broadcast updates along one axis.
    /// Invalid indices are ignored and counted once per logical index on GPU.
    /// Replace chooses the last logical row-major index among duplicates.
    /// Integer add/multiply wrap; f32 accumulation order is parallel and every
    /// input and intermediate must remain finite. Highly contended f32 writes
    /// and integer products may retry atomic compare-and-swap operations.
    pub fn tensor_scatter<T: GpuElement>(
        &mut self,
        op: ScatterOp,
        input: &GpuTensor<T>,
        indices: &GpuTensor<u32>,
        updates: &GpuTensor<T>,
        axis: usize,
    ) -> Result<Scattered<GpuTensor<T>, GpuTensor<u32>>> {
        self.index_check(input)?;
        self.index_check(indices)?;
        self.index_check(updates)?;
        scatter_updates_shape(input.shape(), indices.shape(), updates.shape(), axis)?;
        let values = self.index_new(input.shape().clone())?;
        let invalid_count = self.index_new(Shape::new(vec![])?)?;
        self.tensor_scatter_into(op, input, indices, updates, axis, &values, &invalid_count)?;
        Ok(Scattered {
            values,
            invalid_count,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn tensor_scatter_into<T: GpuElement>(
        &mut self,
        op: ScatterOp,
        input: &GpuTensor<T>,
        indices: &GpuTensor<u32>,
        updates: &GpuTensor<T>,
        axis: usize,
        output: &GpuTensor<T>,
        invalid_count: &GpuTensor<u32>,
    ) -> Result<()> {
        self.index_check(input)?;
        self.index_check(indices)?;
        self.index_check(updates)?;
        let shape = scatter_updates_shape(input.shape(), indices.shape(), updates.shape(), axis)?;
        self.index_output(
            output,
            input.shape(),
            &[
                input.values().buffer(),
                indices.values().buffer(),
                updates.values().buffer(),
                invalid_count.values().buffer(),
            ],
        )?;
        self.index_output(
            invalid_count,
            &Shape::new(vec![])?,
            &[
                input.values().buffer(),
                indices.values().buffer(),
                updates.values().buffer(),
                output.values().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        let axis_length = checked_index(input.shape().dims()[axis])?;
        prepared.index_count_into(indices, axis_length, invalid_count)?;
        prepared.copy_typed_into(input, output)?;
        let count = checked_index(shape.numel())?;
        // Empty destination slices still require independent index validation,
        // but do not need owner scratch or any update dispatch.
        if count > 0 && input.shape().numel() > 0 {
            let update_layout = updates.layout().broadcast_to(shape.clone())?;
            let kernels = self.runtime.tensor_scatter_kernels()?;
            let owners = prepared.scatter_owners(op, indices, axis_length)?;
            let groups = self.index_groups(count.div_ceil(256));
            let metadata = scatter_metadata(
                indices.layout(),
                &update_layout,
                output.layout(),
                axis,
                op,
                groups,
            )?;
            prepared.index_dispatch(
                &kernels.apply[kind::<T>()],
                &metadata,
                &[
                    indices.values().buffer(),
                    updates.values().buffer(),
                    owners.buffer(),
                    output.values().buffer(),
                ],
                groups,
            )?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
    /// Owner scratch and resets are shared by typed and low-format scatter.
    pub(in crate::tensor) fn scatter_owners(
        &mut self,
        op: ScatterOp,
        indices: &GpuTensor<u32>,
        axis_length: u32,
    ) -> Result<GpuArray<u32>> {
        let kernels = self.runtime.tensor_scatter_kernels()?;
        let owners = self.runtime.zeros::<u32>(if op == ScatterOp::Replace {
            axis_length as usize
        } else {
            1
        })?;
        if op == ScatterOp::Replace {
            let reset_groups = self.index_groups(axis_length.div_ceil(256));
            self.index_dispatch(
                &kernels.reset,
                &[axis_length, reset_groups],
                &[owners.buffer()],
                reset_groups,
            )?;
            let index_count = checked_index(indices.shape().numel())?;
            let groups = self.index_groups(index_count.div_ceil(256));
            let mut metadata = vec![
                index_count,
                checked_index(indices.shape().rank())?,
                groups,
                checked_index(indices.layout().offset())?,
                axis_length,
            ];
            for (&dim, &stride) in indices
                .shape()
                .dims()
                .iter()
                .zip(indices.layout().strides())
            {
                metadata.extend([checked_index(dim)?, checked_index(stride)?]);
            }
            self.index_dispatch(
                &kernels.owners,
                &metadata,
                &[indices.values().buffer(), owners.buffer()],
                groups,
            )?;
        }

        Ok(owners)
    }
}

/// Common checked update-to-destination traversal metadata for all storage types.
pub(in crate::tensor) fn scatter_metadata(
    indices: &Layout,
    updates: &Layout,
    output: &Layout,
    axis: usize,
    op: ScatterOp,
    groups: u32,
) -> Result<Vec<u32>> {
    let mut metadata = vec![
        checked_index(updates.shape().numel())?,
        checked_index(updates.shape().rank())?,
        groups,
        checked_index(indices.offset())?,
        checked_index(updates.offset())?,
        checked_index(output.offset())?,
        checked_index(output.shape().dims()[axis])?,
        checked_index(output.strides()[axis])?,
        op as u32,
    ];
    for original in 0..axis {
        metadata.extend([
            checked_index(output.shape().dims()[original])?,
            checked_index(output.strides()[original])?,
            0,
            checked_index(updates.strides()[original])?,
            0,
        ]);
    }
    let index_layout = Layout::contiguous(indices.shape().clone())?;
    for index_axis in 0..indices.shape().rank() {
        metadata.extend([
            checked_index(indices.shape().dims()[index_axis])?,
            0,
            checked_index(indices.strides()[index_axis])?,
            checked_index(updates.strides()[axis + index_axis])?,
            checked_index(index_layout.strides()[index_axis])?,
        ]);
    }
    for original in axis + 1..output.shape().rank() {
        let update_axis = original - 1 + indices.shape().rank();
        metadata.extend([
            checked_index(output.shape().dims()[original])?,
            checked_index(output.strides()[original])?,
            0,
            checked_index(updates.strides()[update_axis])?,
            0,
        ]);
    }
    Ok(metadata)
}
