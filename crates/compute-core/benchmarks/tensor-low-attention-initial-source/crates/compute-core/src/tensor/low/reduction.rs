use super::{GpuLowTensor, GpuTensor, Result, checked_index};
use crate::ComputeProgram;
use tensor_core::{ReduceOp, Shape, mean_shape, reduction_shape};

impl ComputeProgram<'_> {
    /// Decode on load and reduce in f32 without expanding the input tensor.
    /// Min/Max preserve exact finite selected values, including subnormals.
    pub fn tensor_reduce_low_f32(
        &mut self,
        op: ReduceOp,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor> {
        self.low_check(input)?;
        let output = self.index_new(reduction_shape(op, input.shape(), axes, keep_dims)?)?;
        self.tensor_reduce_low_f32_into(op, input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_reduce_low_f32_into(
        &mut self,
        op: ReduceOp,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        self.low_reduce_plan(op, input, axes, keep_dims, output, shape, false)
    }
    /// F32 sum then one division, with finite f32 accumulation required.
    pub fn tensor_mean_low_f32(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor> {
        self.low_check(input)?;
        let output = self.index_new(mean_shape(input.shape(), axes, keep_dims)?)?;
        self.tensor_mean_low_f32_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_mean_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        self.low_reduce_plan(ReduceOp::Sum, input, axes, keep_dims, output, shape, true)
    }
    /// Reduces in f32 and rounds once into the input low dtype.
    pub fn tensor_reduce_low(
        &mut self,
        op: ReduceOp,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        let output = self.runtime.zeros_low(input.dtype(), shape)?;
        self.tensor_reduce_low_into(op, input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_reduce_low_into(
        &mut self,
        op: ReduceOp,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.low_check(input)?;
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        self.low_output(
            output,
            &shape,
            input.dtype(),
            &[input.packed_words().buffer()],
        )?;
        if axes.is_empty() {
            return self.tensor_materialize_low_into(input, output);
        }
        let mut prepared = self.runtime.program();
        let result = prepared.tensor_reduce_low_f32(op, input, axes, keep_dims)?;
        prepared.tensor_cast_to_low_into(&result, output)?;
        self.batch.append(prepared.batch);
        Ok(())
    }
    pub fn tensor_mean_low(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuLowTensor> {
        self.low_check(input)?;
        let output = self
            .runtime
            .zeros_low(input.dtype(), mean_shape(input.shape(), axes, keep_dims)?)?;
        self.tensor_mean_low_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }
    pub fn tensor_mean_low_into(
        &mut self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuLowTensor,
    ) -> Result<()> {
        self.low_check(input)?;
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        self.low_output(
            output,
            &shape,
            input.dtype(),
            &[input.packed_words().buffer()],
        )?;
        if axes.is_empty() {
            return self.tensor_materialize_low_into(input, output);
        }
        let mut prepared = self.runtime.program();
        let result = prepared.tensor_mean_low_f32(input, axes, keep_dims)?;
        prepared.tensor_cast_to_low_into(&result, output)?;
        self.batch.append(prepared.batch);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn low_reduce_plan(
        &mut self,
        op: ReduceOp,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor,
        shape: Shape,
        mean: bool,
    ) -> Result<()> {
        self.low_check(input)?;
        self.index_output(output, &shape, &[input.packed_words().buffer()])?;
        if axes.is_empty() {
            return self.tensor_cast_to_f32_into(input, output);
        }
        let count = checked_index(shape.numel())?;
        if count == 0 {
            return Ok(());
        }
        let mut reduced = vec![false; input.shape().rank()];
        for &axis in axes {
            reduced[axis] = true;
        }
        let mut row_descriptors = Vec::new();
        let mut red_descriptors = Vec::new();
        let mut red_dims = Vec::new();
        for (axis, &is_reduced) in reduced.iter().enumerate() {
            let dim = input.shape().dims()[axis];
            let stride = input.layout().strides()[axis];
            if is_reduced {
                red_dims.push(dim);
                red_descriptors.extend([checked_index(dim)?, checked_index(stride)?]);
                if keep_dims {
                    row_descriptors.extend([1, 0]);
                }
            } else {
                row_descriptors.extend([checked_index(dim)?, checked_index(stride)?]);
            }
        }
        let red_count = checked_index(Shape::new(red_dims)?.numel())?;
        let parts = red_count
            .div_ceil(4096)
            .clamp(1, 256)
            .min((4096 / count).max(1));
        let groups = self.index_groups(count * parts);
        let divisor = if mean { red_count } else { 1 };
        let mut metadata = vec![
            count,
            checked_index(shape.rank())?,
            checked_index(input.layout().offset())?,
            red_count,
            checked_index(axes.len())?,
            if parts > 1 {
                0
            } else {
                checked_index(output.layout().offset())?
            },
            groups,
            parts,
            op as u32,
            if parts > 1 { 1 } else { divisor },
            input.dtype() as u32,
            0,
        ];
        metadata.extend(row_descriptors);
        metadata.extend(red_descriptors);
        let partials = if parts > 1 {
            Some(self.runtime.zeros::<f32>((count * parts) as usize)?)
        } else {
            None
        };
        let kernel = &self.runtime.low_kernels()?.reduce;
        let mut prepared = self.runtime.program();
        prepared.index_dispatch(
            kernel,
            &metadata,
            &[
                input.packed_words().buffer(),
                partials.as_ref().unwrap_or(output.values()).buffer(),
            ],
            groups,
        )?;
        if let Some(partials) = partials {
            let groups = self.index_groups(count);
            let tail = [
                count,
                1,
                0,
                parts,
                1,
                checked_index(output.layout().offset())?,
                groups,
                1,
                op as u32,
                divisor,
                input.dtype() as u32,
                1,
                count,
                parts,
                parts,
                1,
            ];
            prepared.index_dispatch(
                kernel,
                &tail,
                &[partials.buffer(), output.values().buffer()],
                groups,
            )?;
        }
        self.batch.append(prepared.batch);
        Ok(())
    }
}
