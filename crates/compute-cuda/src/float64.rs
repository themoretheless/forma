//! Native binary64 tensors; storage and all arithmetic remain double precision.
mod indexing;
use crate::{
    CudaError, CudaRuntime, CudaTensor,
    dispatch::{BinaryDispatch, UnaryDispatch},
    indexing::output,
    matmul::GemmPlan,
};
use gpu_compute::cuda::{CudaFunction, CudaModule, cudarc::cublas::sys};
use std::sync::Arc;
use tensor_core::{
    BinaryOp, MatmulPlan, ReduceOp, Shape, TensorError, TensorF64Backend, UnaryOp, mean_shape,
};

#[derive(Clone)]
pub(crate) struct F64Kernels {
    unary: CudaFunction,
    binary: CudaFunction,
}
impl F64Kernels {
    pub(crate) fn load(module: &Arc<CudaModule>) -> Result<Self, CudaError> {
        Ok(Self {
            unary: module.load_function("unary_f64")?,
            binary: module.load_function("binary_f64")?,
        })
    }
}
impl CudaRuntime {
    fn unary_op_f64(
        &self,
        input: &CudaTensor<f64>,
        op: u32,
        scale: f64,
        bias: f64,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.check(input)?;
        let pass = UnaryDispatch::new(&input.layout, op, scale, bias)?;
        let mut result = self.zeros_typed(pass.shape.clone())?;
        if !pass.shape.is_empty() {
            let metadata = self.metadata(&pass.metadata)?;
            pass.launch_loaded(
                self,
                &self.float64.unary,
                input.storage.as_ref(),
                output(&mut result),
                &metadata,
            )?;
        }
        Ok(result)
    }
    /// Updates unique contiguous storage, rejecting aliased or sliced outputs.
    pub fn write_f64(&self, tensor: &mut CudaTensor<f64>, values: &[f64]) -> Result<(), CudaError> {
        self.check(tensor)?;
        if values.len() != tensor.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: values.len(),
            }
            .into());
        }
        if !tensor.layout.is_contiguous() || tensor.layout.offset() != 0 {
            return Err(CudaError::SharedOutput);
        }
        let storage = Arc::get_mut(&mut tensor.storage).ok_or(CudaError::SharedOutput)?;
        if !values.is_empty() {
            self.device.stream.memcpy_htod(values, storage)?;
        }
        Ok(())
    }
    pub fn affine_f64(
        &self,
        input: &CudaTensor<f64>,
        scale: f64,
        bias: f64,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.unary_op_f64(input, 9, scale, bias)
    }
}
impl TensorF64Backend for CudaRuntime {
    type F64Tensor = CudaTensor<f64>;
    fn upload_f64(&self, shape: Shape, values: &[f64]) -> Result<CudaTensor<f64>, CudaError> {
        if shape.numel() != values.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: values.len(),
            }
            .into());
        }
        let mut tensor = self.zeros_typed(shape)?;
        self.write_f64(&mut tensor, values)?;
        Ok(tensor)
    }
    fn read_f64(&self, tensor: &CudaTensor<f64>) -> Result<Vec<f64>, CudaError> {
        self.check(tensor)?;
        if tensor.shape().is_empty() {
            return Ok(Vec::new());
        }
        let contiguous = self.materialize_f64(tensor)?;
        let start = contiguous.layout.offset();
        Ok(self.device.stream.clone_dtoh(
            &contiguous
                .storage
                .slice(start..start + contiguous.shape().numel()),
        )?)
    }
    fn materialize_f64(&self, tensor: &CudaTensor<f64>) -> Result<CudaTensor<f64>, CudaError> {
        self.check(tensor)?;
        if tensor.layout.is_contiguous() {
            Ok(tensor.clone())
        } else {
            self.copy_typed(tensor)
        }
    }
    fn reshape_f64(
        &self,
        tensor: &CudaTensor<f64>,
        shape: Shape,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.check(tensor)?;
        if shape.numel() != tensor.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        let contiguous = self.materialize_f64(tensor)?;
        self.view(&contiguous, contiguous.layout.reshape(shape)?)
    }
    fn permute_f64(
        &self,
        tensor: &CudaTensor<f64>,
        axes: &[usize],
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.view(tensor, tensor.layout.permute(axes)?)
    }
    fn broadcast_f64(
        &self,
        tensor: &CudaTensor<f64>,
        shape: Shape,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.view(tensor, tensor.layout.broadcast_to(shape)?)
    }
    fn narrow_f64(
        &self,
        tensor: &CudaTensor<f64>,
        axis: usize,
        start: usize,
        len: usize,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.narrow(tensor, axis, start, len)
    }
    fn unary_f64(
        &self,
        op: UnaryOp,
        input: &CudaTensor<f64>,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.unary_op_f64(input, op as u32, 1., 0.)
    }
    fn binary_f64(
        &self,
        op: BinaryOp,
        left: &CudaTensor<f64>,
        right: &CudaTensor<f64>,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.check(left)?;
        self.check(right)?;
        let pass = BinaryDispatch::new_typed::<f64>(&left.layout, &right.layout, op)?;
        let mut result = self.zeros_typed(pass.shape.clone())?;
        if !pass.shape.is_empty() {
            let metadata = self.metadata(&pass.metadata)?;
            pass.launch_loaded(
                self,
                &self.float64.binary,
                (left.storage.as_ref(), right.storage.as_ref()),
                output(&mut result),
                &metadata,
            )?;
        }
        Ok(result)
    }
    fn reduce_f64(
        &self,
        op: ReduceOp,
        input: &CudaTensor<f64>,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.reduce_typed(op, input, axes, keep_dims)
    }
    fn mean_f64(
        &self,
        input: &CudaTensor<f64>,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.check(input)?;
        let shape = mean_shape(input.shape(), axes, keep_dims)?;
        if axes.is_empty() {
            return self.materialize_f64(input);
        }
        if shape.is_empty() {
            return self.zeros_typed(shape);
        }
        let denominator = (input.shape().numel() / shape.numel()) as f64;
        let sum = self.reduce_f64(ReduceOp::Sum, input, axes, keep_dims)?;
        self.unary_op_f64(&sum, 11, denominator, 0.)
    }
    fn matmul_f64(
        &self,
        left: &CudaTensor<f64>,
        right: &CudaTensor<f64>,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.check(left)?;
        self.check(right)?;
        let shape = MatmulPlan::new(left.shape(), right.shape())?;
        crate::runtime::validate_logical_size::<f64>(&shape.matrix_output)?;
        if shape.matrix_output.is_empty() || *shape.left.dims().last().unwrap() == 0 {
            return self.zeros_typed(shape.output);
        }
        let left = self.reshape_f64(left, shape.left)?;
        let right = self.reshape_f64(right, shape.right)?;
        let plan = GemmPlan::new_typed::<f64>(&left.layout, &right.layout)?;
        let prepared = self.prepare_gemm(
            plan,
            sys::cudaDataType_t::CUDA_R_64F,
            sys::cublasComputeType_t::CUBLAS_COMPUTE_64F_PEDANTIC,
        )?;
        let mut result = self.zeros_typed(prepared.plan().output().clone())?;
        self.launch_gemm(
            &prepared,
            left.storage.as_ref(),
            right.storage.as_ref(),
            output(&mut result),
        )?;
        self.view(&result, result.layout.reshape(shape.output)?)
    }
    fn evaluate_f64(&self, tensors: &[&CudaTensor<f64>]) -> Result<(), CudaError> {
        for tensor in tensors {
            self.check(tensor)?;
        }
        self.synchronize()
    }
}
