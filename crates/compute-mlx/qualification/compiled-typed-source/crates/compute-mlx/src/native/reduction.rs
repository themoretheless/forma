use super::*;
use tensor_core::{ReduceOp, TensorReduceBackend, mean_shape, reduction_shape};

impl MlxBackend {
    pub(super) fn reduce_values(
        &self,
        op: ReduceOp,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, None)?;
        let shape = reduction_shape(op, &input.shape, axes, keep_dims)?;
        if axes.is_empty() {
            return Ok(input.clone());
        }
        if shape.is_empty() {
            return self.zeros(shape, input.dtype);
        }
        if input.shape.is_empty() {
            // Common validation permits only sum/product for an empty
            // contraction with nonempty output. Build their native identities;
            // no reduction initializer or host data expansion is needed.
            if op == ReduceOp::Product {
                let dims = Self::dimensions(&shape)?;
                return self.output("empty product", shape, input.dtype, |a, out| unsafe {
                    (a.ones)(
                        out,
                        dims.as_ptr(),
                        dims.len(),
                        input.dtype.raw(),
                        self.context.stream,
                    )
                });
            }
            return self.zeros(shape, input.dtype);
        }
        let operation = reduce_function(&self.context.api, op);
        self.reduce_output("reduce", input, shape, axes, keep_dims, operation)
    }

    fn reduce_output(
        &self,
        name: &'static str,
        input: &MlxTensor,
        shape: Shape,
        axes: &[usize],
        keep_dims: bool,
        operation: ffi::Reduce,
    ) -> Result<MlxTensor, MlxError> {
        let axes: Vec<i32> = axes
            .iter()
            .map(|&axis| i32::try_from(axis).map_err(|_| MlxError::TooLarge))
            .collect::<Result<_, _>>()?;
        self.output(name, shape, input.dtype, |_, out| unsafe {
            operation(
                out,
                input.array.raw,
                axes.as_ptr(),
                axes.len(),
                keep_dims,
                self.context.stream,
            )
        })
    }

    /// Mean over checked f32 axes. Empty axes preserve the input; reducing a
    /// zero-length axis to a nonempty output returns `TensorError::EmptyReduction`.
    pub fn mean_axes(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        let shape = mean_shape(&input.shape, axes, keep_dims)?;
        if axes.is_empty() {
            return Ok(input.clone());
        }
        if shape.is_empty() {
            return self.zeros(shape, MlxDtype::F32);
        }
        self.reduce_output(
            "mean",
            input,
            shape,
            axes,
            keep_dims,
            self.context.api.mean_axes,
        )
    }
}

impl TensorReduceBackend for MlxBackend {
    fn reduce_f32(
        &self,
        op: ReduceOp,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::F32))?;
        self.reduce_values(op, input, axes, keep_dims)
    }

    fn reduce_u32(
        &self,
        op: ReduceOp,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.check(input, Some(MlxDtype::U32))?;
        self.reduce_values(op, input, axes, keep_dims)
    }

    fn mean_axes(
        &self,
        input: &MlxTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<MlxTensor, MlxError> {
        self.mean_axes(input, axes, keep_dims)
    }
}
