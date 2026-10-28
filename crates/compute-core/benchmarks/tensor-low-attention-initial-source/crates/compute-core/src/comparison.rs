use crate::{CompareOp, ComputeError, ComputeProgram, GpuArray, uniform_f32};

impl ComputeProgram<'_> {
    /// Compares finite f32 values and produces exact zero/one u32 masks without
    /// a CPU readback. Equal-length operands or a scalar on either side are
    /// supported, with the same broadcasting rules as [`Self::binary`].
    pub fn compare(
        &mut self,
        op: CompareOp,
        a: &GpuArray<f32>,
        b: &GpuArray<f32>,
    ) -> Result<GpuArray<u32>, ComputeError> {
        let len = self.binary_length(a, b)?;
        let output = self.runtime.zeros(len)?;
        self.compare_into(op, a, b, &output)?;
        Ok(output)
    }

    /// Reuses output storage of the broadcast length. Even differently typed
    /// views of an input allocation are rejected as output aliases.
    pub fn compare_into(
        &mut self,
        op: CompareOp,
        a: &GpuArray<f32>,
        b: &GpuArray<f32>,
        output: &GpuArray<u32>,
    ) -> Result<(), ComputeError> {
        let len = self.binary_length(a, b)?;
        self.runtime.check(output)?;
        if output.len() != len {
            return Err(ComputeError::LengthMismatch {
                expected: len,
                actual: output.len(),
            });
        }
        if [a.buffer(), b.buffer()].contains(&output.buffer()) {
            return Err(ComputeError::AliasedOutput);
        }
        if len == 0 {
            return Ok(());
        }
        let kernel = &self.runtime.compare;
        let params = uniform_f32(
            &self.runtime.device,
            &self.runtime.queue,
            &[
                f32::from_bits(len as u32),
                f32::from_bits(op as u32),
                f32::from_bits(u32::from(a.len() != 1)),
                f32::from_bits(u32::from(b.len() != 1)),
            ],
        );
        let bindings = kernel.create_bind_group(
            &self.runtime.device,
            &[&params, a.buffer(), b.buffer(), output.buffer()],
        );
        self.batch.push(
            kernel,
            &bindings,
            kernel.workgroup_count(len as u32).min(65535),
        );
        Ok(())
    }
}
