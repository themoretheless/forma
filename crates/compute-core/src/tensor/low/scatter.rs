use super::super::indexing::scatter::scatter_metadata;
use super::{GpuLowTensor, GpuTensor, Result, checked_index};
use crate::{ComputeProgram, wgpu};
use tensor_core::{Layout, ScatterOp, Scattered, Shape, low_scatter_updates_shape};
impl ComputeProgram<'_> {
    /// Scatter packed updates with f32 accumulation and one final low rounding.
    /// Replace preserves all raw payloads; finite Min/Max preserve subnormals
    /// and choose -0/+0 respectively. Last logical index wins duplicate Replace.
    pub fn tensor_scatter_low(
        &mut self,
        op: ScatterOp,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        updates: &GpuLowTensor,
        axis: usize,
    ) -> Result<Scattered<GpuLowTensor, GpuTensor<u32>>> {
        self.low_check(input)?;
        self.index_check(indices)?;
        self.low_check(updates)?;
        low_scatter_updates_shape(input, indices.shape(), updates, axis)?;
        let values = self
            .runtime
            .zeros_low(input.dtype(), input.shape().clone())?;
        let invalid_count = self.index_new(Shape::new(vec![])?)?;
        self.tensor_scatter_low_into(op, input, indices, updates, axis, &values, &invalid_count)?;
        Ok(Scattered {
            values,
            invalid_count,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn tensor_scatter_low_into(
        &mut self,
        op: ScatterOp,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        updates: &GpuLowTensor,
        axis: usize,
        output: &GpuLowTensor,
        invalid_count: &GpuTensor<u32>,
    ) -> Result<()> {
        self.low_check(input)?;
        self.index_check(indices)?;
        self.low_check(updates)?;
        let shape = low_scatter_updates_shape(input, indices.shape(), updates, axis)?;
        self.low_output(
            output,
            input.shape(),
            input.dtype(),
            &[
                input.packed_words().buffer(),
                indices.values().buffer(),
                updates.packed_words().buffer(),
                invalid_count.values().buffer(),
            ],
        )?;
        self.index_output(
            invalid_count,
            &Shape::new(vec![])?,
            &[
                input.packed_words().buffer(),
                indices.values().buffer(),
                updates.packed_words().buffer(),
                output.packed_words().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        if matches!(op, ScatterOp::Add | ScatterOp::Multiply) {
            let accumulated = prepared.index_new(input.shape().clone())?;
            prepared.tensor_scatter_low_f32_into(
                op,
                input,
                indices,
                updates,
                axis,
                &accumulated,
                invalid_count,
            )?;
            prepared.tensor_cast_to_low_into(&accumulated, output)?;
        } else {
            prepared.index_count_into(
                indices,
                checked_index(input.shape().dims()[axis])?,
                invalid_count,
            )?;
            prepared.tensor_materialize_low_into(input, output)?;
            prepared.low_scatter_apply(
                op,
                indices,
                updates,
                &shape,
                axis,
                output.layout(),
                output.packed_words().buffer(),
                true,
            )?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
    /// Direct packed update loads into an f32 destination. This retains the
    /// f32 result before low rounding; only the result/base is decoded in full.
    /// Replace follows cast semantics, so only NaN classification is promised.
    pub fn tensor_scatter_low_f32(
        &mut self,
        op: ScatterOp,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        updates: &GpuLowTensor,
        axis: usize,
    ) -> Result<Scattered<GpuTensor, GpuTensor<u32>>> {
        self.low_check(input)?;
        self.index_check(indices)?;
        self.low_check(updates)?;
        low_scatter_updates_shape(input, indices.shape(), updates, axis)?;
        let values = self.index_new(input.shape().clone())?;
        let invalid_count = self.index_new(Shape::new(vec![])?)?;
        self.tensor_scatter_low_f32_into(
            op,
            input,
            indices,
            updates,
            axis,
            &values,
            &invalid_count,
        )?;
        Ok(Scattered {
            values,
            invalid_count,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn tensor_scatter_low_f32_into(
        &mut self,
        op: ScatterOp,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        updates: &GpuLowTensor,
        axis: usize,
        output: &GpuTensor,
        invalid_count: &GpuTensor<u32>,
    ) -> Result<()> {
        self.low_check(input)?;
        self.index_check(indices)?;
        self.low_check(updates)?;
        let shape = low_scatter_updates_shape(input, indices.shape(), updates, axis)?;
        self.index_output(
            output,
            input.shape(),
            &[
                input.packed_words().buffer(),
                indices.values().buffer(),
                updates.packed_words().buffer(),
                invalid_count.values().buffer(),
            ],
        )?;
        self.index_output(
            invalid_count,
            &Shape::new(vec![])?,
            &[
                input.packed_words().buffer(),
                indices.values().buffer(),
                updates.packed_words().buffer(),
                output.values().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        prepared.index_count_into(
            indices,
            checked_index(input.shape().dims()[axis])?,
            invalid_count,
        )?;
        prepared.tensor_cast_to_f32_into(input, output)?;
        prepared.low_scatter_apply(
            op,
            indices,
            updates,
            &shape,
            axis,
            output.layout(),
            output.values().buffer(),
            false,
        )?;
        self.batch.append(prepared.batch);
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn low_scatter_apply(
        &mut self,
        op: ScatterOp,
        indices: &GpuTensor<u32>,
        updates: &GpuLowTensor,
        shape: &Shape,
        axis: usize,
        output: &Layout,
        buffer: &wgpu::Buffer,
        packed: bool,
    ) -> Result<()> {
        if shape.numel() == 0 || output.shape().numel() == 0 {
            return Ok(());
        }
        let axis_length = checked_index(output.shape().dims()[axis])?;
        let owners = self.scatter_owners(op, indices, axis_length)?;
        let update_layout = updates.layout().broadcast_to(shape.clone())?;
        let groups = self.index_groups(checked_index(shape.numel())?.div_ceil(256));
        let mut metadata =
            scatter_metadata(indices.layout(), &update_layout, output, axis, op, groups)?;
        metadata.push(updates.dtype() as u32);
        let kernel = &self.runtime.low_kernels()?.scatter[usize::from(packed)];
        self.index_dispatch(
            kernel,
            &metadata,
            &[
                indices.values().buffer(),
                updates.packed_words().buffer(),
                owners.buffer(),
                buffer,
            ],
            groups,
        )
    }
}
