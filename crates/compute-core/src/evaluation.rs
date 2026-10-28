use crate::{ComputeRuntime, GpuTensor, TensorComputeError, wgpu};
use tensor_core::TensorEvalBackend;

impl TensorEvalBackend for ComputeRuntime {
    fn evaluate(
        &self,
        floats: &[&GpuTensor],
        integers: &[&GpuTensor<u32>],
    ) -> Result<(), TensorComputeError> {
        for tensor in floats {
            self.check(tensor.values())?;
        }
        for tensor in integers {
            self.check(tensor.values())?;
        }
        // Flush pending queue.write_buffer uploads as well as previously
        // submitted kernels. The exact submission is a stable fence even if
        // another owner of this queue submits more work while we wait.
        let submission = self.queue().submit([]);
        self.device()
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| TensorComputeError::Evaluation(error.to_string()))?;
        Ok(())
    }
}
