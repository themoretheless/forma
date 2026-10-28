use crate::{CudaError, CudaRuntime, CudaTensor};
use tensor_core::{
    Compacted, CompareOp, Gathered, ScanOptions, ScatterOp, Scattered, TensorF64IndexBackend,
    TensorF64ScatterBackend,
};

impl TensorF64IndexBackend for CudaRuntime {
    fn compare_f64(
        &self,
        op: CompareOp,
        left: &CudaTensor<f64>,
        right: &CudaTensor<f64>,
    ) -> Result<CudaTensor<u32>, CudaError> {
        self.compare_typed(op, left, right)
    }
    fn select_f64(
        &self,
        mask: &CudaTensor<u32>,
        on_true: &CudaTensor<f64>,
        on_false: &CudaTensor<f64>,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.select_typed(mask, on_true, on_false)
    }
    fn scan_f64(
        &self,
        input: &CudaTensor<f64>,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaTensor<f64>, CudaError> {
        self.scan_typed(input, axis, options)
    }
    fn gather_f64(
        &self,
        input: &CudaTensor<f64>,
        indices: &CudaTensor<u32>,
        axis: usize,
    ) -> Result<Gathered<CudaTensor<f64>, CudaTensor<u32>>, CudaError> {
        self.gather_typed(input, indices, axis)
    }
    fn compact_f64(
        &self,
        input: &CudaTensor<f64>,
        mask: &CudaTensor<u32>,
    ) -> Result<Compacted<CudaTensor<f64>, CudaTensor<u32>>, CudaError> {
        self.compact_typed(input, mask)
    }
}
impl TensorF64ScatterBackend for CudaRuntime {
    fn scatter_f64(
        &self,
        op: ScatterOp,
        input: &CudaTensor<f64>,
        indices: &CudaTensor<u32>,
        updates: &CudaTensor<f64>,
        axis: usize,
    ) -> Result<Scattered<CudaTensor<f64>, CudaTensor<u32>>, CudaError> {
        self.scatter_typed(op, input, indices, updates, axis)
    }
}

#[cfg(test)]
mod tests {
    use crate::indexing::{
        dispatch::{CopyDispatch, SelectDispatch},
        gather_dispatch::GatherDispatch,
        scan_dispatch::ScanPass,
    };
    use tensor_core::{Layout, ScanOptions, Shape};
    fn repeated(dims: &[usize]) -> Layout {
        Layout::new(Shape::new(dims.to_vec()).unwrap(), vec![0; dims.len()], 0).unwrap()
    }
    #[test]
    fn f64_indexing_descriptors_bound_actual_output_and_scan_widths() {
        let huge = repeated(&[usize::MAX / 8 + 1]);
        let scalar = repeated(&[]);
        assert!(CopyDispatch::new::<f32>(&huge).is_ok());
        assert!(CopyDispatch::new::<f64>(&huge).is_err());
        assert!(SelectDispatch::new::<f32>(&scalar, &huge, &scalar).is_ok());
        assert!(SelectDispatch::new::<f64>(&scalar, &huge, &scalar).is_err());
        assert!(ScanPass::new::<f32, f32>(&huge, 0, ScanOptions::default(), 40).is_ok());
        assert!(ScanPass::new::<f64, f64>(&huge, 0, ScanOptions::default(), 40).is_err());
        let input = repeated(&[usize::MAX / 16 + 1, 1]);
        let indices = repeated(&[2]);
        assert!(GatherDispatch::new::<f32>(&input, &indices, 1).is_ok());
        assert!(GatherDispatch::new::<f64>(&input, &indices, 1).is_err());
    }
}
