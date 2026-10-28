use crate::{
    CudaError, CudaLowTensor, CudaRuntime, CudaTensor,
    indexing::output,
    low_dispatch::{LowBinaryDispatch, LowUnaryDispatch},
};
use gpu_compute::cuda::PushKernelArg;
use tensor_core::{
    BinaryOp, ReduceOp, TensorLowBackend, TensorLowOpsBackend, TensorReduceBackend, UnaryOp,
    low_binary_shape, mean_shape, reduction_shape,
};

impl TensorLowOpsBackend for CudaRuntime {
    fn unary_low(&self, op: UnaryOp, input: &CudaLowTensor) -> Result<CudaLowTensor, CudaError> {
        self.check(&input.tensor)?;
        let pass = LowUnaryDispatch::new(input.layout(), op, input.dtype)?;
        let mut result = self.zeros_typed::<u16>(input.shape().clone())?;
        if !input.shape().is_empty() {
            let metadata = self.metadata(&pass.geometry.metadata)?;
            pass.launch(
                self,
                input.tensor.storage.as_ref(),
                output(&mut result),
                &metadata,
            )?;
        }
        Ok(CudaLowTensor {
            tensor: result,
            dtype: input.dtype,
        })
    }

    fn binary_low(
        &self,
        op: BinaryOp,
        left: &CudaLowTensor,
        right: &CudaLowTensor,
    ) -> Result<CudaLowTensor, CudaError> {
        self.check(&left.tensor)?;
        self.check(&right.tensor)?;
        // Shared contract rejects mismatched dtypes even for empty operands.
        low_binary_shape(left, right)?;
        let pass = LowBinaryDispatch::new(left.layout(), right.layout(), op, left.dtype)?;
        let mut result = self.zeros_typed::<u16>(pass.geometry.shape.clone())?;
        if !pass.geometry.shape.is_empty() {
            let metadata = self.metadata(&pass.geometry.metadata)?;
            pass.launch(
                self,
                left.tensor.storage.as_ref(),
                right.tensor.storage.as_ref(),
                output(&mut result),
                &metadata,
            )?;
        }
        Ok(CudaLowTensor {
            tensor: result,
            dtype: left.dtype,
        })
    }

    fn reduce_low_f32(
        &self,
        op: ReduceOp,
        input: &CudaLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor, CudaError> {
        self.check(&input.tensor)?;
        let shape = reduction_shape(op, input.shape(), axes, keep_dims)?;
        if axes.is_empty() {
            return self.cast_to_f32(input);
        }
        if shape.is_empty() {
            return self.zeros(shape);
        }
        if input.shape().is_empty() {
            let mut result = self.zeros(shape.clone())?;
            if op == ReduceOp::Product {
                let n = shape.numel() as u64;
                let one = 1f32;
                unsafe {
                    self.device
                        .stream
                        .launch_builder(&self.reduction.fill[0])
                        .arg(output(&mut result))
                        .arg(&n)
                        .arg(&one)
                        .launch(self.config(shape.numel()))?;
                }
            }
            return Ok(result);
        }
        if axes.len() == input.shape().rank() {
            // The only low-input pass reads native u16. Later hierarchy levels
            // share the existing f32 reducer, including bit-exact extrema.
            let partials = self.reduce_all_loaded::<u16, f32>(
                op,
                &input.tensor,
                &self.low.reduce_all,
                Some(input.dtype as u32),
            )?;
            let reduced = if partials.shape().numel() == 1 {
                partials
            } else {
                self.reduce_f32(op, &partials, &[0], false)?
            };
            return self.view(&reduced, reduced.layout().reshape(shape)?);
        }
        self.reduce_axes_loaded(
            op,
            &input.tensor,
            axes,
            shape,
            &self.low.reduce_axes,
            Some(input.dtype as u32),
        )
    }

    fn mean_low_f32(
        &self,
        input: &CudaLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor, CudaError> {
        self.check(&input.tensor)?;
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        if axes.is_empty() {
            return self.cast_to_f32(input);
        }
        if shape.is_empty() {
            return self.zeros(shape);
        }
        let count = (input.shape().numel() / shape.numel()) as f32;
        let sum = self.reduce_low_f32(ReduceOp::Sum, input, axes, keep_dims)?;
        self.unary_op(&sum, 11, count, 0.)
    }
}
