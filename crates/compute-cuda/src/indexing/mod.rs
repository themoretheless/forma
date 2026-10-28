//! Typed indexing shares layout validation and traversal across storage formats.
pub(crate) mod compact_dispatch;
pub(crate) mod dispatch;
mod gather;
pub(crate) mod gather_dispatch;
mod scan;
pub(crate) mod scan_dispatch;
mod scatter;
pub(crate) mod scatter_dispatch;
mod selection;

use crate::{CudaError, CudaRuntime, CudaTensor};
use gpu_compute::cuda::{
    CudaFunction, CudaModule, CudaSlice,
    cudarc::driver::{DeviceRepr, ValidAsZeroBits},
};
use std::sync::Arc;
use tensor_core::{
    Compacted, CompareOp, Gathered, ScanOptions, Shape, TensorBackend, TensorError,
    TensorF64Backend, TensorIndexBackend,
};

pub(crate) trait CudaScalar: DeviceRepr + ValidAsZeroBits + Copy + Default {
    const KIND: usize;
    fn materialize(
        runtime: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError>;
}
impl CudaScalar for f32 {
    const KIND: usize = 0;
    fn materialize(
        runtime: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError> {
        runtime.materialize(input)
    }
}
impl CudaScalar for u32 {
    const KIND: usize = 1;
    fn materialize(
        runtime: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError> {
        runtime.materialize_u32(input)
    }
}

impl CudaScalar for f64 {
    const KIND: usize = 2;
    fn materialize(
        runtime: &CudaRuntime,
        input: &CudaTensor<Self>,
    ) -> Result<CudaTensor<Self>, CudaError> {
        runtime.materialize_f64(input)
    }
}

#[derive(Clone)]
pub(crate) struct IndexKernels {
    pub(crate) copy: [CudaFunction; 3],
    pub(crate) compare: [CudaFunction; 3],
    pub(crate) select: [CudaFunction; 3],
    pub(crate) scan: [CudaFunction; 3],
    pub(crate) add_scan: [CudaFunction; 3],
    pub(crate) gather: [CudaFunction; 3],
    pub(crate) compact: [CudaFunction; 3],
    pub(crate) invalid_indices: CudaFunction,
    pub(crate) normalize_mask: CudaFunction,
    pub(crate) scatter_owners: CudaFunction,
    pub(crate) scatter: [CudaFunction; 3],
}
impl IndexKernels {
    pub(crate) fn load(module: &Arc<CudaModule>) -> Result<Self, CudaError> {
        let triple = |f: &str, u: &str, d: &str| -> Result<_, CudaError> {
            Ok([
                module.load_function(f)?,
                module.load_function(u)?,
                module.load_function(d)?,
            ])
        };
        Ok(Self {
            copy: triple("copy_f32", "copy_u32", "copy_f64")?,
            compare: triple("compare_f32", "compare_u32", "compare_f64")?,
            select: triple("where_f32", "where_u32", "where_f64")?,
            scan: triple("scan_f32", "scan_u32", "scan_f64")?,
            add_scan: triple("add_scan_f32", "add_scan_u32", "add_scan_f64")?,
            gather: triple("gather_f32", "gather_u32", "gather_f64")?,
            compact: triple("compact_f32", "compact_u32", "compact_f64")?,
            invalid_indices: module.load_function("invalid_indices")?,
            normalize_mask: module.load_function("normalize_mask")?,
            scatter_owners: module.load_function("scatter_owners")?,
            scatter: triple("scatter_f32", "scatter_u32", "scatter_f64")?,
        })
    }
}

pub(crate) fn output<T>(tensor: &mut CudaTensor<T>) -> &mut CudaSlice<T> {
    // Every caller just allocated this output and has not exposed a view.
    Arc::get_mut(&mut tensor.storage).unwrap()
}

impl CudaRuntime {
    /// Always allocates distinct contiguous storage, including for dense input.
    pub(crate) fn copy_typed<T: CudaScalar>(
        &self,
        tensor: &CudaTensor<T>,
    ) -> Result<CudaTensor<T>, CudaError> {
        self.check(tensor)?;
        let pass = dispatch::CopyDispatch::new::<T>(&tensor.layout)?;
        let mut out = self.zeros_typed(pass.shape.clone())?;
        if pass.shape.is_empty() {
            return Ok(out);
        }
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            &self.indexing.copy[T::KIND],
            tensor.storage.as_ref(),
            output(&mut out),
            &metadata,
        )?;
        Ok(out)
    }
    /// Broadcast unsigned arithmetic. Add, subtract and multiply wrap modulo
    /// 2^32; min/max compare unsigned values. Division is rejected.
    pub fn binary_u32(
        &self,
        op: tensor_core::BinaryOp,
        left: &CudaTensor<u32>,
        right: &CudaTensor<u32>,
    ) -> Result<CudaTensor<u32>, CudaError> {
        if op == tensor_core::BinaryOp::Divide {
            return Err(CudaError::InvalidInput("u32 division is not supported"));
        }
        self.check(left)?;
        self.check(right)?;
        let pass =
            crate::dispatch::BinaryDispatch::new_typed::<u32>(&left.layout, &right.layout, op)?;
        let mut out = self.zeros_typed(pass.shape.clone())?;
        if pass.shape.is_empty() {
            return Ok(out);
        }
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch_u32(
            self,
            left.storage.as_ref(),
            right.storage.as_ref(),
            output(&mut out),
            &metadata,
        )?;
        Ok(out)
    }
    /// Updates unique contiguous u32 storage. Live views reject mutation.
    pub fn write_u32(&self, tensor: &mut CudaTensor<u32>, values: &[u32]) -> Result<(), CudaError> {
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
}

impl TensorIndexBackend for CudaRuntime {
    type UIntTensor = CudaTensor<u32>;

    fn upload_u32(&self, shape: Shape, values: &[u32]) -> Result<Self::UIntTensor, CudaError> {
        if shape.numel() != values.len() {
            return Err(TensorError::ElementCountMismatch {
                expected: shape.numel(),
                actual: values.len(),
            }
            .into());
        }
        let mut tensor = self.zeros_typed(shape)?;
        self.write_u32(&mut tensor, values)?;
        Ok(tensor)
    }
    fn read_u32(&self, tensor: &Self::UIntTensor) -> Result<Vec<u32>, CudaError> {
        self.check(tensor)?;
        if tensor.shape().is_empty() {
            return Ok(Vec::new());
        }
        let tensor = self.materialize_u32(tensor)?;
        let start = tensor.layout.offset();
        Ok(self
            .device
            .stream
            .clone_dtoh(&tensor.storage.slice(start..start + tensor.shape().numel()))?)
    }
    fn materialize_u32(&self, tensor: &Self::UIntTensor) -> Result<Self::UIntTensor, CudaError> {
        self.check(tensor)?;
        if tensor.layout.is_contiguous() {
            return Ok(tensor.clone());
        }
        self.copy_typed(tensor)
    }
    fn reshape_u32(
        &self,
        tensor: &Self::UIntTensor,
        shape: Shape,
    ) -> Result<Self::UIntTensor, CudaError> {
        self.check(tensor)?;
        if shape.numel() != tensor.shape().numel() {
            return Err(TensorError::ElementCountMismatch {
                expected: tensor.shape().numel(),
                actual: shape.numel(),
            }
            .into());
        }
        let tensor = self.materialize_u32(tensor)?;
        self.view(&tensor, tensor.layout.reshape(shape)?)
    }
    fn permute_u32(
        &self,
        tensor: &Self::UIntTensor,
        axes: &[usize],
    ) -> Result<Self::UIntTensor, CudaError> {
        self.view(tensor, tensor.layout.permute(axes)?)
    }
    fn broadcast_u32(
        &self,
        tensor: &Self::UIntTensor,
        shape: Shape,
    ) -> Result<Self::UIntTensor, CudaError> {
        self.view(tensor, tensor.layout.broadcast_to(shape)?)
    }
    fn compare(
        &self,
        op: CompareOp,
        left: &CudaTensor,
        right: &CudaTensor,
    ) -> Result<Self::UIntTensor, CudaError> {
        self.compare_typed(op, left, right)
    }
    fn compare_u32(
        &self,
        op: CompareOp,
        left: &Self::UIntTensor,
        right: &Self::UIntTensor,
    ) -> Result<Self::UIntTensor, CudaError> {
        self.compare_typed(op, left, right)
    }
    fn select_f32(
        &self,
        mask: &Self::UIntTensor,
        yes: &CudaTensor,
        no: &CudaTensor,
    ) -> Result<CudaTensor, CudaError> {
        self.select_typed(mask, yes, no)
    }
    fn select_u32(
        &self,
        mask: &Self::UIntTensor,
        yes: &Self::UIntTensor,
        no: &Self::UIntTensor,
    ) -> Result<Self::UIntTensor, CudaError> {
        self.select_typed(mask, yes, no)
    }
    fn scan_f32(
        &self,
        input: &CudaTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaTensor, CudaError> {
        self.scan_typed(input, axis, options)
    }
    fn scan_u32(
        &self,
        input: &Self::UIntTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<Self::UIntTensor, CudaError> {
        self.scan_typed(input, axis, options)
    }
    fn gather_f32(
        &self,
        input: &CudaTensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Gathered<CudaTensor, Self::UIntTensor>, CudaError> {
        self.gather_typed(input, indices, axis)
    }
    fn gather_u32(
        &self,
        input: &Self::UIntTensor,
        indices: &Self::UIntTensor,
        axis: usize,
    ) -> Result<Gathered<Self::UIntTensor, Self::UIntTensor>, CudaError> {
        self.gather_typed(input, indices, axis)
    }
    fn compact_f32(
        &self,
        input: &CudaTensor,
        mask: &Self::UIntTensor,
    ) -> Result<Compacted<CudaTensor, Self::UIntTensor>, CudaError> {
        self.compact_typed(input, mask)
    }
    fn compact_u32(
        &self,
        input: &Self::UIntTensor,
        mask: &Self::UIntTensor,
    ) -> Result<Compacted<Self::UIntTensor, Self::UIntTensor>, CudaError> {
        self.compact_typed(input, mask)
    }
}
