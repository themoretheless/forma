//! Resident packed Q/K/V attention, sharing the tiled/split-key planner.
use super::{GpuLowTensor, GpuTensor, Result};
use crate::tensor::attention::AttentionKernels;
use crate::{ComputeProgram, ComputeRuntime, KernelError};
use tensor_core::{AttentionMask, AttentionOptions, AttentionPlan, LowDtype};

impl ComputeRuntime {
    fn low_attention_kernels(
        &self,
        dtype: LowDtype,
    ) -> std::result::Result<&AttentionKernels, KernelError> {
        let slot = &self.tensor_low_attention[dtype as usize];
        if let Some(kernels) = slot.get() {
            return Ok(kernels);
        }
        let kernels =
            AttentionKernels::new(self.device(), &super::attention_source::source(dtype))?;
        let _ = slot.set(kernels);
        Ok(slot.get().expect("low attention kernels initialized"))
    }
}
impl ComputeProgram<'_> {
    /// Direct packed Q/K/V loads, online softmax and bounded f32 accumulation.
    /// No complete expanded operand or score/probability matrix is allocated.
    pub fn tensor_attention_low_f32(
        &mut self,
        query: &GpuLowTensor,
        key: &GpuLowTensor,
        value: &GpuLowTensor,
        mask: AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: AttentionOptions,
    ) -> Result<GpuTensor> {
        let plan = self.low_attention_plan(query, key, value, &mask, options)?;
        let output = self.index_new(plan.output)?;
        self.tensor_attention_low_f32_into(query, key, value, mask, options, &output)?;
        Ok(output)
    }
    /// Records the evaluated f32 result into a distinct contiguous view.
    /// The result is never rounded through the input low dtype.
    pub fn tensor_attention_low_f32_into(
        &mut self,
        query: &GpuLowTensor,
        key: &GpuLowTensor,
        value: &GpuLowTensor,
        mask: AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: AttentionOptions,
        output: &GpuTensor,
    ) -> Result<()> {
        let plan = self.low_attention_plan(query, key, value, &mask, options)?;
        self.attention_plan_loaded(
            plan,
            [query.layout(), key.layout(), value.layout()],
            [
                query.packed_words().buffer(),
                key.packed_words().buffer(),
                value.packed_words().buffer(),
            ],
            mask,
            output,
            self.runtime.low_attention_kernels(query.dtype())?,
            &[],
        )
    }
    /// Records f32 accumulation and one final nearest-even cast to Q's dtype.
    pub fn tensor_attention_low(
        &mut self,
        query: &GpuLowTensor,
        key: &GpuLowTensor,
        value: &GpuLowTensor,
        mask: AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: AttentionOptions,
    ) -> Result<GpuLowTensor> {
        let plan = self.low_attention_plan(query, key, value, &mask, options)?;
        let output = self.runtime.zeros_low(query.dtype(), plan.output)?;
        self.tensor_attention_low_into(query, key, value, mask, options, &output)?;
        Ok(output)
    }
    /// All validation and final casting are appended transactionally. Odd
    /// output halfword offsets preserve adjacent physical storage.
    pub fn tensor_attention_low_into(
        &mut self,
        query: &GpuLowTensor,
        key: &GpuLowTensor,
        value: &GpuLowTensor,
        mask: AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: AttentionOptions,
        output: &GpuLowTensor,
    ) -> Result<()> {
        let plan = self.low_attention_plan(query, key, value, &mask, options)?;
        let mut inputs = vec![
            query.packed_words().buffer(),
            key.packed_words().buffer(),
            value.packed_words().buffer(),
        ];
        match &mask {
            AttentionMask::None => {}
            AttentionMask::Keep(t) => {
                self.index_check(t)?;
                inputs.push(t.values().buffer());
            }
            AttentionMask::Additive(t) => {
                self.index_check(t)?;
                inputs.push(t.values().buffer());
            }
        }
        self.low_output(output, &plan.output, query.dtype(), &inputs)?;
        let mut prepared = self.runtime.program();
        let result = prepared.tensor_attention_low_f32(query, key, value, mask, options)?;
        prepared.tensor_cast_to_low_into(&result, output)?;
        self.batch.append(prepared.batch);
        Ok(())
    }
    fn low_attention_plan(
        &self,
        query: &GpuLowTensor,
        key: &GpuLowTensor,
        value: &GpuLowTensor,
        mask: &AttentionMask<'_, GpuTensor, GpuTensor<u32>>,
        options: AttentionOptions,
    ) -> Result<AttentionPlan> {
        self.low_check(query)?;
        self.low_check(key)?;
        self.low_check(value)?;
        Ok(tensor_core::low_attention_plan(
            query,
            key,
            value,
            mask.shape(),
            options,
        )?)
    }
}
