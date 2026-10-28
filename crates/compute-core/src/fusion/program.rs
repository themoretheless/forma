use super::{FusedKernel, FusedSumKernel, FusionError};
use crate::{
    ComputeError, ComputeProgram, GpuArray, Reduction,
    gpu_compute::{GpuBuffer, pack_u32},
    wgpu,
};

impl<'a> ComputeProgram<'a> {
    /// Adds one dispatch for the whole expression, allocating only its final
    /// outputs. Inputs have `len` elements or one broadcast scalar. Explicit len
    /// also supports constant-only expressions and empty arrays.
    pub fn fused(
        &mut self,
        kernel: &'a FusedKernel,
        inputs: &[&GpuArray<f32>],
        len: usize,
    ) -> Result<Vec<GpuArray<f32>>, FusionError> {
        self.check_fused_inputs(kernel, inputs, len)?;
        let output = (0..kernel.outputs)
            .map(|_| self.runtime.zeros(len))
            .collect::<Result<Vec<_>, _>>()?;
        self.fused_into(kernel, inputs, &output.iter().collect::<Vec<_>>())?;
        Ok(output)
    }
    fn check_fused_inputs(
        &self,
        kernel: &FusedKernel,
        inputs: &[&GpuArray<f32>],
        len: usize,
    ) -> Result<(), FusionError> {
        if len > u32::MAX as usize {
            return Err(ComputeError::TooLarge {
                bytes: (len as u64).saturating_mul(4),
                limit: u64::from(u32::MAX) * 4,
            }
            .into());
        }
        if !kernel.context.same_device(self.runtime.context()) {
            return Err(FusionError::ForeignDevice);
        }
        if inputs.len() != kernel.inputs {
            return Err(FusionError::InputCount {
                expected: kernel.inputs,
                actual: inputs.len(),
            });
        }
        for input in inputs {
            self.runtime.check(input)?;
            if input.len() != len && input.len() != 1 {
                return Err(ComputeError::LengthMismatch {
                    expected: len,
                    actual: input.len(),
                }
                .into());
            }
        }
        Ok(())
    }
    /// Reuses distinct output buffers; invalid owner/shape/aliases leave the
    /// program unchanged. Bindings and uniforms are created once and retained.
    pub fn fused_into(
        &mut self,
        kernel: &'a FusedKernel,
        inputs: &[&GpuArray<f32>],
        outputs: &[&GpuArray<f32>],
    ) -> Result<(), FusionError> {
        if outputs.len() != kernel.outputs {
            return Err(FusionError::OutputCount {
                expected: kernel.outputs,
                actual: outputs.len(),
            });
        }
        let len = outputs[0].len();
        self.check_fused_inputs(kernel, inputs, len)?;
        for (i, output) in outputs.iter().enumerate() {
            self.runtime.check(output)?;
            if output.len() != len {
                return Err(ComputeError::LengthMismatch {
                    expected: len,
                    actual: output.len(),
                }
                .into());
            }
            if inputs
                .iter()
                .chain(outputs[..i].iter())
                .any(|other| other.aliases(output))
            {
                return Err(ComputeError::AliasedOutput.into());
            }
        }
        if len == 0 {
            return Ok(());
        }
        let bindings = self.bind_fused(kernel, inputs, outputs, len)?;
        let groups = kernel.kernel.workgroup_count(len as u32).min(
            self.runtime
                .device()
                .limits()
                .max_compute_workgroups_per_dimension
                .min(65535),
        );
        self.batch.push(&kernel.kernel, &bindings, groups);
        Ok(())
    }

    fn bind_fused(
        &self,
        kernel: &FusedKernel,
        inputs: &[&GpuArray<f32>],
        outputs: &[&GpuArray<f32>],
        len: usize,
    ) -> Result<wgpu::BindGroup, FusionError> {
        let mut words = vec![len as u32, 0, 0, 0];
        words.extend(
            kernel
                .used_inputs
                .iter()
                .map(|&slot| u32::from(inputs[slot].len() != 1)),
        );
        words.resize(words.len().div_ceil(4) * 4, 0);
        let params = GpuBuffer::new(
            self.runtime.context(),
            words.len() as u64 * 4,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        )
        .map_err(ComputeError::from)?;
        self.runtime
            .queue()
            .write_buffer(params.raw(), 0, &pack_u32(&words));
        let mut views = vec![params.view(0..params.size()).map_err(ComputeError::from)?];
        // Empty reductions still dispatch to reset their output to zero. The
        // loop reads no inputs, but WGSL runtime-array bindings need one word.
        // Typed allocations reserve at least four bytes even when len is zero.
        for &slot in &kernel.used_inputs {
            let input = inputs[slot];
            views.push(
                input
                    .allocation
                    .buffer
                    .view(0..(input.len() as u64 * 4).max(4))
                    .map_err(ComputeError::from)?,
            );
        }
        views.extend(outputs.iter().map(|array| array.view()));
        Ok(kernel.kernel.bind(self.runtime.context(), &views)?)
    }

    /// Evaluates and sums a compiled expression without allocating an array of
    /// mapped values. Intermediate storage contains only workgroup partials.
    pub fn fused_sum(
        &mut self,
        kernel: &'a FusedSumKernel,
        inputs: &[&GpuArray<f32>],
        len: usize,
    ) -> Result<GpuArray<f32>, FusionError> {
        self.check_fused_inputs(&kernel.0, inputs, len)?;
        let output = self.runtime.zeros(1)?;
        self.fused_sum_into(kernel, inputs, len, &output)?;
        Ok(output)
    }

    /// Reuses a distinct single-f32 output. Empty input actively writes zero on
    /// every execution. Inputs and partials remain on the GPU; the summation
    /// order differs from ordinary `sum`, so compare using numerical tolerances.
    pub fn fused_sum_into(
        &mut self,
        kernel: &'a FusedSumKernel,
        inputs: &[&GpuArray<f32>],
        len: usize,
        output: &GpuArray<f32>,
    ) -> Result<(), FusionError> {
        let kernel = &kernel.0;
        self.check_fused_inputs(kernel, inputs, len)?;
        self.runtime.check(output)?;
        if output.len() != 1 {
            return Err(ComputeError::LengthMismatch {
                expected: 1,
                actual: output.len(),
            }
            .into());
        }
        if inputs.iter().any(|input| input.aliases(output)) {
            return Err(ComputeError::AliasedOutput.into());
        }
        let groups = (len as u32)
            .div_ceil(kernel.kernel.workgroup_size() * 8)
            .clamp(
                1,
                self.runtime
                    .device()
                    .limits()
                    .max_compute_workgroups_per_dimension
                    .min(65535),
            );
        let partials = if groups == 1 {
            output.clone()
        } else {
            self.runtime.zeros(groups as usize)?
        };
        let bindings = self.bind_fused(kernel, inputs, &[&partials], len)?;
        // Prepare every fallible resource before appending any work.
        let tail = (groups > 1).then(|| {
            Reduction::with_output(
                self.runtime.device(),
                self.runtime.queue(),
                &self.runtime.sum,
                partials.buffer(),
                groups,
                output.buffer(),
            )
        });
        self.batch.push(&kernel.kernel, &bindings, groups);
        if let Some(tail) = tail {
            self.batch.push_reduction(&tail);
        }
        Ok(())
    }
}
