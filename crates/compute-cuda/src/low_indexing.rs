//! Native two-byte indexing. Only scan decodes values, directly into f32
//! accumulators; movement kernels preserve every raw bit pattern.
use crate::{CudaError, CudaLowTensor, CudaRuntime, CudaTensor};
use tensor_core::{
    Compacted, CompareOp, Gathered, ScanOptions, TensorLowIndexBackend, low_binary_shape,
    low_select_shape,
};

impl TensorLowIndexBackend for CudaRuntime {
    fn compare_low(
        &self,
        op: CompareOp,
        left: &CudaLowTensor,
        right: &CudaLowTensor,
    ) -> Result<CudaTensor<u32>, CudaError> {
        self.check(&left.tensor)?;
        self.check(&right.tensor)?;
        low_binary_shape(left, right)?;
        self.compare_loaded(
            op,
            &left.tensor,
            &right.tensor,
            &self.low.compare,
            Some(left.dtype as u32),
        )
    }

    fn select_low(
        &self,
        mask: &CudaTensor<u32>,
        yes: &CudaLowTensor,
        no: &CudaLowTensor,
    ) -> Result<CudaLowTensor, CudaError> {
        self.check(mask)?;
        self.check(&yes.tensor)?;
        self.check(&no.tensor)?;
        low_select_shape(mask.shape(), yes, no)?;
        Ok(CudaLowTensor {
            tensor: self.select_loaded(mask, &yes.tensor, &no.tensor, &self.low.select)?,
            dtype: yes.dtype,
        })
    }

    fn gather_low(
        &self,
        input: &CudaLowTensor,
        indices: &CudaTensor<u32>,
        axis: usize,
    ) -> Result<Gathered<CudaLowTensor, CudaTensor<u32>>, CudaError> {
        let gathered = self.gather_loaded(&input.tensor, indices, axis, &self.low.gather)?;
        Ok(Gathered {
            values: CudaLowTensor {
                tensor: gathered.values,
                dtype: input.dtype,
            },
            invalid_count: gathered.invalid_count,
        })
    }

    fn compact_low(
        &self,
        input: &CudaLowTensor,
        mask: &CudaTensor<u32>,
    ) -> Result<Compacted<CudaLowTensor, CudaTensor<u32>>, CudaError> {
        let compacted = self.compact_loaded(&input.tensor, mask, &self.low.compact)?;
        Ok(Compacted {
            values: CudaLowTensor {
                tensor: compacted.values,
                dtype: input.dtype,
            },
            count: compacted.count,
        })
    }

    fn scan_low_f32(
        &self,
        input: &CudaLowTensor,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaTensor, CudaError> {
        self.scan_loaded(
            &input.tensor,
            axis,
            options,
            &self.low.scan,
            Some(input.dtype as u32),
        )
    }
}
