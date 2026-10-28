//! Native common f32/u32 tensor API over the shader recorder. Each arithmetic
//! or indexing call submits its GPU work; ComputeProgram is the batching API.
use crate::{ComputeRuntime, GpuTensor, TensorComputeError};
use std::time::Duration;
use tensor_core::{
    BackendKind, BinaryOp, MatmulPrecision, Shape, TensorBackend, TensorError, UnaryOp,
};

impl tensor_core::TensorConvBackend for ComputeRuntime {
    fn conv(
        &self,
        input: &GpuTensor,
        weight: &GpuTensor,
        options: &tensor_core::ConvOptions,
    ) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let output = program.tensor_conv(input, weight, options)?;
        program.submit();
        Ok(output)
    }
}

impl tensor_core::TensorAttentionBackend for ComputeRuntime {
    fn attention(
        &self,
        query: &GpuTensor,
        key: &GpuTensor,
        value: &GpuTensor,
        mask: tensor_core::AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: tensor_core::AttentionOptions,
    ) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_attention(query, key, value, mask, options)?;
        program.submit();
        Ok(result)
    }
}

impl tensor_core::TensorStatsBackend for ComputeRuntime {
    fn softmax(&self, input: &GpuTensor, axes: &[usize]) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_softmax(input, axes)?;
        program.submit();
        Ok(result)
    }
    fn log_softmax(&self, input: &GpuTensor, axes: &[usize]) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_log_softmax(input, axes)?;
        program.submit();
        Ok(result)
    }
    fn logsumexp(
        &self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_logsumexp(input, axes, keep_dims)?;
        program.submit();
        Ok(result)
    }
    fn moments(
        &self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<tensor_core::Moments<GpuTensor>, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_moments(input, axes, keep_dims)?;
        program.submit();
        Ok(result)
    }
    fn layer_norm(
        &self,
        input: &GpuTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_layer_norm(input, axes, epsilon)?;
        program.submit();
        Ok(result)
    }
}

impl TensorBackend for ComputeRuntime {
    type Tensor = GpuTensor;
    type Error = TensorComputeError;

    fn kind(&self) -> BackendKind {
        BackendKind::Wgsl
    }

    fn f64_support(&self) -> tensor_core::Float64Support {
        tensor_core::Float64Support::SoftwareBinary64
    }

    fn upload_f32(&self, shape: Shape, values: &[f32]) -> Result<GpuTensor, Self::Error> {
        if shape.numel() != values.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: values.len(),
            }
            .into());
        }
        GpuTensor::from_array(self.upload(values)?, shape)
    }

    fn read_f32(&self, tensor: &GpuTensor) -> Result<Vec<f32>, Self::Error> {
        self.check(tensor.values())?;
        let read = if tensor.layout().is_contiguous() && tensor.layout().offset() == 0 {
            self.read(&tensor.values().prefix(tensor.shape().numel())?)?
        } else {
            let mut program = self.program();
            let dense = program.tensor_materialize(tensor)?;
            program.submit_read(dense.values())?
        };
        Ok(read.wait(Duration::from_secs(30))?)
    }

    fn materialize(&self, tensor: &GpuTensor) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_materialize(tensor)?;
        program.submit();
        Ok(result)
    }

    fn reshape(&self, tensor: &GpuTensor, shape: Shape) -> Result<GpuTensor, Self::Error> {
        self.check(tensor.values())?;
        if shape.numel() != tensor.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        if tensor.layout().is_contiguous() {
            tensor.reshape(shape)
        } else {
            self.materialize(tensor)?.reshape(shape)
        }
    }

    fn permute(&self, tensor: &GpuTensor, axes: &[usize]) -> Result<GpuTensor, Self::Error> {
        self.check(tensor.values())?;
        tensor.permute(axes)
    }

    fn broadcast_to(&self, tensor: &GpuTensor, shape: Shape) -> Result<GpuTensor, Self::Error> {
        self.check(tensor.values())?;
        tensor.broadcast_to(shape)
    }

    fn unary(&self, op: UnaryOp, input: &GpuTensor) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_unary(op, input)?;
        program.submit();
        Ok(result)
    }

    fn binary(
        &self,
        op: BinaryOp,
        left: &GpuTensor,
        right: &GpuTensor,
    ) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_binary(op, left, right)?;
        program.submit();
        Ok(result)
    }

    fn sum_axes(
        &self,
        input: &GpuTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_sum(input, axes, keep_dims)?;
        program.submit();
        Ok(result)
    }

    fn matmul(
        &self,
        left: &GpuTensor,
        right: &GpuTensor,
        precision: MatmulPrecision,
    ) -> Result<GpuTensor, Self::Error> {
        if precision != MatmulPrecision::F32 {
            return Err(TensorComputeError::UnsupportedPrecision(precision));
        }
        let mut program = self.program();
        let result = program.tensor_matmul(left, right)?;
        program.submit();
        Ok(result)
    }
}

impl tensor_core::TensorIndexBackend for ComputeRuntime {
    type UIntTensor = GpuTensor<u32>;
    fn upload_u32(&self, shape: Shape, values: &[u32]) -> Result<Self::UIntTensor, Self::Error> {
        if shape.numel() != values.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: values.len(),
            }
            .into());
        }
        GpuTensor::from_array(self.upload(values)?, shape)
    }
    fn read_u32(&self, tensor: &Self::UIntTensor) -> Result<Vec<u32>, Self::Error> {
        self.check(tensor.values())?;
        let ticket = if tensor.layout().is_contiguous() && tensor.layout().offset() == 0 {
            self.read(&tensor.values().prefix(tensor.shape().numel())?)?
        } else {
            let mut p = self.program();
            let dense = p.tensor_materialize_u32(tensor)?;
            p.submit_read(dense.values())?
        };
        Ok(ticket.wait(Duration::from_secs(30))?)
    }
    fn materialize_u32(&self, tensor: &Self::UIntTensor) -> Result<Self::UIntTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_materialize_u32(tensor)?;
        p.submit();
        Ok(result)
    }
    fn reshape_u32(
        &self,
        tensor: &Self::UIntTensor,
        shape: Shape,
    ) -> Result<Self::UIntTensor, Self::Error> {
        self.check(tensor.values())?;
        if shape.numel() != tensor.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        if tensor.layout().is_contiguous() {
            tensor.reshape(shape)
        } else {
            self.materialize_u32(tensor)?.reshape(shape)
        }
    }
    fn permute_u32(
        &self,
        tensor: &Self::UIntTensor,
        axes: &[usize],
    ) -> Result<Self::UIntTensor, Self::Error> {
        self.check(tensor.values())?;
        tensor.permute(axes)
    }
    fn broadcast_u32(
        &self,
        tensor: &Self::UIntTensor,
        shape: Shape,
    ) -> Result<Self::UIntTensor, Self::Error> {
        self.check(tensor.values())?;
        tensor.broadcast_to(shape)
    }
    fn compare(
        &self,
        op: tensor_core::CompareOp,
        a: &Self::Tensor,
        b: &Self::Tensor,
    ) -> Result<Self::UIntTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_compare(op, a, b)?;
        p.submit();
        Ok(result)
    }
    fn compare_u32(
        &self,
        op: tensor_core::CompareOp,
        a: &Self::UIntTensor,
        b: &Self::UIntTensor,
    ) -> Result<Self::UIntTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_compare(op, a, b)?;
        p.submit();
        Ok(result)
    }
    fn select_f32(
        &self,
        mask: &Self::UIntTensor,
        a: &Self::Tensor,
        b: &Self::Tensor,
    ) -> Result<Self::Tensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_select(mask, a, b)?;
        p.submit();
        Ok(result)
    }
    fn select_u32(
        &self,
        mask: &Self::UIntTensor,
        a: &Self::UIntTensor,
        b: &Self::UIntTensor,
    ) -> Result<Self::UIntTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_select(mask, a, b)?;
        p.submit();
        Ok(result)
    }
    fn scan_f32(
        &self,
        input: &Self::Tensor,
        axis: usize,
        options: tensor_core::ScanOptions,
    ) -> Result<Self::Tensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_scan(input, axis, options)?;
        p.submit();
        Ok(result)
    }
    fn scan_u32(
        &self,
        input: &Self::UIntTensor,
        axis: usize,
        options: tensor_core::ScanOptions,
    ) -> Result<Self::UIntTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_scan_u32(input, axis, options)?;
        p.submit();
        Ok(result)
    }
    fn gather_f32(
        &self,
        input: &Self::Tensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<tensor_core::Gathered<Self::Tensor, Self::UIntTensor>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_gather(input, indices, axis)?;
        p.submit();
        Ok(result)
    }
    fn gather_u32(
        &self,
        input: &Self::UIntTensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<tensor_core::Gathered<Self::UIntTensor, Self::UIntTensor>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_gather(input, indices, axis)?;
        p.submit();
        Ok(result)
    }
    fn compact_f32(
        &self,
        input: &Self::Tensor,
        mask: &Self::UIntTensor,
    ) -> Result<tensor_core::Compacted<Self::Tensor, Self::UIntTensor>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_compact(input, mask)?;
        p.submit();
        Ok(result)
    }
    fn compact_u32(
        &self,
        input: &Self::UIntTensor,
        mask: &Self::UIntTensor,
    ) -> Result<tensor_core::Compacted<Self::UIntTensor, Self::UIntTensor>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_compact(input, mask)?;
        p.submit();
        Ok(result)
    }
}

impl tensor_core::TensorReduceBackend for ComputeRuntime {
    fn reduce_f32(
        &self,
        op: tensor_core::ReduceOp,
        input: &Self::Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        let mut program = self.program();
        let output = program.tensor_reduce(op, input, axes, keep_dims)?;
        program.submit();
        Ok(output)
    }

    fn reduce_u32(
        &self,
        op: tensor_core::ReduceOp,
        input: &Self::UIntTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::UIntTensor, Self::Error> {
        let mut program = self.program();
        let output = program.tensor_reduce(op, input, axes, keep_dims)?;
        program.submit();
        Ok(output)
    }

    fn mean_axes(
        &self,
        input: &Self::Tensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<Self::Tensor, Self::Error> {
        let mut program = self.program();
        let output = program.tensor_mean(input, axes, keep_dims)?;
        program.submit();
        Ok(output)
    }
}

impl tensor_core::TensorScatterBackend for ComputeRuntime {
    fn scatter_f32(
        &self,
        op: tensor_core::ScatterOp,
        input: &Self::Tensor,
        indices: &Self::UIntTensor,
        updates: &Self::Tensor,
        axis: usize,
    ) -> Result<tensor_core::Scattered<Self::Tensor, Self::UIntTensor>, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_scatter(op, input, indices, updates, axis)?;
        program.submit();
        Ok(result)
    }

    fn scatter_u32(
        &self,
        op: tensor_core::ScatterOp,
        input: &Self::UIntTensor,
        indices: &Self::UIntTensor,
        updates: &Self::UIntTensor,
        axis: usize,
    ) -> Result<tensor_core::Scattered<Self::UIntTensor, Self::UIntTensor>, Self::Error> {
        let mut program = self.program();
        let result = program.tensor_scatter(op, input, indices, updates, axis)?;
        program.submit();
        Ok(result)
    }
}
