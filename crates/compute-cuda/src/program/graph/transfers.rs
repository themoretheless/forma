use super::super::storage::{StorageMut, StorageRef};
use super::*;
use crate::indexing::dispatch::CopyDispatch;
use gpu_compute::cuda::CudaSlice;

pub(super) struct TransferPlan {
    pub pass: CopyDispatch,
    dtype: CudaDtype,
}
impl TransferPlan {
    pub fn new(spec: &TensorSpec) -> Result<Self, CudaError> {
        let pass = match spec.dtype {
            CudaDtype::F32 => CopyDispatch::new::<f32>(&spec.layout)?,
            CudaDtype::U32 => CopyDispatch::new::<u32>(&spec.layout)?,
            _ => CopyDispatch::new::<u16>(&spec.layout)?,
        };
        Ok(Self {
            pass,
            dtype: spec.dtype,
        })
    }
    pub fn metadata_bytes(&self) -> Result<usize, CudaError> {
        if self.pass.count == 0 {
            return Ok(0);
        }
        self.pass
            .metadata
            .len()
            .max(1)
            .checked_mul(8)
            .ok_or(CudaError::InvalidInput(
                "CUDA graph transfer metadata overflows",
            ))
    }
    pub fn prepare(self, runtime: &CudaRuntime) -> Result<Transfer, CudaError> {
        let metadata = if self.pass.count == 0 {
            None
        } else {
            Some(runtime.metadata(&self.pass.metadata)?)
        };
        Ok(Transfer {
            plan: self,
            metadata,
        })
    }
}
pub(super) struct Transfer {
    plan: TransferPlan,
    metadata: Option<CudaSlice<u64>>,
}
impl Transfer {
    pub fn launch(
        &self,
        rt: &CudaRuntime,
        input: StorageRef<'_>,
        output: StorageMut<'_>,
    ) -> Result<(), CudaError> {
        if self.plan.pass.count == 0 {
            return Ok(());
        }
        let metadata = self.metadata.as_ref().expect("nonempty transfer metadata");
        match (self.plan.dtype, input, output) {
            (CudaDtype::F32, StorageRef::F32(input), StorageMut::F32(output)) => self
                .plan
                .pass
                .launch(rt, &rt.indexing.copy[0], input, output, metadata),
            (CudaDtype::U32, StorageRef::U32(input), StorageMut::U32(output)) => self
                .plan
                .pass
                .launch(rt, &rt.indexing.copy[1], input, output, metadata),
            (dtype, StorageRef::Low(input, a), StorageMut::Low(output, b))
                if a == b && dtype.low_dtype() == Some(a) =>
            {
                self.plan
                    .pass
                    .launch(rt, &rt.low.copy, input, output, metadata)
            }
            _ => Err(CudaError::Dtype),
        }
    }
}
