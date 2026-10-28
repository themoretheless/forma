use super::{
    CudaScalar, output,
    scan_dispatch::{LoadedScan, ScanOutputs, ScanPass},
};
use crate::{CudaError, CudaRuntime, CudaTensor};
use gpu_compute::cuda::{CudaFunction, cudarc::driver::DeviceRepr};
use tensor_core::ScanOptions;

impl CudaRuntime {
    pub(crate) fn scan_typed<T: CudaScalar>(
        &self,
        input: &CudaTensor<T>,
        axis: usize,
        options: ScanOptions,
    ) -> Result<CudaTensor<T>, CudaError> {
        self.scan_loaded(input, axis, options, &self.indexing.scan[T::KIND], None)
    }

    // Only the first pass varies by source storage. Totals, recursive scans,
    // carries and the final output all use the accumulator type O.
    pub(crate) fn scan_loaded<I: DeviceRepr, O: CudaScalar>(
        &self,
        input: &CudaTensor<I>,
        axis: usize,
        options: ScanOptions,
        kernel: &CudaFunction,
        dtype: Option<u32>,
    ) -> Result<CudaTensor<O>, CudaError> {
        self.check(input)?;
        let pass =
            ScanPass::new::<I, O>(&input.layout, axis, options, self.device.multiprocessors)?;
        let mut out = self.zeros_typed(pass.output_shape.clone())?;
        if pass.output_shape.is_empty() {
            return Ok(out);
        }
        let mut totals = self.zeros_typed::<O>(pass.totals_shape.clone())?;
        let metadata = self.metadata(&pass.metadata)?;
        pass.launch(
            self,
            LoadedScan {
                function: kernel,
                dtype,
            },
            input.storage.as_ref(),
            ScanOutputs {
                values: output(&mut out),
                totals: output(&mut totals),
            },
            &metadata,
        )?;
        if let Some(carry) = pass.carry() {
            let offsets = self.scan_typed(
                &totals,
                1,
                ScanOptions {
                    inclusive: false,
                    reverse: false,
                },
            )?;
            carry.launch(
                self,
                &self.indexing.add_scan[O::KIND],
                output(&mut out),
                offsets.storage.as_ref(),
            )?;
        }
        Ok(out)
    }
}
