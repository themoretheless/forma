//! Native low-precision facade. Upload/read cross the host boundary; views,
//! casts and matrix products preserve device-resident intermediate values.
use crate::{ComputeRuntime, GpuLowTensor, GpuTensor, TensorComputeError};
use std::time::Duration;
use tensor_core::{LowDtype, LowPrecisionSupport, Shape, TensorLowBackend};
impl TensorLowBackend for ComputeRuntime {
    type LowTensor = GpuLowTensor;
    fn low_precision_support(&self, dtype: LowDtype) -> LowPrecisionSupport {
        ComputeRuntime::low_precision_support(self, dtype)
    }
    fn upload_low(
        &self,
        dtype: LowDtype,
        shape: Shape,
        bits: &[u16],
    ) -> Result<GpuLowTensor, TensorComputeError> {
        self.upload_low_bits(dtype, shape, bits)
    }
    fn read_low_bits(&self, tensor: &GpuLowTensor) -> Result<Vec<u16>, TensorComputeError> {
        self.check(tensor.packed_words())?;
        let mut p = self.program();
        let dense = if tensor.layout().is_contiguous() && tensor.layout().offset() == 0 {
            tensor.clone()
        } else {
            p.tensor_materialize_low(tensor)?
        };
        let words = dense
            .packed_words()
            .prefix(tensor.shape().numel().div_ceil(2))?;
        let values = p.submit_read(&words)?.wait(Duration::from_secs(30))?;
        Ok(values
            .into_iter()
            .flat_map(|v| [v as u16, (v >> 16) as u16])
            .take(tensor.shape().numel())
            .collect())
    }
    fn materialize_low(&self, tensor: &GpuLowTensor) -> Result<GpuLowTensor, TensorComputeError> {
        let mut p = self.program();
        let result = p.tensor_materialize_low(tensor)?;
        p.submit();
        Ok(result)
    }
    fn reshape_low(
        &self,
        tensor: &GpuLowTensor,
        shape: Shape,
    ) -> Result<GpuLowTensor, TensorComputeError> {
        self.check(tensor.packed_words())?;
        if tensor.shape().numel() != shape.numel() {
            return Err(tensor_core::TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        if tensor.layout().is_contiguous() {
            tensor.reshape(shape)
        } else {
            self.materialize_low(tensor)?.reshape(shape)
        }
    }
    fn permute_low(
        &self,
        tensor: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuLowTensor, TensorComputeError> {
        self.check(tensor.packed_words())?;
        tensor.permute(axes)
    }
    fn broadcast_low(
        &self,
        tensor: &GpuLowTensor,
        shape: Shape,
    ) -> Result<GpuLowTensor, TensorComputeError> {
        self.check(tensor.packed_words())?;
        tensor.broadcast_to(shape)
    }
    fn cast_to_low(
        &self,
        tensor: &GpuTensor,
        dtype: LowDtype,
    ) -> Result<GpuLowTensor, TensorComputeError> {
        let mut p = self.program();
        let result = p.tensor_cast_to_low(tensor, dtype)?;
        p.submit();
        Ok(result)
    }
    fn cast_to_f32(&self, tensor: &GpuLowTensor) -> Result<GpuTensor, TensorComputeError> {
        let mut p = self.program();
        let result = p.tensor_cast_to_f32(tensor)?;
        p.submit();
        Ok(result)
    }
    fn matmul_low(
        &self,
        left: &GpuLowTensor,
        right: &GpuLowTensor,
    ) -> Result<GpuLowTensor, TensorComputeError> {
        let mut p = self.program();
        let result = p.tensor_matmul_low(left, right)?;
        p.submit();
        Ok(result)
    }
    fn matmul_low_f32(
        &self,
        left: &GpuLowTensor,
        right: &GpuLowTensor,
    ) -> Result<GpuTensor, TensorComputeError> {
        let mut p = self.program();
        let result = p.tensor_matmul_low_f32(left, right)?;
        p.submit();
        Ok(result)
    }
}

impl tensor_core::TensorLowOpsBackend for ComputeRuntime {
    fn unary_low(
        &self,
        op: tensor_core::UnaryOp,
        input: &GpuLowTensor,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_unary_low(op, input)?;
        p.submit();
        Ok(result)
    }
    fn binary_low(
        &self,
        op: tensor_core::BinaryOp,
        left: &GpuLowTensor,
        right: &GpuLowTensor,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_binary_low(op, left, right)?;
        p.submit();
        Ok(result)
    }
    fn reduce_low_f32(
        &self,
        op: tensor_core::ReduceOp,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_reduce_low_f32(op, input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
    fn mean_low_f32(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_mean_low_f32(input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
    fn reduce_low(
        &self,
        op: tensor_core::ReduceOp,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_reduce_low(op, input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
    fn mean_low(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_mean_low(input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
}

impl tensor_core::TensorLowIndexBackend for ComputeRuntime {
    fn compare_low(
        &self,
        op: tensor_core::CompareOp,
        left: &GpuLowTensor,
        right: &GpuLowTensor,
    ) -> Result<GpuTensor<u32>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_compare_low(op, left, right)?;
        p.submit();
        Ok(result)
    }
    fn select_low(
        &self,
        mask: &GpuTensor<u32>,
        yes: &GpuLowTensor,
        no: &GpuLowTensor,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_select_low(mask, yes, no)?;
        p.submit();
        Ok(result)
    }
    fn gather_low(
        &self,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        axis: usize,
    ) -> Result<tensor_core::Gathered<GpuLowTensor, GpuTensor<u32>>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_gather_low(input, indices, axis)?;
        p.submit();
        Ok(result)
    }
    fn compact_low(
        &self,
        input: &GpuLowTensor,
        mask: &GpuTensor<u32>,
    ) -> Result<tensor_core::Compacted<GpuLowTensor, GpuTensor<u32>>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_compact_low(input, mask)?;
        p.submit();
        Ok(result)
    }
    fn scan_low_f32(
        &self,
        input: &GpuLowTensor,
        axis: usize,
        options: tensor_core::ScanOptions,
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_scan_low_f32(input, axis, options)?;
        p.submit();
        Ok(result)
    }
    fn scan_low(
        &self,
        input: &GpuLowTensor,
        axis: usize,
        options: tensor_core::ScanOptions,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_scan_low(input, axis, options)?;
        p.submit();
        Ok(result)
    }
}

impl tensor_core::TensorLowScatterBackend for ComputeRuntime {
    fn scatter_low(
        &self,
        op: tensor_core::ScatterOp,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        updates: &GpuLowTensor,
        axis: usize,
    ) -> Result<tensor_core::Scattered<GpuLowTensor, GpuTensor<u32>>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_scatter_low(op, input, indices, updates, axis)?;
        p.submit();
        Ok(result)
    }
    fn scatter_low_f32(
        &self,
        op: tensor_core::ScatterOp,
        input: &GpuLowTensor,
        indices: &GpuTensor<u32>,
        updates: &GpuLowTensor,
        axis: usize,
    ) -> Result<tensor_core::Scattered<GpuTensor, GpuTensor<u32>>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_scatter_low_f32(op, input, indices, updates, axis)?;
        p.submit();
        Ok(result)
    }
}

impl tensor_core::TensorLowStatsBackend for ComputeRuntime {
    fn softmax_low_f32(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_softmax_low_f32(input, axes)?;
        p.submit();
        Ok(result)
    }
    fn log_softmax_low_f32(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_log_softmax_low_f32(input, axes)?;
        p.submit();
        Ok(result)
    }
    fn logsumexp_low_f32(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_logsumexp_low_f32(input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
    fn moments_low_f32(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<tensor_core::Moments<GpuTensor>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_moments_low_f32(input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
    fn layer_norm_low_f32(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_layer_norm_low_f32(input, axes, epsilon)?;
        p.submit();
        Ok(result)
    }
    fn softmax_low(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_softmax_low(input, axes)?;
        p.submit();
        Ok(result)
    }
    fn log_softmax_low(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_log_softmax_low(input, axes)?;
        p.submit();
        Ok(result)
    }
    fn logsumexp_low(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_logsumexp_low(input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
    fn moments_low(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        keep_dims: bool,
    ) -> Result<tensor_core::Moments<GpuLowTensor>, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_moments_low(input, axes, keep_dims)?;
        p.submit();
        Ok(result)
    }
    fn layer_norm_low(
        &self,
        input: &GpuLowTensor,
        axes: &[usize],
        epsilon: f32,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_layer_norm_low(input, axes, epsilon)?;
        p.submit();
        Ok(result)
    }
}

impl tensor_core::TensorLowAttentionBackend for ComputeRuntime {
    fn attention_low_f32(
        &self,
        query: &GpuLowTensor,
        key: &GpuLowTensor,
        value: &GpuLowTensor,
        mask: tensor_core::AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: tensor_core::AttentionOptions,
    ) -> Result<GpuTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_attention_low_f32(query, key, value, mask, options)?;
        p.submit();
        Ok(result)
    }
    fn attention_low(
        &self,
        query: &GpuLowTensor,
        key: &GpuLowTensor,
        value: &GpuLowTensor,
        mask: tensor_core::AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: tensor_core::AttentionOptions,
    ) -> Result<GpuLowTensor, Self::Error> {
        let mut p = self.program();
        let result = p.tensor_attention_low(query, key, value, mask, options)?;
        p.submit();
        Ok(result)
    }
}
