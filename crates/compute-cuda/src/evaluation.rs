use crate::{CudaError, CudaRuntime, CudaTensor};
use tensor_core::TensorEvalBackend;

impl TensorEvalBackend for CudaRuntime {
    fn evaluate(
        &self,
        floats: &[&CudaTensor],
        integers: &[&CudaTensor<u32>],
    ) -> Result<(), CudaError> {
        for tensor in floats {
            self.check(tensor)?;
        }
        for tensor in integers {
            self.check(tensor)?;
        }
        self.synchronize()
    }
}
