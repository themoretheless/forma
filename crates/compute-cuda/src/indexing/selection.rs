use super::{
    CudaScalar,
    compact_dispatch::{CompactDispatch, CompactInputs, CompactOutputs, NormalizeMaskDispatch},
    dispatch::{CompareDispatch, LoadedCompare, SelectDispatch, SelectInputs},
    output,
};
use crate::{CudaError, CudaRuntime, CudaTensor};
use gpu_compute::cuda::{
    CudaFunction,
    cudarc::driver::{DeviceRepr, ValidAsZeroBits},
};
use tensor_core::{Compacted, CompareOp, ScanOptions};

impl CudaRuntime {
    pub(crate) fn compare_typed<T: CudaScalar>(
        &self,
        op: CompareOp,
        a: &CudaTensor<T>,
        b: &CudaTensor<T>,
    ) -> Result<CudaTensor<u32>, CudaError> {
        self.compare_loaded(op, a, b, &self.indexing.compare[T::KIND], None)
    }

    pub(crate) fn compare_loaded<T: DeviceRepr>(
        &self,
        op: CompareOp,
        a: &CudaTensor<T>,
        b: &CudaTensor<T>,
        kernel: &CudaFunction,
        dtype: Option<u32>,
    ) -> Result<CudaTensor<u32>, CudaError> {
        self.check(a)?;
        self.check(b)?;
        let pass = CompareDispatch::new(op, &a.layout, &b.layout)?;
        let mut out = self.zeros_typed(pass.shape.clone())?;
        if pass.shape.is_empty() {
            return Ok(out);
        }
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            LoadedCompare {
                function: kernel,
                dtype,
            },
            (a.storage.as_ref(), b.storage.as_ref()),
            output(&mut out),
            &metadata,
        )?;
        Ok(out)
    }

    pub(crate) fn select_typed<T: CudaScalar>(
        &self,
        mask: &CudaTensor<u32>,
        yes: &CudaTensor<T>,
        no: &CudaTensor<T>,
    ) -> Result<CudaTensor<T>, CudaError> {
        self.select_loaded(mask, yes, no, &self.indexing.select[T::KIND])
    }

    pub(crate) fn select_loaded<T: DeviceRepr + ValidAsZeroBits>(
        &self,
        mask: &CudaTensor<u32>,
        yes: &CudaTensor<T>,
        no: &CudaTensor<T>,
        kernel: &CudaFunction,
    ) -> Result<CudaTensor<T>, CudaError> {
        self.check(mask)?;
        self.check(yes)?;
        self.check(no)?;
        let pass = SelectDispatch::new::<T>(&mask.layout, &yes.layout, &no.layout)?;
        let mut out = self.zeros_typed(pass.shape.clone())?;
        if pass.shape.is_empty() {
            return Ok(out);
        }
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            kernel,
            SelectInputs {
                mask: mask.storage.as_ref(),
                yes: yes.storage.as_ref(),
                no: no.storage.as_ref(),
            },
            output(&mut out),
            &metadata,
        )?;
        Ok(out)
    }

    pub(crate) fn compact_typed<T: CudaScalar>(
        &self,
        input: &CudaTensor<T>,
        mask: &CudaTensor<u32>,
    ) -> Result<Compacted<CudaTensor<T>, CudaTensor<u32>>, CudaError> {
        self.compact_loaded(input, mask, &self.indexing.compact[T::KIND])
    }

    pub(crate) fn compact_loaded<T: DeviceRepr + ValidAsZeroBits>(
        &self,
        input: &CudaTensor<T>,
        mask: &CudaTensor<u32>,
        kernel: &CudaFunction,
    ) -> Result<Compacted<CudaTensor<T>, CudaTensor<u32>>, CudaError> {
        self.check(input)?;
        self.check(mask)?;
        let normalize = NormalizeMaskDispatch::new(input.shape(), &mask.layout)?;
        let pass = CompactDispatch::new::<T>(&input.layout)?;
        // Fresh eager allocations supply the same reset that prepared replay
        // schedules explicitly on every run, including the empty count scalar.
        let mut values = self.zeros_typed(pass.values_shape.clone())?;
        let mut count = self.zeros_typed(pass.count_shape.clone())?;
        if pass.values_shape.is_empty() {
            return Ok(Compacted { values, count });
        }
        let mut flags = self.zeros_typed::<u32>(normalize.flags_shape.clone())?;
        let metadata = self.metadata(&normalize.geometry.metadata)?;
        normalize.launch(self, mask.storage.as_ref(), output(&mut flags), &metadata)?;
        let prefix = self.scan_typed(
            &flags,
            0,
            ScanOptions {
                inclusive: false,
                reverse: false,
            },
        )?;
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            kernel,
            CompactInputs {
                values: input.storage.as_ref(),
                flags: flags.storage.as_ref(),
                prefix: prefix.storage.as_ref(),
            },
            CompactOutputs {
                values: output(&mut values),
                count: output(&mut count),
            },
            &metadata,
        )?;
        Ok(Compacted { values, count })
    }
}
