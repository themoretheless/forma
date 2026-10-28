use super::{GpuLowTensor, GpuTensor, Result};
use crate::ComputeProgram;
use tensor_core::{ConvOptions, ConvPlan, low_convolution_plan};

impl ComputeProgram<'_> {
    pub fn tensor_conv_low_f32(
        &mut self,
        input: &GpuLowTensor,
        weight: &GpuLowTensor,
        options: &ConvOptions,
    ) -> Result<GpuTensor> {
        let plan = self.low_conv_plan(input, weight, options)?;
        let output = self.index_new(plan.output)?;
        self.tensor_conv_low_f32_into(input, weight, options, &output)?;
        Ok(output)
    }
    pub fn tensor_conv_low_f32_into(
        &mut self,
        input: &GpuLowTensor,
        weight: &GpuLowTensor,
        options: &ConvOptions,
        output: &GpuTensor,
    ) -> Result<()> {
        let plan = self.low_conv_plan(input, weight, options)?;
        self.conv_loaded(
            &plan,
            [input.layout(), weight.layout()],
            [
                input.packed_words().buffer(),
                weight.packed_words().buffer(),
            ],
            Some(input.dtype()),
            output,
        )
    }
    /// Direct low operand loads, f32 accumulation and one final low cast.
    pub fn tensor_conv_low(
        &mut self,
        input: &GpuLowTensor,
        weight: &GpuLowTensor,
        options: &ConvOptions,
    ) -> Result<GpuLowTensor> {
        let plan = self.low_conv_plan(input, weight, options)?;
        let output = self.runtime.zeros_low(input.dtype(), plan.output)?;
        self.tensor_conv_low_into(input, weight, options, &output)?;
        Ok(output)
    }
    /// Appends convolution and casting transactionally. Output may start at
    /// an odd halfword offset; neighboring packed lanes are preserved.
    pub fn tensor_conv_low_into(
        &mut self,
        input: &GpuLowTensor,
        weight: &GpuLowTensor,
        options: &ConvOptions,
        output: &GpuLowTensor,
    ) -> Result<()> {
        let plan = self.low_conv_plan(input, weight, options)?;
        self.low_output(
            output,
            &plan.output,
            input.dtype(),
            &[
                input.packed_words().buffer(),
                weight.packed_words().buffer(),
            ],
        )?;
        let mut prepared = self.runtime.program();
        let wide = prepared.tensor_conv_low_f32(input, weight, options)?;
        prepared.tensor_cast_to_low_into(&wide, output)?;
        self.batch.append(prepared.batch);
        Ok(())
    }
    fn low_conv_plan(
        &self,
        input: &GpuLowTensor,
        weight: &GpuLowTensor,
        options: &ConvOptions,
    ) -> Result<ConvPlan> {
        self.low_check(input)?;
        self.low_check(weight)?;
        Ok(low_convolution_plan(input, weight, options)?)
    }
}
