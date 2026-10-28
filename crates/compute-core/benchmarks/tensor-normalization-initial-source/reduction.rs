use super::{GpuTensor, TensorComputeError, checked_index, index_kernels::kind};
use crate::{ComputeProgram, GpuElement, Reduction};
use tensor_core::{ReduceOp, Shape, mean_shape, reduction_shape};

type Result<T> = std::result::Result<T, TensorComputeError>;

impl ComputeProgram<'_> {
    /// Reduces arbitrary axes over logical f32 or u32 values. Empty axes copy
    /// the input. Empty sums/products are zero/one; empty min/max contractions
    /// with nonempty output are errors. Integer arithmetic wraps modulo 2^32.
    /// Floating-point inputs and intermediate results must be finite.
    pub fn tensor_reduce<T: GpuElement>(
        &mut self,
        op: ReduceOp,
        input: &GpuTensor<T>,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor<T>> {
        self.index_check(input)?;
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        let output = self.index_new(shape)?;
        self.tensor_reduce_into(op, input, axes, keep_dims, &output)?;
        Ok(output)
    }

    pub fn tensor_reduce_into<T: GpuElement>(
        &mut self,
        op: ReduceOp,
        input: &GpuTensor<T>,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor<T>,
    ) -> Result<()> {
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        self.tensor_reduce_plan(op, input, axes, keep_dims, output, shape, false)
    }

    /// Arithmetic mean over arbitrary axes. Division is applied once after
    /// all partial sums; floating-point inputs and intermediates must be finite.
    pub fn tensor_mean(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor> {
        self.index_check(input)?;
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        let output = self.index_new(shape)?;
        self.tensor_mean_into(input, axes, keep_dims, &output)?;
        Ok(output)
    }

    pub fn tensor_mean_into(
        &mut self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor,
    ) -> Result<()> {
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        self.tensor_reduce_plan(ReduceOp::Sum, input, axes, keep_dims, output, shape, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn tensor_reduce_plan<T: GpuElement>(
        &mut self,
        op: ReduceOp,
        input: &GpuTensor<T>,
        axes: &[usize],
        keep_dims: bool,
        output: &GpuTensor<T>,
        shape: Shape,
        mean: bool,
    ) -> Result<()> {
        self.index_check(input)?;
        self.index_output(output, &shape, &[input.values().buffer()])?;
        if axes.is_empty() {
            return self.copy_typed_into(input, output);
        }
        let count = checked_index(shape.numel())?;
        if count == 0 {
            return Ok(());
        }
        // Preserve the measured array sum schedule for complete contiguous
        // f32 contractions. The sealed element type and kind check prove the
        // buffer representation; ownership and aliases were validated above.
        if matches!(op, ReduceOp::Sum)
            && !mean
            && kind::<T>() == 0
            && count == 1
            && input.layout().is_contiguous()
            && input.layout().offset() == 0
            && output.layout().offset() == 0
        {
            let reduction = Reduction::for_runtime(
                self.runtime,
                input.values().buffer(),
                checked_index(input.shape().numel())?,
                output.values().buffer(),
            );
            reduction.append_to(&mut self.batch);
            return Ok(());
        }
        let mut reduced = vec![false; input.shape().rank()];
        for &axis in axes {
            reduced[axis] = true;
        }
        let mut output_axes = Vec::new();
        let mut reduction_axes = Vec::new();
        let mut reduction_dims = Vec::new();
        for (axis, &is_reduced) in reduced.iter().enumerate() {
            let dim = input.shape().dims()[axis];
            let stride = input.layout().strides()[axis];
            if is_reduced {
                reduction_axes.extend([checked_index(dim)?, checked_index(stride)?]);
                reduction_dims.push(dim);
                if keep_dims {
                    output_axes.extend([1, 0]);
                }
            } else {
                output_axes.extend([checked_index(dim)?, checked_index(stride)?]);
            }
        }
        let reduction_count = checked_index(Shape::new(reduction_dims)?.numel())?;
        let parts = reduction_count
            .div_ceil(4096)
            .clamp(1, 256)
            .min((4096 / count).max(1));
        // At most 4096 typed partials (16 KiB); many independent outputs use
        // one workgroup per output instead. Grid stride respects device limits.
        let partials = if parts > 1 {
            Some(self.runtime.zeros::<T>((count * parts) as usize)?)
        } else {
            None
        };
        let operation = match op {
            ReduceOp::Sum => 0,
            ReduceOp::Product => 1,
            ReduceOp::Min => 2,
            ReduceOp::Max => 3,
        };
        // Keep the measured f32 sum shader. Its eight-word ABI and shared
        // schedule are unchanged; other reducers add operation/divisor words.
        let specialized_sum = matches!(op, ReduceOp::Sum) && !mean && kind::<T>() == 0;
        let divisor = if mean { reduction_count } else { 1 };
        let groups = self.index_groups(count * parts);
        let mut meta = vec![
            count,
            checked_index(shape.rank())?,
            checked_index(input.layout().offset())?,
            reduction_count,
            checked_index(axes.len())?,
            if parts > 1 {
                0
            } else {
                checked_index(output.layout().offset())?
            },
            groups,
            parts,
        ];
        if !specialized_sum {
            meta.extend([operation, if parts > 1 { 1 } else { divisor }]);
        }
        meta.extend(output_axes);
        meta.extend(reduction_axes);
        let metadata = self.runtime.upload(&meta)?;
        let kernels = self.runtime.tensor_kernels()?;
        let kernel = if specialized_sum {
            &kernels.sum
        } else {
            &kernels.reduce[kind::<T>()]
        };
        let bindings = kernel.create_bind_group(
            self.runtime.device(),
            &[
                metadata.buffer(),
                input.values().buffer(),
                partials.as_ref().unwrap_or(output.values()).buffer(),
            ],
        );
        // Prepare every allocation and binding before appending either stage.
        let tail = if let Some(partials) = partials {
            let tail_groups = self.index_groups(count);
            let mut tail_meta = vec![
                count,
                1,
                0,
                parts,
                1,
                checked_index(output.layout().offset())?,
                tail_groups,
                1,
            ];
            if !specialized_sum {
                tail_meta.extend([operation, divisor]);
            }
            tail_meta.extend([count, parts, parts, 1]);
            let tail_meta = self.runtime.upload(&tail_meta)?;
            Some((
                kernel.create_bind_group(
                    self.runtime.device(),
                    &[
                        tail_meta.buffer(),
                        partials.buffer(),
                        output.values().buffer(),
                    ],
                ),
                tail_groups,
            ))
        } else {
            None
        };
        self.batch.push(kernel, &bindings, groups);
        if let Some((bindings, groups)) = tail {
            self.batch.push(kernel, &bindings, groups);
        }
        Ok(())
    }
}
